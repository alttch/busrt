use crate::borrow::Cow;
use crate::comm::{Flush, TtlBufWriter};
use crate::Error;
use crate::EventChannel;
use crate::IntoBusRtResult;
use crate::OpConfirm;
use crate::QoS;
use crate::GREETINGS;
use crate::PING_FRAME;
use crate::PROTOCOL_VERSION;
use crate::RESPONSE_OK;
use crate::SECONDARY_SEP;
use crate::{Frame, FrameData, FrameKind, FrameOp};
use async_tungstenite::tokio::TokioAdapter;
use futures_util::AsyncReadExt as _;
#[cfg(not(feature = "rt"))]
use parking_lot::Mutex;
#[cfg(feature = "rt")]
use parking_lot_rt::Mutex;
use std::collections::BTreeMap;
use std::marker::Unpin;
use std::sync::atomic;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
#[cfg(not(target_os = "windows"))]
use tokio::net::unix;
#[cfg(not(target_os = "windows"))]
use tokio::net::UnixStream;
use tokio::net::{tcp, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::compat::FuturesAsyncReadCompatExt as _;
use tokio_util::compat::FuturesAsyncWriteCompatExt as _;
use ws_stream_tungstenite::WsStream;

use crate::client::AsyncClient;

use log::{error, trace, warn};

use async_trait::async_trait;

type ResponseMap = Arc<Mutex<BTreeMap<u32, oneshot::Sender<Result<(), Error>>>>>;

enum Writer {
    #[cfg(not(target_os = "windows"))]
    Unix(TtlBufWriter<unix::OwnedWriteHalf>),
    Tcp(TtlBufWriter<tcp::OwnedWriteHalf>),
    WebSocket(
        TtlBufWriter<
            tokio_util::compat::Compat<
                futures_util::io::WriteHalf<
                    WsStream<
                        async_tungstenite::stream::Stream<
                            TokioAdapter<TcpStream>,
                            TokioAdapter<tokio_rustls::client::TlsStream<TcpStream>>,
                        >,
                    >,
                >,
            >,
        >,
    ),
}

impl Writer {
    pub async fn write(&mut self, buf: &[u8], flush: Flush) -> Result<(), Error> {
        match self {
            #[cfg(not(target_os = "windows"))]
            Writer::Unix(w) => w.write(buf, flush).await.map_err(Into::into),
            Writer::Tcp(w) => w.write(buf, flush).await.map_err(Into::into),
            Writer::WebSocket(w) => w.write(buf, flush).await.map_err(Into::into),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    path: String,
    name: String,
    buf_size: usize,
    buf_ttl: Duration,
    queue_size: usize,
    timeout: Duration,
    token: Option<String>,
}

impl Config {
    /// path - /path/to/socket (must end with .sock .socket or .ipc) or host:port,
    /// name - an unique client name
    pub fn new(path: &str, name: &str) -> Self {
        Self {
            path: path.to_owned(),
            name: name.to_owned(),
            buf_size: crate::DEFAULT_BUF_SIZE,
            buf_ttl: crate::DEFAULT_BUF_TTL,
            queue_size: crate::DEFAULT_QUEUE_SIZE,
            timeout: crate::DEFAULT_TIMEOUT,
            token: None,
        }
    }
    pub fn buf_size(mut self, size: usize) -> Self {
        self.buf_size = size;
        self
    }
    pub fn buf_ttl(mut self, ttl: Duration) -> Self {
        self.buf_ttl = ttl;
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
    /// Bearer token for authorization header if required
    pub fn token<S: AsRef<str>>(mut self, token: S) -> Self {
        self.token = Some(token.as_ref().to_owned());
        self
    }
}

pub struct Client {
    name: String,
    writer: Writer,
    reader_fut: JoinHandle<()>,
    frame_id: u32,
    responses: ResponseMap,
    rx: Option<EventChannel>,
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
    ($self: expr, $data: expr, $flush: expr) => {
        match tokio::time::timeout($self.timeout, $self.writer.write($data, $flush)).await {
            Ok(result) => {
                if let Err(e) = result {
                    $self.reader_fut.abort();
                    $self.connected.store(false, atomic::Ordering::Relaxed);
                    return Err(e.into());
                }
            }
            Err(e) => {
                return Err(e.into());
            }
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
        send_data_or_mark_disconnected!($self, $buf, Flush::No);
        send_data_or_mark_disconnected!($self, $payload, $qos.is_realtime().into());
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
    ($name: expr, $reader: expr, $writer: expr,
         $responses: expr, $connected: expr, $timeout: expr, $queue_size: expr) => {{
        chat($name, &mut $reader, &mut $writer).await?;
        let (tx, rx) = async_channel::bounded($queue_size);
        let reader_responses = $responses.clone();
        let rconn = $connected.clone();
        let timeout = $timeout.clone();
        let reader_fut = tokio::spawn(async move {
            if let Err(e) = handle_read($reader, tx, timeout, reader_responses).await {
                error!("busrt client reader error: {}", e);
            }
            rconn.store(false, atomic::Ordering::Relaxed);
        });
        (reader_fut, rx)
    }};
}

impl Client {
    pub async fn connect(config: &Config) -> Result<Self, Error> {
        tokio::time::timeout(config.timeout, Self::connect_broker(config, None)).await?
    }
    pub async fn connect_stream(stream: UnixStream, config: &Config) -> Result<Self, Error> {
        tokio::time::timeout(config.timeout, Self::connect_broker(config, Some(stream))).await?
    }
    async fn connect_broker(config: &Config, stream: Option<UnixStream>) -> Result<Self, Error> {
        let responses: ResponseMap = <_>::default();
        let connected = Arc::new(atomic::AtomicBool::new(true));
        #[allow(clippy::case_sensitive_file_extension_comparisons)]
        let (writer, reader_fut, rx) =
            if config.path.starts_with("ws://") || config.path.starts_with("wss://") {
                let mut ws_config = tungstenite::protocol::WebSocketConfig::default();
                ws_config.read_buffer_size = config.buf_size;
                ws_config.write_buffer_size = config.buf_size;
                ws_config.max_write_buffer_size = config.buf_size * 2;
                ws_config.max_message_size = Some(usize::try_from(u32::MAX).unwrap());
                ws_config.max_frame_size = Some(usize::try_from(u32::MAX).unwrap());

                let mut req = tungstenite::client::ClientRequestBuilder::new(
                    config
                        .path
                        .parse::<tungstenite::http::Uri>()
                        .map_err(Error::io)?,
                );
                if let Some(token) = &config.token {
                    req = req.with_header("Authorization", format!("Bearer {}", token))
                }

                let (ws_client, _) =
                    async_tungstenite::tokio::connect_async_with_config(req, Some(ws_config))
                        .await
                        .map_err(Error::io)?;
                let ws = WsStream::new(ws_client);
                let (r, w) = ws.split();
                let mut r = r.compat();
                let mut w = w.compat_write();
                let (reader_fut, rx) = connect_broker!(
                    &config.name,
                    r,
                    w,
                    responses,
                    connected,
                    config.timeout,
                    config.queue_size
                );
                (
                    Writer::WebSocket(TtlBufWriter::new(
                        w,
                        config.buf_size,
                        config.buf_ttl,
                        config.timeout,
                    )),
                    reader_fut,
                    rx,
                )
            } else if config.path.ends_with(".sock")
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
                    let stream = if let Some(s) = stream {
                        s
                    } else {
                        UnixStream::connect(&config.path).await?
                    };
                    let (r, mut writer) = stream.into_split();
                    let mut reader = BufReader::with_capacity(config.buf_size, r);
                    let (reader_fut, rx) = connect_broker!(
                        &config.name,
                        reader,
                        writer,
                        responses,
                        connected,
                        config.timeout,
                        config.queue_size
                    );
                    (
                        Writer::Unix(TtlBufWriter::new(
                            writer,
                            config.buf_size,
                            config.buf_ttl,
                            config.timeout,
                        )),
                        reader_fut,
                        rx,
                    )
                }
            } else {
                let stream = TcpStream::connect(&config.path).await?;
                stream.set_nodelay(true)?;
                let (r, mut writer) = stream.into_split();
                let mut reader = BufReader::with_capacity(config.buf_size, r);
                let (reader_fut, rx) = connect_broker!(
                    &config.name,
                    reader,
                    writer,
                    responses,
                    connected,
                    config.timeout,
                    config.queue_size
                );
                (
                    Writer::Tcp(TtlBufWriter::new(
                        writer,
                        config.buf_size,
                        config.buf_ttl,
                        config.timeout,
                    )),
                    reader_fut,
                    rx,
                )
            };
        Ok(Self {
            name: config.name.clone(),
            writer,
            reader_fut,
            frame_id: 0,
            responses,
            rx: Some(rx),
            connected,
            timeout: config.timeout,
            config: config.clone(),
            secondary_counter: atomic::AtomicUsize::new(0),
        })
    }
    pub async fn register_secondary(&self) -> Result<Self, Error> {
        if self.name.contains(SECONDARY_SEP) {
            Err(Error::not_supported("not a primary client"))
        } else {
            let secondary_id = self
                .secondary_counter
                .fetch_add(1, atomic::Ordering::Relaxed);
            let secondary_name = format!("{}{}{}", self.name, SECONDARY_SEP, secondary_id);
            let mut config = self.config.clone();
            config.name = secondary_name;
            Self::connect(&config).await
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
#[async_trait]
impl AsyncClient for Client {
    #[inline]
    fn take_event_channel(&mut self) -> Option<EventChannel> {
        self.rx.take()
    }
    #[inline]
    fn get_connected_beacon(&self) -> Option<Arc<atomic::AtomicBool>> {
        Some(self.connected.clone())
    }
    async fn send(
        &mut self,
        target: &str,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error> {
        send_frame!(self, target, payload.as_slice(), FrameOp::Message, qos)
    }
    async fn zc_send(
        &mut self,
        target: &str,
        header: Cow<'async_trait>,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error> {
        send_zc_frame!(
            self,
            target,
            header.as_slice(),
            payload.as_slice(),
            FrameOp::Message,
            qos
        )
    }
    async fn send_broadcast(
        &mut self,
        target: &str,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error> {
        send_frame!(self, target, payload.as_slice(), FrameOp::Broadcast, qos)
    }
    async fn publish(
        &mut self,
        target: &str,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error> {
        send_frame!(self, target, payload.as_slice(), FrameOp::PublishTopic, qos)
    }
    async fn publish_for(
        &mut self,
        target: &str,
        receiver: &str,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error> {
        send_frame!(
            self,
            target,
            receiver,
            payload.as_slice(),
            FrameOp::PublishTopicFor,
            qos
        )
    }
    async fn subscribe(&mut self, topic: &str, qos: QoS) -> Result<OpConfirm, Error> {
        send_frame!(self, topic.as_bytes(), FrameOp::SubscribeTopic, qos)
    }
    async fn unsubscribe(&mut self, topic: &str, qos: QoS) -> Result<OpConfirm, Error> {
        send_frame!(self, topic.as_bytes(), FrameOp::UnsubscribeTopic, qos)
    }
    async fn subscribe_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<OpConfirm, Error> {
        let mut payload = Vec::new();
        for topic in topics {
            if !payload.is_empty() {
                payload.push(0x00);
            }
            payload.extend(topic.as_bytes());
        }
        send_frame!(self, &payload, FrameOp::SubscribeTopic, qos)
    }
    async fn unsubscribe_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<OpConfirm, Error> {
        let mut payload = Vec::new();
        for topic in topics {
            if !payload.is_empty() {
                payload.push(0x00);
            }
            payload.extend(topic.as_bytes());
        }
        send_frame!(self, &payload, FrameOp::UnsubscribeTopic, qos)
    }
    async fn exclude(&mut self, topic: &str, qos: QoS) -> Result<OpConfirm, Error> {
        send_frame!(self, topic.as_bytes(), FrameOp::ExcludeTopic, qos)
    }
    async fn unexclude(&mut self, topic: &str, qos: QoS) -> Result<OpConfirm, Error> {
        send_frame!(self, topic.as_bytes(), FrameOp::UnexcludeTopic, qos)
    }
    async fn exclude_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<OpConfirm, Error> {
        let mut payload = Vec::new();
        for topic in topics {
            if !payload.is_empty() {
                payload.push(0x00);
            }
            payload.extend(topic.as_bytes());
        }
        send_frame!(self, &payload, FrameOp::ExcludeTopic, qos)
    }
    async fn unexclude_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<OpConfirm, Error> {
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
    async fn ping(&mut self) -> Result<(), Error> {
        send_data_or_mark_disconnected!(self, PING_FRAME, Flush::Instant);
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
        self.reader_fut.abort();
    }
}

async fn handle_read<R>(
    mut reader: R,
    tx: async_channel::Sender<Frame>,
    timeout: Duration,
    responses: ResponseMap,
) -> Result<(), Error>
where
    R: AsyncReadExt + Unpin,
{
    loop {
        let mut buf = [0_u8; 6];
        reader.read_exact(&mut buf).await?;
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
                tokio::time::timeout(timeout, reader.read_exact(&mut buf)).await??;
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
                tx.send(frame).await.map_err(Error::io)?;
            }
        }
    }
}

async fn chat<R, W>(name: &str, reader: &mut R, writer: &mut W) -> Result<(), Error>
where
    R: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    if name.len() > u16::MAX as usize {
        return Err(Error::data("name too long"));
    }
    let mut buf = [0_u8; 3];
    reader.read_exact(&mut buf).await?;
    if buf[0] != GREETINGS[0] {
        return Err(Error::not_supported("Invalid greetings"));
    }
    if u16::from_le_bytes(buf[1..3].try_into().unwrap()) != PROTOCOL_VERSION {
        return Err(Error::not_supported("Unsupported protocol version"));
    }
    writer.write_all(&buf).await?;
    let mut buf = [0_u8; 1];
    reader.read_exact(&mut buf).await?;
    if buf[0] != RESPONSE_OK {
        return Err(Error::new(
            buf[0].into(),
            Some(format!("Server greetings response: {:?}", buf[0])),
        ));
    }
    let n = name.as_bytes().to_vec();
    #[allow(clippy::cast_possible_truncation)]
    writer.write_all(&(name.len() as u16).to_le_bytes()).await?;
    writer.write_all(&n).await?;
    let mut buf = [0_u8; 1];
    reader.read_exact(&mut buf).await?;
    if buf[0] != RESPONSE_OK {
        return Err(Error::new(
            buf[0].into(),
            Some(format!("Server registration response: {:?}", buf[0])),
        ));
    }
    Ok(())
}
