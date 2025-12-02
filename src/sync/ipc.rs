use crate::borrow::Cow;
use crate::Error;
use crate::IntoBusRtResult;
use crate::QoS;
use crate::GREETINGS;
use crate::PING_FRAME;
use crate::PROTOCOL_VERSION;
use crate::RESPONSE_OK;
use crate::SECONDARY_SEP;
use crate::{FrameData, FrameKind, FrameOp};
#[cfg(not(feature = "rt"))]
use parking_lot::Mutex;
#[cfg(feature = "rt")]
use rtsc::pi::Mutex;
use std::collections::BTreeMap;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::net::Shutdown;
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::sync::atomic;
use std::sync::Arc;
use std::time::Duration;

use log::{error, trace, warn};

use crate::sync::client::SyncClient;
use crate::SyncEventChannel;
use crate::SyncEventSender;
use crate::SyncOpConfirm;

type ResponseMap = Arc<Mutex<BTreeMap<u32, oneshot::Sender<Result<(), Error>>>>>;

#[derive(Debug, Clone)]
pub struct Config {
    path: String,
    name: String,
    buf_size: usize,
    queue_size: usize,
    timeout: Duration,
}

impl Config {
    /// path - /path/to/socket (must end with .sock .socket or .ipc) or host:port,
    /// name - an unique client name
    pub fn new(path: &str, name: &str) -> Self {
        Self {
            path: path.to_owned(),
            name: name.to_owned(),
            buf_size: crate::DEFAULT_BUF_SIZE,
            queue_size: crate::DEFAULT_QUEUE_SIZE,
            timeout: crate::DEFAULT_TIMEOUT,
        }
    }
    pub fn buf_size(mut self, size: usize) -> Self {
        self.buf_size = size;
        self
    }
    pub fn queue_size(mut self, size: usize) -> Self {
        self.queue_size = size;
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

trait Socket: Read + Write {
    fn shutdown(&self);
}

#[cfg(not(target_os = "windows"))]
impl Socket for UnixStream {
    fn shutdown(&self) {
        let _ = self.shutdown(Shutdown::Both);
    }
}

impl Socket for TcpStream {
    fn shutdown(&self) {
        let _ = self.shutdown(Shutdown::Both);
    }
}

pub struct Client {
    name: String,
    writer: Box<dyn Socket + Send>,
    frame_id: u32,
    responses: ResponseMap,
    rx: Option<SyncEventChannel>,
    connected: Arc<atomic::AtomicBool>,
    timeout: Duration,
    config: Config,
    secondary_counter: atomic::AtomicUsize,
}

// keep these as macros to insure inline and avoid unecc. futures

macro_rules! prepare_frame_buf {
    ($self: expr, $op: expr, $qos: expr, $expected_header_len: expr) => {{
        $self.increment_frame_id();
        let mut buf = Vec::with_capacity($expected_header_len + 4 + 1);
        buf.extend($self.frame_id.to_le_bytes());
        buf.push($op as u8 | ($qos as u8) << 6);
        buf
    }};
}

macro_rules! send_data_or_mark_disconnected {
    ($self: expr, $data: expr) => {
        if let Err(e) = $self.writer.write_all($data) {
            $self.connected.store(false, atomic::Ordering::Relaxed);
            $self.writer.shutdown();
            return Err(e.into());
        }
    };
}

macro_rules! send_frame_and_confirm {
    ($self: expr, $buf: expr, $payload: expr, $qos: expr) => {{
        let rx = if $qos.needs_ack() {
            let (tx, rx) = oneshot::channel();
            {
                $self.responses.lock().insert($self.frame_id, tx);
            }
            Some(rx)
        } else {
            None
        };
        send_data_or_mark_disconnected!($self, $buf);
        send_data_or_mark_disconnected!($self, $payload);
        Ok(rx)
    }};
}

macro_rules! send_zc_frame {
    // zc-send to target or topic
    ($self: expr, $target: expr, $header: expr, $payload: expr, $op: expr, $qos: expr) => {{
        let t = $target.as_bytes();
        let mut buf = prepare_frame_buf!($self, $op, $qos, 4 + t.len() + 1 + $header.len());
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(
            &((t.len() + $payload.len() + $header.len() + 1) as u32).to_le_bytes(),
        );
        buf.extend_from_slice(t);
        buf.push(0x00);
        buf.extend_from_slice($header);
        trace!("sending busrt {:?} to {} QoS={:?}", $op, $target, $qos);
        send_frame_and_confirm!($self, &buf, $payload, $qos)
    }};
}

macro_rules! send_frame {
    // send to target or topic
    ($self: expr, $target: expr, $payload: expr, $op: expr, $qos: expr) => {{
        let t = $target.as_bytes();
        let mut buf = prepare_frame_buf!($self, $op, $qos, 4 + t.len() + 1);
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(&((t.len() + $payload.len() + 1) as u32).to_le_bytes());
        buf.extend_from_slice(t);
        buf.push(0x00);
        trace!("sending busrt {:?} to {} QoS={:?}", $op, $target, $qos);
        send_frame_and_confirm!($self, &buf, $payload, $qos)
    }};
    // send to topic with a receiver
    ($self: expr, $target: expr, $receiver: expr, $payload: expr, $op: expr, $qos: expr) => {{
        let t = $target.as_bytes();
        let r = $receiver.as_bytes();
        let mut buf = prepare_frame_buf!($self, $op, $qos, 4 + t.len() + 1 + r.len() + 1);
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(&((t.len() + r.len() + $payload.len() + 2) as u32).to_le_bytes());
        buf.extend_from_slice(t);
        buf.push(0x00);
        buf.extend_from_slice(r);
        buf.push(0x00);
        trace!("sending busrt {:?} to {} QoS={:?}", $op, $target, $qos);
        send_frame_and_confirm!($self, &buf, $payload, $qos)
    }};
    // send w/o a target
    ($self: expr, $payload: expr, $op: expr, $qos: expr) => {{
        let mut buf = prepare_frame_buf!($self, $op, $qos, 4);
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(&($payload.len() as u32).to_le_bytes());
        send_frame_and_confirm!($self, &buf, $payload, $qos)
    }};
}

macro_rules! connect_broker {
    ($name: expr, $socket: expr, $reader: expr,
         $responses: expr, $connected: expr, $timeout: expr, $queue_size: expr) => {{
        chat($name, &mut $socket)?;
        let (tx, rx) = rtsc::channel::bounded($queue_size);
        let reader_responses = $responses.clone();
        let rconn = $connected.clone();
        (
            rx,
            Reader {
                inner: Box::new($reader),
                tx,
                responses: reader_responses,
                rconn,
            },
        )
    }};
}

impl Client {
    /// Creates a new client instance. The Reader must be started manually by calling
    /// `Reader::run()` (e.g. in a separate thread).
    pub fn connect(config: &Config) -> Result<(Self, Reader), Error> {
        Self::connect_broker(config, None)
    }
    pub fn connect_stream(stream: UnixStream, config: &Config) -> Result<(Self, Reader), Error> {
        Self::connect_broker(config, Some(stream))
    }
    fn connect_broker(
        config: &Config,
        stream: Option<UnixStream>,
    ) -> Result<(Self, Reader), Error> {
        let responses: ResponseMap = <_>::default();
        let connected = Arc::new(atomic::AtomicBool::new(true));
        #[allow(clippy::case_sensitive_file_extension_comparisons)]
        let (writer, rx, reader) = if config.path.ends_with(".sock")
            || config.path.ends_with(".socket")
            || config.path.ends_with(".ipc")
            || config.path.starts_with('/')
        {
            #[cfg(target_os = "windows")]
            {
                return Err(Error::not_supported("unix sockets"));
            }
            #[cfg(not(target_os = "windows"))]
            {
                let mut stream = if let Some(s) = stream {
                    s
                } else {
                    UnixStream::connect(&config.path)?
                };
                stream.set_write_timeout(Some(config.timeout))?;
                let r = stream.try_clone()?;
                let reader = BufReader::with_capacity(config.buf_size, r);
                let (rx, reader) = connect_broker!(
                    &config.name,
                    stream,
                    reader,
                    responses,
                    connected,
                    config.timeout,
                    config.queue_size
                );
                (
                    Box::new(stream) as Box<dyn Socket + Send + 'static>,
                    rx,
                    reader,
                )
            }
        } else {
            let mut stream = TcpStream::connect(&config.path)?;
            stream.set_write_timeout(Some(config.timeout))?;
            stream.set_nodelay(true)?;
            let r = stream.try_clone()?;
            let reader = BufReader::with_capacity(config.buf_size, r);
            let (rx, reader) = connect_broker!(
                &config.name,
                stream,
                reader,
                responses,
                connected,
                config.timeout,
                config.queue_size
            );
            (
                Box::new(stream) as Box<dyn Socket + Send + 'static>,
                rx,
                reader,
            )
        };
        Ok((
            Self {
                name: config.name.clone(),
                writer,
                frame_id: 0,
                responses,
                rx: Some(rx),
                connected,
                timeout: config.timeout,
                config: config.clone(),
                secondary_counter: atomic::AtomicUsize::new(0),
            },
            reader,
        ))
    }
    /// The Reader must be started manually by calling
    /// `Reader::run()` (e.g. in a separate thread).
    pub fn register_secondary(&self) -> Result<(Self, Reader), Error> {
        if self.name.contains(SECONDARY_SEP) {
            Err(Error::not_supported("not a primary client"))
        } else {
            let secondary_id = self
                .secondary_counter
                .fetch_add(1, atomic::Ordering::Relaxed);
            let secondary_name = format!("{}{}{}", self.name, SECONDARY_SEP, secondary_id);
            let mut config = self.config.clone();
            config.name = secondary_name;
            Self::connect(&config)
        }
    }
    #[inline]
    fn increment_frame_id(&mut self) {
        if self.frame_id == u32::MAX {
            self.frame_id = 1;
        } else {
            self.frame_id += 1;
        }
    }
    #[inline]
    pub fn get_timeout(&self) -> Duration {
        self.timeout
    }
}

impl SyncClient for Client {
    #[inline]
    fn take_event_channel(&mut self) -> Option<SyncEventChannel> {
        self.rx.take()
    }
    #[inline]
    fn get_connected_beacon(&self) -> Option<Arc<atomic::AtomicBool>> {
        Some(self.connected.clone())
    }
    fn send(&mut self, target: &str, payload: Cow<'_>, qos: QoS) -> Result<SyncOpConfirm, Error> {
        send_frame!(self, target, payload.as_slice(), FrameOp::Message, qos)
    }
    fn zc_send(
        &mut self,
        target: &str,
        header: Cow<'_>,
        payload: Cow<'_>,
        qos: QoS,
    ) -> Result<SyncOpConfirm, Error> {
        send_zc_frame!(
            self,
            target,
            header.as_slice(),
            payload.as_slice(),
            FrameOp::Message,
            qos
        )
    }
    fn send_broadcast(
        &mut self,
        target: &str,
        payload: Cow<'_>,
        qos: QoS,
    ) -> Result<SyncOpConfirm, Error> {
        send_frame!(self, target, payload.as_slice(), FrameOp::Broadcast, qos)
    }
    fn publish(
        &mut self,
        target: &str,
        payload: Cow<'_>,
        qos: QoS,
    ) -> Result<SyncOpConfirm, Error> {
        send_frame!(self, target, payload.as_slice(), FrameOp::PublishTopic, qos)
    }
    fn publish_for(
        &mut self,
        target: &str,
        receiver: &str,
        payload: Cow<'_>,
        qos: QoS,
    ) -> Result<SyncOpConfirm, Error> {
        send_frame!(
            self,
            target,
            receiver,
            payload.as_slice(),
            FrameOp::PublishTopicFor,
            qos
        )
    }
    fn subscribe(&mut self, topic: &str, qos: QoS) -> Result<SyncOpConfirm, Error> {
        send_frame!(self, topic.as_bytes(), FrameOp::SubscribeTopic, qos)
    }
    fn unsubscribe(&mut self, topic: &str, qos: QoS) -> Result<SyncOpConfirm, Error> {
        send_frame!(self, topic.as_bytes(), FrameOp::UnsubscribeTopic, qos)
    }
    fn subscribe_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<SyncOpConfirm, Error> {
        let mut payload = Vec::new();
        for topic in topics {
            if !payload.is_empty() {
                payload.push(0x00);
            }
            payload.extend(topic.as_bytes());
        }
        send_frame!(self, &payload, FrameOp::SubscribeTopic, qos)
    }
    fn unsubscribe_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<SyncOpConfirm, Error> {
        let mut payload = Vec::new();
        for topic in topics {
            if !payload.is_empty() {
                payload.push(0x00);
            }
            payload.extend(topic.as_bytes());
        }
        send_frame!(self, &payload, FrameOp::UnsubscribeTopic, qos)
    }
    fn exclude(&mut self, topic: &str, qos: QoS) -> Result<SyncOpConfirm, Error> {
        send_frame!(self, topic.as_bytes(), FrameOp::ExcludeTopic, qos)
    }
    fn unexclude(&mut self, topic: &str, qos: QoS) -> Result<SyncOpConfirm, Error> {
        send_frame!(self, topic.as_bytes(), FrameOp::UnexcludeTopic, qos)
    }
    fn exclude_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<SyncOpConfirm, Error> {
        let mut payload = Vec::new();
        for topic in topics {
            if !payload.is_empty() {
                payload.push(0x00);
            }
            payload.extend(topic.as_bytes());
        }
        send_frame!(self, &payload, FrameOp::ExcludeTopic, qos)
    }
    fn unexclude_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<SyncOpConfirm, Error> {
        let mut payload = Vec::new();
        for topic in topics {
            if !payload.is_empty() {
                payload.push(0x00);
            }
            payload.extend(topic.as_bytes());
        }
        send_frame!(self, &payload, FrameOp::UnexcludeTopic, qos)
    }
    #[inline]
    fn ping(&mut self) -> Result<(), Error> {
        send_data_or_mark_disconnected!(self, PING_FRAME);
        Ok(())
    }
    #[inline]
    fn is_connected(&self) -> bool {
        self.connected.load(atomic::Ordering::Relaxed)
    }
    #[inline]
    fn get_timeout(&self) -> Option<Duration> {
        Some(self.timeout)
    }
    #[inline]
    fn get_name(&self) -> &str {
        self.name.as_str()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.writer.shutdown();
    }
}

fn handle_read<R>(mut reader: R, tx: SyncEventSender, responses: ResponseMap) -> Result<(), Error>
where
    R: Read,
{
    loop {
        let mut buf = [0_u8; 6];
        reader.read_exact(&mut buf)?;
        let frame_type: FrameKind = buf[0].try_into()?;
        let realtime = buf[5] != 0;
        match frame_type {
            FrameKind::Nop => {}
            FrameKind::Acknowledge => {
                let ack_id = u32::from_le_bytes(buf[1..5].try_into().unwrap());
                let tx_channel = { responses.lock().remove(&ack_id) };
                if let Some(tx) = tx_channel {
                    let _r = tx.send(buf[5].to_busrt_result());
                } else {
                    warn!("orphaned busrt op ack {}", ack_id);
                }
            }
            _ => {
                let frame_len = u32::from_le_bytes(buf[1..5].try_into().unwrap());
                let mut buf = vec![0; frame_len as usize];
                reader.read_exact(&mut buf)?;
                let (sender, topic, payload_pos) = {
                    if frame_type == FrameKind::Publish {
                        let mut sp = buf.splitn(3, |c| *c == 0);
                        let s = sp.next().ok_or_else(|| Error::data("broken frame"))?;
                        let sender = std::str::from_utf8(s)?.to_owned();
                        let t = sp.next().ok_or_else(|| Error::data("broken frame"))?;
                        let topic = std::str::from_utf8(t)?.to_owned();
                        sp.next().ok_or_else(|| Error::data("broken frame"))?;
                        let payload_pos = s.len() + t.len() + 2;
                        (Some(sender), Some(topic), payload_pos)
                    } else {
                        let mut sp = buf.splitn(2, |c| *c == 0);
                        let s = sp.next().ok_or_else(|| Error::data("broken frame"))?;
                        let sender = std::str::from_utf8(s)?.to_owned();
                        sp.next().ok_or_else(|| Error::data("broken frame"))?;
                        let payload_pos = s.len() + 1;
                        (Some(sender), None, payload_pos)
                    }
                };
                let frame = Arc::new(FrameData::new(
                    frame_type,
                    sender,
                    topic,
                    None,
                    buf,
                    payload_pos,
                    realtime,
                ));
                tx.send(frame).map_err(Error::io)?;
            }
        }
    }
}

fn chat<S>(name: &str, socket: &mut S) -> Result<(), Error>
where
    S: Read + Write,
{
    if name.len() > u16::MAX as usize {
        return Err(Error::data("name too long"));
    }
    let mut buf = [0_u8; 3];
    socket.read_exact(&mut buf)?;
    if buf[0] != GREETINGS[0] {
        return Err(Error::not_supported("Invalid greetings"));
    }
    if u16::from_le_bytes(buf[1..3].try_into().unwrap()) != PROTOCOL_VERSION {
        return Err(Error::not_supported("Unsupported protocol version"));
    }
    socket.write_all(&buf)?;
    let mut buf = [0_u8; 1];
    socket.read_exact(&mut buf)?;
    if buf[0] != RESPONSE_OK {
        return Err(Error::new(
            buf[0].into(),
            Some(format!("Server greetings response: {:?}", buf[0])),
        ));
    }
    let n = name.as_bytes().to_vec();
    #[allow(clippy::cast_possible_truncation)]
    socket.write_all(&(name.len() as u16).to_le_bytes())?;
    socket.write_all(&n)?;
    let mut buf = [0_u8; 1];
    socket.read_exact(&mut buf)?;
    if buf[0] != RESPONSE_OK {
        return Err(Error::new(
            buf[0].into(),
            Some(format!("Server registration response: {:?}", buf[0])),
        ));
    }
    Ok(())
}

pub struct Reader {
    inner: Box<dyn Read + Send>,
    tx: SyncEventSender,
    responses: ResponseMap,
    rconn: Arc<atomic::AtomicBool>,
}

impl Reader {
    pub fn run(self) {
        if let Err(e) = handle_read(self.inner, self.tx, self.responses) {
            error!("busrt client reader error: {}", e);
            self.rconn.store(false, atomic::Ordering::Relaxed);
        }
    }
}
