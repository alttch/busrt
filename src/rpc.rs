use crate::borrow::Cow;
use crate::client::AsyncClient;
use crate::EventChannel;
use crate::{Error, Frame, FrameKind, OpConfirm, QoS};

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use log::{error, trace, warn};

use async_trait::async_trait;

pub const RPC_NOTIFICATION: u8 = 0x00;
pub const RPC_REQUEST: u8 = 0x01;
pub const RPC_REPLY: u8 = 0x11;
pub const RPC_ERROR: u8 = 0x12;

pub const RPC_NOTIFICATION_BULK: u8 = 0x40;
pub const RPC_REQUEST_BULK: u8 = 0x41;
pub const RPC_REPLY_BULK: u8 = 0x51;

pub const RPC_ERROR_CODE_PARSE: i16 = -32700;
pub const RPC_ERROR_CODE_INVALID_REQUEST: i16 = -32600;
pub const RPC_ERROR_CODE_METHOD_NOT_FOUND: i16 = -32601;
pub const RPC_ERROR_CODE_INVALID_METHOD_PARAMS: i16 = -32602;
pub const RPC_ERROR_CODE_INTERNAL: i16 = -32603;

/// By default, RPC frame and notification handlers are launched in background, which allows
/// non-blocking event processing, however events can be processed in random order
///
/// RPC options allow to launch handlers in blocking mode. In this case handlers must process
/// events as fast as possible (e.g. send them to processing channels) and avoid using any RPC
/// client functions from inside.
///
/// WARNING: when handling frames in blocking mode, it is forbidden to use the current RPC client
/// directly or with any kind of bounded channels, otherwise the RPC client may get stuck!
///
/// See https://elbus.readthedocs.io/en/latest/rpc_blocking.html
#[derive(Default, Clone, Debug)]
pub struct Options {
    blocking_notifications: bool,
    blocking_frames: bool,
}

impl Options {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
    #[inline]
    pub fn blocking_notifications(mut self) -> Self {
        self.blocking_notifications = true;
        self
    }
    #[inline]
    pub fn blocking_frames(mut self) -> Self {
        self.blocking_frames = true;
        self
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum RpcEventKind {
    Notification = RPC_NOTIFICATION,
    Request = RPC_REQUEST,
    Reply = RPC_REPLY,
    ErrorReply = RPC_ERROR,
}

#[allow(clippy::module_name_repetitions)]
#[inline]
pub fn rpc_err_str(v: impl fmt::Display) -> Option<Vec<u8>> {
    Some(v.to_string().as_bytes().to_vec())
}

impl fmt::Display for RpcEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                RpcEventKind::Notification => "notifcation",
                RpcEventKind::Request => "request",
                RpcEventKind::Reply => "reply",
                RpcEventKind::ErrorReply => "error reply",
            }
        )
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct RpcEvent {
    kind: RpcEventKind,
    frame: Frame,
    payload_pos: usize,
    payload_end_pos: Option<usize>,
    subframe_pos: usize,
    use_header: bool,
}

