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

pub const RPC_ERROR_CODE_PARSE: i16 = -32700;
pub const RPC_ERROR_CODE_INVALID_REQUEST: i16 = -32600;
pub const RPC_ERROR_CODE_METHOD_NOT_FOUND: i16 = -32601;
pub const RPC_ERROR_CODE_INVALID_METHOD_PARAMS: i16 = -32602;
pub const RPC_ERROR_CODE_INTERNAL: i16 = -32603;

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum RpcEventKind {
    Notification = RPC_NOTIFICATION,
    Request = RPC_REQUEST,
    Reply = RPC_REPLY,
    ErrorReply = RPC_ERROR,
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
    pub fn payload(&self) -> &[u8] {
        &self.frame().payload()[self.payload_pos..]
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
    /// # Panics
    ///
    /// Should not panic
    #[inline]
    pub fn method(&self) -> &[u8] {
        if self.use_header {
            &self.frame().header().unwrap()[5..]
        } else {
            &self.frame().payload()[5..self.payload_pos - 1]
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
                    &self.frame.header().unwrap()[5..7]
                } else {
                    &self.frame.payload()[5..7]
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
                    use_header: false,
                }),
                RPC_REQUEST => {
                    check_len!(6);
                    if use_header {
                        Ok(RpcEvent {
                            kind: RpcEventKind::Request,
                            frame,
                            payload_pos: 0,
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
                        use_header,
                    })
                }
                RPC_ERROR => {
                    check_len!(7);
                    Ok(RpcEvent {
                        kind: RpcEventKind::ErrorReply,
                        frame,
                        payload_pos: if use_header { 0 } else { 7 },
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

type CallMap = Arc<std::sync::Mutex<BTreeMap<u32, oneshot::Sender<RpcEvent>>>>;

#[async_trait]
pub trait Rpc {
    /// When created, elbus client is wrapped with Arc<Mutex<_>> to let it be sent into
    /// the incoming frames handler future
    ///
    /// This mehtod allows to get the containered-client back, to call its methods directly (manage
    /// pub/sub and send broadcast messages)
    fn client(&self) -> Arc<Mutex<(dyn AsyncClient + 'static)>>;
    async fn notify(
        &mut self,
        target: &str,
        data: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error>;
    /// Call the method, no response is required
    async fn call0(
        &self,
        target: &str,
        method: &str,
        params: Cow<'async_trait>,
    ) -> Result<OpConfirm, Error>;
    /// Call the method and get the response
    async fn call(
        &mut self,
        target: &str,
        method: &str,
        params: Cow<'async_trait>,
    ) -> Result<RpcEvent, RpcError>;
    fn is_connected(&self) -> bool;
}

#[allow(clippy::module_name_repetitions)]
pub struct RpcClient {
    call_id: u32,
    timeout: Option<Duration>,
    client: Arc<Mutex<dyn AsyncClient>>,
    processor_fut: Arc<std::sync::Mutex<JoinHandle<()>>>,
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
) where
    C: AsyncClient + 'static,
    H: RpcHandlers + Send + Sync + 'static,
{
    while let Ok(frame) = rx.recv().await {
        if frame.kind() == FrameKind::Message {
            match TryInto::<RpcEvent>::try_into(frame) {
                Ok(event) => match event.kind() {
                    RpcEventKind::Notification => {
                        trace!("RPC notification from {}", event.frame().sender());
                        let h = handlers.clone();
                        tokio::spawn(async move {
                            h.handle_notification(event).await;
                        });
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
                            let res = h.handle_call(event).await;
                            if let Some(ev) = ev {
                                macro_rules! send_reply {
                                    ($payload: expr, $result: expr) => {{
                                        let mut client = ev.1.lock().await;
                                        if let Some(result) = $result {
                                            client
                                                .zc_send(
                                                    &ev.0,
                                                    $payload,
                                                    result.into(),
                                                    QoS::Processed,
                                                )
                                                .await
                                        } else {
                                            client
                                                .zc_send(
                                                    &ev.0,
                                                    $payload,
                                                    (&[][..]).into(),
                                                    QoS::Processed,
                                                )
                                                .await
                                        }
                                    }};
                                }
                                match res {
                                    Ok(v) => {
                                        trace!("Sending RPC reply id {} to {}", id, ev.0);
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
                                            ev.0
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
                        if let Some(tx) = { calls.lock().unwrap().remove(&id) } {
                            let _r = tx.send(event);
                        } else {
                            warn!("orphaned RPC response: {}", id);
                        }
                    }
                },
                Err(e) => {
                    error!("{}", e);
                }
            }
        } else {
            handlers.handle_frame(frame).await;
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
    /// # Panics
    ///
    /// Should not panic
    pub fn new<H>(mut client: impl AsyncClient + 'static, handlers: H) -> Self
    where
        H: RpcHandlers + Send + Sync + 'static,
    {
        let timeout = client.get_timeout();
        let rx = { client.take_event_channel().unwrap() };
        let connected = client.get_connected_beacon();
        let client = Arc::new(Mutex::new(client));
        let calls: CallMap = <_>::default();
        let processor_fut = Arc::new(std::sync::Mutex::new(tokio::spawn(processor(
            rx,
            client.clone(),
            calls.clone(),
            Arc::new(handlers),
        ))));
        let pinger_client = client.clone();
        let pfut = processor_fut.clone();
        let pinger_fut = timeout.map(|t| {
            tokio::spawn(async move {
                loop {
                    if let Err(e) = pinger_client.lock().await.ping().await {
                        error!("{}", e);
                        pfut.lock().unwrap().abort();
                        break;
                    }
                    tokio::time::sleep(t).await;
                }
            })
        });
        Self {
            call_id: 0,
            timeout,
            client,
            processor_fut,
            pinger_fut,
            calls,
            connected,
        }
    }
    #[inline]
    fn increase_call_id(&mut self) {
        if self.call_id == u32::MAX {
            self.call_id = 1;
        } else {
            self.call_id += 1;
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
        &mut self,
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
    async fn call0(
        &self,
        target: &str,
        method: &str,
        params: Cow<'async_trait>,
    ) -> Result<OpConfirm, Error> {
        let payload = prepare_call_payload(method, &[0, 0, 0, 0]);
        self.client
            .lock()
            .await
            .zc_send(target, payload.into(), params, QoS::Processed)
            .await
    }
    /// # Panics
    ///
    /// Will panic on poisoned mutex
    async fn call(
        &mut self,
        target: &str,
        method: &str,
        params: Cow<'async_trait>,
    ) -> Result<RpcEvent, RpcError> {
        self.increase_call_id();
        let call_id = self.call_id;
        let payload = prepare_call_payload(method, &call_id.to_le_bytes());
        let (tx, rx) = oneshot::channel();
        self.calls.lock().unwrap().insert(call_id, tx);
        macro_rules! unwrap_or_cancel {
            ($result: expr) => {
                match $result {
                    Ok(v) => v,
                    Err(e) => {
                        self.calls.lock().unwrap().remove(&call_id);
                        return Err(Into::<Error>::into(e).into());
                    }
                }
            };
        }
        {
            let mut client = self.client.lock().await;
            let fut = client.zc_send(target, payload.into(), params, QoS::Processed);
            if let Some(timeout) = self.timeout {
                unwrap_or_cancel!(unwrap_or_cancel!(
                    unwrap_or_cancel!(unwrap_or_cancel!(tokio::time::timeout(timeout, fut).await))
                        .unwrap()
                        .await
                ));
            } else {
                unwrap_or_cancel!(unwrap_or_cancel!(
                    unwrap_or_cancel!(fut.await).unwrap().await
                ));
            }
        }
        let result = if let Some(timeout) = self.timeout {
            tokio::time::timeout(timeout, rx)
                .await
                .map_err(Into::<Error>::into)?
                .map_err(Into::<Error>::into)?
        } else {
            rx.await.map_err(Into::<Error>::into)?
        };
        Ok(result)
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
        self.processor_fut.lock().unwrap().abort();
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct RpcError {
    code: i16,
    data: Option<Vec<u8>>,
}

impl RpcError {
    pub fn new(code: i16, data: Option<Vec<u8>>) -> Self {
        Self { code, data }
    }
    pub fn method() -> Self {
        Self {
            code: RPC_ERROR_CODE_METHOD_NOT_FOUND,
            data: None,
        }
    }
    pub fn params() -> Self {
        Self {
            code: RPC_ERROR_CODE_INVALID_METHOD_PARAMS,
            data: None,
        }
    }
    pub fn parse() -> Self {
        Self {
            code: RPC_ERROR_CODE_PARSE,
            data: None,
        }
    }
    pub fn invalid() -> Self {
        Self {
            code: RPC_ERROR_CODE_INVALID_REQUEST,
            data: None,
        }
    }
}

impl From<Error> for RpcError {
    fn from(e: Error) -> RpcError {
        RpcError {
            code: -32000 - e.kind() as i16,
            data: None,
        }
    }
}

impl From<rmp_serde::encode::Error> for RpcError {
    fn from(e: rmp_serde::encode::Error) -> RpcError {
        RpcError {
            code: RPC_ERROR_CODE_INTERNAL,
            data: Some(e.to_string().as_bytes().to_vec()),
        }
    }
}

impl From<rmp_serde::decode::Error> for RpcError {
    fn from(e: rmp_serde::decode::Error) -> RpcError {
        RpcError {
            code: RPC_ERROR_CODE_PARSE,
            data: Some(e.to_string().as_bytes().to_vec()),
        }
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rpc error code: {}", self.code)
    }
}

#[allow(clippy::module_name_repetitions)]
pub type RpcResult = Result<Option<Vec<u8>>, RpcError>;