impl RpcEvent {
    #[inline]
    pub fn kind(&self) -> RpcEventKind {
        self.kind
    }
    #[inline]
    pub fn frame(&self) -> &Frame {
        &self.frame
    }
    #[inline]
    pub fn sender(&self) -> &str {
        self.frame.sender()
    }
    #[inline]
    pub fn primary_sender(&self) -> &str {
        self.frame.primary_sender()
    }
    #[inline]
    pub fn payload(&self) -> &[u8] {
        if let Some(payload_end_pos) = self.payload_end_pos {
            &self.frame().payload()[self.payload_pos..payload_end_pos]
        } else {
            &self.frame().payload()[self.payload_pos..]
        }
    }
    /// # Panics
    ///
    /// Should not panic
    #[inline]
    pub fn id(&self) -> u32 {
        u32::from_le_bytes(
            if self.use_header {
                &self.frame.header().unwrap()[1..5]
            } else {
                &self.frame.payload()[1..5]
            }
            .try_into()
            .unwrap(),
        )
    }
    #[inline]
    pub fn is_response_required(&self) -> bool {
        self.id() != 0
    }
    /// # Panics
    ///
    /// Should not panic
    #[inline]
    pub fn method(&self) -> &[u8] {
        if self.use_header {
            let header = self.frame.header.as_ref().unwrap();
            &header[self.subframe_pos + 5..header.len() - 1]
        } else {
            &self.frame().payload()[self.subframe_pos + 5..self.payload_pos - 1]
        }
    }
    #[inline]
    pub fn parse_method(&self) -> Result<&str, Error> {
        std::str::from_utf8(self.method()).map_err(Into::into)
    }
    /// # Panics
    ///
    /// Should not panic
    #[inline]
    pub fn code(&self) -> i16 {
        if self.kind == RpcEventKind::ErrorReply {
            i16::from_le_bytes(
                if self.use_header {
                    &self.frame.header().unwrap()[self.subframe_pos + 5..self.subframe_pos + 7]
                } else {
                    &self.frame.payload()[self.subframe_pos + 5..self.subframe_pos + 7]
                }
                .try_into()
                .unwrap(),
            )
        } else {
            0
        }
    }
}

impl TryFrom<Frame> for RpcEvent {
    type Error = Error;
    fn try_from(frame: Frame) -> Result<Self, Self::Error> {
        let (body, use_header) = frame
            .header()
            .map_or_else(|| (frame.payload(), false), |h| (h, true));
        if body.is_empty() {
            Err(Error::data("Empty RPC frame"))
        } else {
            macro_rules! check_len {
                ($len: expr) => {
                    if body.len() < $len {
                        return Err(Error::data("Invalid RPC frame"));
                    }
                };
            }
            match body[0] {
                RPC_NOTIFICATION => Ok(RpcEvent {
                    kind: RpcEventKind::Notification,
                    frame,
                    payload_pos: if use_header { 0 } else { 1 },
                    payload_end_pos: None,
                    subframe_pos: 0,
                    use_header: false,
                }),
                RPC_REQUEST => {
                    check_len!(6);
                    if use_header {
                        Ok(RpcEvent {
                            kind: RpcEventKind::Request,
                            frame,
                            payload_pos: 0,
                            payload_end_pos: None,
                            subframe_pos: 0,
                            use_header: true,
                        })
                    } else {
                        let mut sp = body[5..].splitn(2, |c| *c == 0);
                        let method = sp.next().ok_or_else(|| Error::data("No RPC method"))?;
                        let payload_pos = 6 + method.len();
                        sp.next()
                            .ok_or_else(|| Error::data("No RPC params block"))?;
                        Ok(RpcEvent {
                            kind: RpcEventKind::Request,
                            frame,
                            payload_pos,
                            payload_end_pos: None,
                            subframe_pos: 0,
                            use_header: false,
                        })
                    }
                }
                RPC_REPLY => {
                    check_len!(5);
                    Ok(RpcEvent {
                        kind: RpcEventKind::Reply,
                        frame,
                        payload_pos: if use_header { 0 } else { 5 },
                        payload_end_pos: None,
                        subframe_pos: 0,
                        use_header,
                    })
                }
                RPC_ERROR => {
                    check_len!(7);
                    Ok(RpcEvent {
                        kind: RpcEventKind::ErrorReply,
                        frame,
                        payload_pos: if use_header { 0 } else { 7 },
                        payload_end_pos: None,
                        subframe_pos: 0,
                        use_header,
                    })
                }
                v => Err(Error::data(format!("Unsupported RPC frame code {}", v))),
            }
        }
    }
}

#[allow(clippy::module_name_repetitions)]
#[async_trait]
pub trait RpcHandlers {
    async fn handle_call(&self, event: RpcEvent) -> RpcResult;
    async fn handle_notification(&self, event: RpcEvent);
    async fn handle_frame(&self, frame: Frame);
}

pub struct DummyHandlers {}

#[async_trait]
impl RpcHandlers for DummyHandlers {
    async fn handle_call(&self, _event: RpcEvent) -> RpcResult {
        Err(RpcError::new(
            RPC_ERROR_CODE_METHOD_NOT_FOUND,
            Some("RPC handler is not implemented".as_bytes().to_vec()),
        ))
    }
    async fn handle_notification(&self, _event: RpcEvent) {}
    async fn handle_frame(&self, _frame: Frame) {}
}

type CallMap = Arc<parking_lot::Mutex<BTreeMap<u32, oneshot::Sender<RpcEvent>>>>;

#[async_trait]
pub trait Rpc {
    /// When created, elbus client is wrapped with Arc<Mutex<_>> to let it be sent into
    /// the incoming frames handler future
    ///
    /// This mehtod allows to get the containered-client back, to call its methods directly (manage
    /// pub/sub and send broadcast messages)
    fn client(&self) -> Arc<Mutex<(dyn AsyncClient + 'static)>>;
    async fn notify(
        &self,
        target: &str,
        data: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error>;
    async fn notify_bulk(
        &self,
        target: &str,
        data: &[Cow<'async_trait>],
        qos: QoS,
    ) -> Result<OpConfirm, Error>;
    /// Call the method, no response is required
    async fn call0(
        &self,
        target: &str,
        method: &str,
        params: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error>;
    /// Call the method and get the response
    async fn call(
        &self,
        target: &str,
        method: &str,
        params: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<RpcEvent, RpcError>;
    fn is_connected(&self) -> bool;
}

#[allow(clippy::module_name_repetitions)]
pub struct RpcClient {
    call_id: parking_lot::Mutex<u32>,
    timeout: Option<Duration>,
    client: Arc<Mutex<dyn AsyncClient>>,
    processor_fut: Arc<parking_lot::Mutex<JoinHandle<()>>>,
    pinger_fut: Option<JoinHandle<()>>,
    calls: CallMap,
    connected: Option<Arc<atomic::AtomicBool>>,
}

#[allow(clippy::too_many_lines)]
async fn processor<C, H>(
    rx: EventChannel,
    processor_client: Arc<Mutex<C>>,
    calls: CallMap,
    handlers: Arc<H>,
    opts: Options,
) where
    C: AsyncClient + 'static,
    H: RpcHandlers + Send + Sync + 'static,
{
    while let Ok(frame) = rx.recv().await {
        if frame.kind() == FrameKind::Message {
            let (body, use_header) = frame
                .header()
                .map_or_else(|| (frame.payload(), false), |v| (v, true));
            if body.is_empty() {
                error!("empty RPC frame");
                continue;
            }
            match body[0] {
                RPC_NOTIFICATION_BULK => {
                    // TODO check offsets
                    let payload = frame.payload();
                    let mut pos = if use_header { 0 } else { 1 };
                    let mut events = Vec::new();
                    while pos < payload.len() {
                        let size =
                            u32::from_le_bytes(payload[pos..pos + 4].try_into().unwrap()) as usize;
                        let event = RpcEvent {
                            kind: RpcEventKind::Notification,
                            frame: frame.clone(),
                            payload_pos: pos + 4,
                            payload_end_pos: Some(pos + 4 + size),
                            subframe_pos: pos + 4,
                            use_header: false,
                        };
                        events.push(event);
                        pos += 4 + size;
                    }
                    for event in events {
                        trace!("RPC bulk notification from {}", event.frame().sender());
                        if opts.blocking_notifications {
                            handlers.handle_notification(event).await;
                        } else {
                            let h = handlers.clone();
                            tokio::spawn(async move {
                                h.handle_notification(event).await;
                            });
                        }
                    }
                }
                _ => match RpcEvent::try_from(frame) {
                    Ok(event) => match event.kind() {
                        RpcEventKind::Notification => {
                            trace!("RPC notification from {}", event.frame().sender());
                            if opts.blocking_notifications {
                                handlers.handle_notification(event).await;
                            } else {
                                let h = handlers.clone();
                                tokio::spawn(async move {
                                    h.handle_notification(event).await;
                                });
                            }
                        }
                        RpcEventKind::Request => {
                            let id = event.id();
                            trace!(
                                "RPC request from {}, id: {}, method: {:?}",
                                event.frame().sender(),
                                id,
                                event.method()
                            );
                            let ev = if id > 0 {
                                Some((event.frame().sender().to_owned(), processor_client.clone()))
                            } else {
                                None
                            };
                            let h = handlers.clone();
                            tokio::spawn(async move {
                                let qos = if event.frame().is_realtime() {
                                    QoS::RealtimeProcessed
                                } else {
                                    QoS::Processed
                                };
                                let res = h.handle_call(event).await;
                                if let Some((target, cl)) = ev {
                                    macro_rules! send_reply {
                                        ($payload: expr, $result: expr) => {{
                                            let mut client = cl.lock().await;
                                            if let Some(result) = $result {
                                                client
                                                    .zc_send(&target, $payload, result.into(), qos)
                                                    .await
                                            } else {
                                                client
                                                    .zc_send(
                                                        &target,
                                                        $payload,
                                                        (&[][..]).into(),
                                                        qos,
                                                    )
                                                    .await
                                            }
                                        }};
                                    }
                                    match res {
                                        Ok(v) => {
                                            trace!("Sending RPC reply id {} to {}", id, target);
                                            let mut payload = Vec::with_capacity(5);
                                            payload.push(RPC_REPLY);
                                            payload.extend_from_slice(&id.to_le_bytes());
                                            let _r = send_reply!(payload.into(), v);
                                        }
                                        Err(e) => {
                                            trace!(
                                                "Sending RPC error {} reply id {} to {}",
                                                e.code,
                                                id,
                                                target,
                                            );
                                            let mut payload = Vec::with_capacity(7);
                                            payload.push(RPC_ERROR);
                                            payload.extend_from_slice(&id.to_le_bytes());
                                            payload.extend_from_slice(&e.code.to_le_bytes());
                                            let _r = send_reply!(payload.into(), e.data);
                                        }
                                    }
                                }
                            });
                        }
                        RpcEventKind::Reply | RpcEventKind::ErrorReply => {
                            let id = event.id();
                            trace!(
                                "RPC {} from {}, id: {}",
                                event.kind(),
                                event.frame().sender(),
                                id
                            );
                            if let Some(tx) = { calls.lock().remove(&id) } {
                                let _r = tx.send(event);
                            } else {
                                warn!("orphaned RPC response: {}", id);
                            }
                        }
                    },
                    Err(e) => {
                        error!("{}", e);
                    }
                },
            }
        } else if opts.blocking_frames {
            handlers.handle_frame(frame).await;
        } else {
            let h = handlers.clone();
            tokio::spawn(async move {
                h.handle_frame(frame).await;
            });
        }
    }
}

#[inline]
fn prepare_call_payload(method: &str, id_bytes: &[u8]) -> Vec<u8> {
    let m = method.as_bytes();
    let mut payload = Vec::with_capacity(m.len() + 6);
    payload.push(RPC_REQUEST);
    payload.extend(id_bytes);
    payload.extend(m);
    payload.push(0x00);
    payload
}

impl RpcClient {
    /// creates RPC client with the specified handlers and the default options
    pub fn new<H>(client: impl AsyncClient + 'static, handlers: H) -> Self
    where
        H: RpcHandlers + Send + Sync + 'static,
    {
        Self::init(client, handlers, Options::default())
    }

    /// creates RPC client with dummy handlers and the default options
    pub fn new0(client: impl AsyncClient + 'static) -> Self {
        Self::init(client, DummyHandlers {}, Options::default())
    }

    /// creates RPC client
    pub fn create<H>(client: impl AsyncClient + 'static, handlers: H, opts: Options) -> Self
    where
        H: RpcHandlers + Send + Sync + 'static,
    {
        Self::init(client, handlers, opts)
    }

    /// creates RPC client with dummy handlers
    pub fn create0(client: impl AsyncClient + 'static, opts: Options) -> Self {
        Self::init(client, DummyHandlers {}, opts)
    }

    fn init<H>(mut client: impl AsyncClient + 'static, handlers: H, opts: Options) -> Self
    where
        H: RpcHandlers + Send + Sync + 'static,
    {
        let timeout = client.get_timeout();
        let rx = { client.take_event_channel().unwrap() };
        let connected = client.get_connected_beacon();
        let client = Arc::new(Mutex::new(client));
        let calls: CallMap = <_>::default();
        let processor_fut = Arc::new(parking_lot::Mutex::new(tokio::spawn(processor(
            rx,
            client.clone(),
            calls.clone(),
            Arc::new(handlers),
            opts,
        ))));
        let pinger_client = client.clone();
        let pfut = processor_fut.clone();
        let pinger_fut = timeout.map(|t| {
            tokio::spawn(async move {
                loop {
                    if let Err(e) = pinger_client.lock().await.ping().await {
                        error!("{}", e);
                        pfut.lock().abort();
                        break;
                    }
                    tokio::time::sleep(t).await;
                }
            })
        });
        Self {
            call_id: parking_lot::Mutex::new(0),
            timeout,
            client,
            processor_fut,
            pinger_fut,
            calls,
            connected,
        }
    }
}

#[async_trait]
impl Rpc for RpcClient {
    #[inline]
    fn client(&self) -> Arc<Mutex<(dyn AsyncClient + 'static)>> {
        self.client.clone()
    }
    #[inline]
    async fn notify(
        &self,
        target: &str,
        data: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error> {
        self.client
            .lock()
            .await
            .zc_send(target, (&[RPC_NOTIFICATION][..]).into(), data, qos)
            .await
    }
    #[inline]
    async fn notify_bulk(
        &self,
        target: &str,
        data: &[Cow<'async_trait>],
        qos: QoS,
    ) -> Result<OpConfirm, Error> {
        let len = data.iter().map(Cow::len).sum::<usize>();
        let mut buf = Vec::with_capacity(len + data.len() * 4);
        for d in data {
            buf.extend((d.len() as u32).to_le_bytes());
            buf.extend(d.as_slice());
        }
        // TODO implement client mentod to send blocks one-by-one with len pfx
        self.client
            .lock()
            .await
            .zc_send(
                target,
                (&[RPC_NOTIFICATION_BULK][..]).into(),
                buf.into(),
                qos,
            )
            .await
    }
    async fn call0(
        &self,
        target: &str,
        method: &str,
        params: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error> {
        let payload = prepare_call_payload(method, &[0, 0, 0, 0]);
        self.client
            .lock()
            .await
            .zc_send(target, payload.into(), params, qos)
            .await
    }
    /// # Panics
    ///
    /// Will panic on poisoned mutex
    async fn call(
        &self,
        target: &str,
        method: &str,
        params: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<RpcEvent, RpcError> {
        let call_id = {
            let mut ci = self.call_id.lock();
            let mut call_id = *ci;
            if call_id == u32::MAX {
                call_id = 1;
            } else {
                call_id += 1;
            }
            *ci = call_id;
            call_id
        };
        let payload = prepare_call_payload(method, &call_id.to_le_bytes());
        let (tx, rx) = oneshot::channel();
        self.calls.lock().insert(call_id, tx);
        macro_rules! unwrap_or_cancel {
            ($result: expr) => {
                match $result {
                    Ok(v) => v,
                    Err(e) => {
                        self.calls.lock().remove(&call_id);
                        return Err(Into::<Error>::into(e).into());
                    }
                }
            };
        }
        let opc = {
            let mut client = self.client.lock().await;
            let fut = client.zc_send(target, payload.into(), params, qos);
            if let Some(timeout) = self.timeout {
                unwrap_or_cancel!(unwrap_or_cancel!(tokio::time::timeout(timeout, fut).await))
            } else {
                unwrap_or_cancel!(fut.await)
            }
        };
        if let Some(c) = opc {
            unwrap_or_cancel!(unwrap_or_cancel!(c.await));
        }
        let result = rx.await.map_err(Into::<Error>::into)?;
        if let Ok(e) = RpcError::try_from(&result) {
            Err(e)
        } else {
            Ok(result)
        }
    }
    fn is_connected(&self) -> bool {
        self.connected
            .as_ref()
            .map_or(true, |b| b.load(atomic::Ordering::SeqCst))
    }
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        self.pinger_fut.as_ref().map(JoinHandle::abort);
        self.processor_fut.lock().abort();
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct RpcError {
    code: i16,
    data: Option<Vec<u8>>,
}

impl TryFrom<&RpcEvent> for RpcError {
    type Error = Error;
    #[inline]
    fn try_from(event: &RpcEvent) -> Result<Self, Self::Error> {
        if event.kind() == RpcEventKind::ErrorReply {
            Ok(RpcError::new(event.code(), Some(event.payload().to_vec())))
        } else {
            Err(Error::data("not a RPC error"))
        }
    }
}

impl RpcError {
    #[inline]
    pub fn new(code: i16, data: Option<Vec<u8>>) -> Self {
        Self { code, data }
    }
    #[inline]
    pub fn code(&self) -> i16 {
        self.code
    }
    #[inline]
    pub fn data(&self) -> Option<&[u8]> {
        self.data.as_deref()
    }
    #[inline]
    pub fn method(err: Option<Vec<u8>>) -> Self {
        Self {
            code: RPC_ERROR_CODE_METHOD_NOT_FOUND,
            data: err,
        }
    }
    #[inline]
    pub fn params(err: Option<Vec<u8>>) -> Self {
        Self {
            code: RPC_ERROR_CODE_INVALID_METHOD_PARAMS,
            data: err,
        }
    }
    #[inline]
    pub fn parse(err: Option<Vec<u8>>) -> Self {
        Self {
            code: RPC_ERROR_CODE_PARSE,
            data: err,
        }
    }
    #[inline]
    pub fn invalid(err: Option<Vec<u8>>) -> Self {
        Self {
            code: RPC_ERROR_CODE_INVALID_REQUEST,
            data: err,
        }
    }
    #[inline]
    pub fn internal(err: Option<Vec<u8>>) -> Self {
        Self {
            code: RPC_ERROR_CODE_INTERNAL,
            data: err,
        }
    }
    /// Converts displayable to Vec<u8>
    #[inline]
    pub fn convert_data(v: impl fmt::Display) -> Vec<u8> {
        v.to_string().as_bytes().to_vec()
    }
}

impl From<Error> for RpcError {
    #[inline]
    fn from(e: Error) -> RpcError {
        RpcError {
            code: -32000 - e.kind() as i16,
            data: None,
        }
    }
}

impl From<rmp_serde::encode::Error> for RpcError {
    #[inline]
    fn from(e: rmp_serde::encode::Error) -> RpcError {
        RpcError {
            code: RPC_ERROR_CODE_INTERNAL,
            data: Some(e.to_string().as_bytes().to_vec()),
        }
    }
}

impl From<std::io::Error> for RpcError {
    #[inline]
    fn from(e: std::io::Error) -> RpcError {
        RpcError {
            code: RPC_ERROR_CODE_INTERNAL,
            data: Some(e.to_string().as_bytes().to_vec()),
        }
    }
}

impl From<rmp_serde::decode::Error> for RpcError {
    #[inline]
    fn from(e: rmp_serde::decode::Error) -> RpcError {
        RpcError {
            code: RPC_ERROR_CODE_PARSE,
            data: Some(e.to_string().as_bytes().to_vec()),
        }
    }
}

impl fmt::Display for RpcError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rpc error code: {}", self.code)
    }
}

#[allow(clippy::module_name_repetitions)]
pub type RpcResult = Result<Option<Vec<u8>>, RpcError>;
