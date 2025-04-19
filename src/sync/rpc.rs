use crate::borrow::Cow;
use crate::rpc::{
    prepare_call_payload, RpcError, RpcEvent, RpcEventKind, RpcResult, RPC_ERROR,
    RPC_ERROR_CODE_METHOD_NOT_FOUND, RPC_NOTIFICATION, RPC_REPLY,
};
use crate::sync::client::SyncClient;
use crate::SyncEventChannel;
use crate::{Error, Frame, FrameKind, QoS, SyncOpConfirm};
use log::{error, trace, warn};
#[cfg(not(feature = "rt"))]
use parking_lot::Mutex;
#[cfg(feature = "rt")]
use parking_lot_rt::Mutex;
use std::collections::BTreeMap;
use std::sync::atomic;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[allow(clippy::module_name_repetitions)]
pub trait SyncRpcHandlers {
    #[allow(unused_variables)]
    fn handle_call(&self, event: RpcEvent) -> RpcResult {
        Err(RpcError::method(None))
    }
    #[allow(unused_variables)]
    fn handle_notification(&self, event: RpcEvent) {}
    #[allow(unused_variables)]
    fn handle_frame(&self, frame: Frame) {}
}

pub struct DummyHandlers {}

impl SyncRpcHandlers for DummyHandlers {
    fn handle_call(&self, _event: RpcEvent) -> RpcResult {
        Err(RpcError::new(
            RPC_ERROR_CODE_METHOD_NOT_FOUND,
            Some("RPC handler is not implemented".as_bytes().to_vec()),
        ))
    }
}

type CallMap = Arc<Mutex<BTreeMap<u32, oneshot::Sender<RpcEvent>>>>;

#[allow(clippy::module_name_repetitions)]
pub trait SyncRpc {
    /// When created, busrt client is wrapped with Arc<Mutex<_>> to let it be sent into
    /// the incoming frames handler future
    ///
    /// This mehtod allows to get the containered-client back, to call its methods directly (manage
    /// pub/sub and send broadcast messages)
    fn client(&self) -> Arc<Mutex<(dyn SyncClient + 'static)>>;
    fn notify(&self, target: &str, data: Cow<'_>, qos: QoS) -> Result<SyncOpConfirm, Error>;
    /// Call the method, no response is required
    fn call0(
        &self,
        target: &str,
        method: &str,
        params: Cow<'_>,
        qos: QoS,
    ) -> Result<SyncOpConfirm, Error>;
    /// Call the method and get the response
    fn call(
        &self,
        target: &str,
        method: &str,
        params: Cow<'_>,
        qos: QoS,
    ) -> Result<RpcEvent, RpcError>;
    fn is_connected(&self) -> bool;
}

#[allow(clippy::module_name_repetitions)]
pub struct RpcClient {
    call_id: Mutex<u32>,
    timeout: Option<Duration>,
    client: Arc<Mutex<dyn SyncClient>>,
    calls: CallMap,
    connected: Option<Arc<atomic::AtomicBool>>,
}

pub struct Processor {
    rx: SyncEventChannel,
    client: Arc<Mutex<dyn SyncClient + Send>>,
    calls: CallMap,
    handlers: Arc<dyn SyncRpcHandlers + Send + Sync>,
}

impl Processor {
    #[allow(clippy::too_many_lines)]
    pub fn run(self) {
        let Self {
            rx,
            client,
            calls,
            handlers,
        } = self;
        while let Ok(frame) = rx.recv() {
            if frame.kind() == FrameKind::Message {
                match RpcEvent::try_from(frame) {
                    Ok(event) => match event.kind() {
                        RpcEventKind::Notification => {
                            trace!("RPC notification from {}", event.frame().sender());
                            handlers.handle_notification(event);
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
                                Some((event.frame().sender().to_owned(), client.clone()))
                            } else {
                                None
                            };
                            let h = handlers.clone();
                            thread::spawn(move || {
                                let qos = if event.frame().is_realtime() {
                                    QoS::RealtimeProcessed
                                } else {
                                    QoS::Processed
                                };
                                let res = h.handle_call(event);
                                if let Some((target, cl)) = ev {
                                    macro_rules! send_reply {
                                        ($payload: expr, $result: expr) => {{
                                            let mut client = cl.lock();
                                            if let Some(result) = $result {
                                                client.zc_send(
                                                    &target,
                                                    $payload,
                                                    result.into(),
                                                    qos,
                                                )
                                            } else {
                                                client.zc_send(
                                                    &target,
                                                    $payload,
                                                    (&[][..]).into(),
                                                    qos,
                                                )
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
                                                e.code(),
                                                id,
                                                target,
                                            );
                                            let mut payload = Vec::with_capacity(7);
                                            payload.push(RPC_ERROR);
                                            payload.extend_from_slice(&id.to_le_bytes());
                                            payload.extend_from_slice(&e.code().to_le_bytes());
                                            let _r = send_reply!(payload.into(), e.data());
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
                }
            } else {
                handlers.handle_frame(frame);
            }
        }
    }
}

impl RpcClient {
    /// creates RPC client with the specified handlers and the default options
    pub fn new<H>(client: impl SyncClient + Send + 'static, handlers: H) -> (Self, Processor)
    where
        H: SyncRpcHandlers + Send + Sync + 'static,
    {
        Self::init(client, handlers)
    }

    /// creates RPC client with dummy handlers and the default options
    pub fn new0(client: impl SyncClient + Send + 'static) -> (Self, Processor) {
        Self::init(client, DummyHandlers {})
    }

    fn init<H>(mut client: impl SyncClient + Send + 'static, handlers: H) -> (Self, Processor)
    where
        H: SyncRpcHandlers + Send + Sync + 'static,
    {
        let timeout = client.get_timeout();
        let rx = { client.take_event_channel().unwrap() };
        let connected = client.get_connected_beacon();
        let client = Arc::new(Mutex::new(client));
        let calls: CallMap = <_>::default();
        let processor = Processor {
            rx,
            client: client.clone(),
            calls: calls.clone(),
            handlers: Arc::new(handlers),
        };
        (
            Self {
                call_id: Mutex::new(0),
                timeout,
                client,
                calls,
                connected,
            },
            processor,
        )
    }
}

impl SyncRpc for RpcClient {
    #[inline]
    fn client(&self) -> Arc<Mutex<(dyn SyncClient + 'static)>> {
        self.client.clone()
    }
    #[inline]
    fn notify(&self, target: &str, data: Cow<'_>, qos: QoS) -> Result<SyncOpConfirm, Error> {
        self.client
            .lock()
            .zc_send(target, (&[RPC_NOTIFICATION][..]).into(), data, qos)
    }
    fn call0(
        &self,
        target: &str,
        method: &str,
        params: Cow<'_>,
        qos: QoS,
    ) -> Result<SyncOpConfirm, Error> {
        let payload = prepare_call_payload(method, &[0, 0, 0, 0]);
        self.client
            .lock()
            .zc_send(target, payload.into(), params, qos)
    }
    /// # Panics
    ///
    /// Will panic on poisoned mutex
    fn call(
        &self,
        target: &str,
        method: &str,
        params: Cow<'_>,
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
        let mut client = self.client.lock();
        client.zc_send(target, payload.into(), params, qos)?;
        let result = if let Some(timeout) = self.timeout {
            match rx.recv_timeout(timeout) {
                Ok(v) => v,
                Err(oneshot::RecvTimeoutError::Timeout) => {
                    self.calls.lock().remove(&call_id);
                    return Err(Error::timeout().into());
                }
                Err(e) => {
                    self.calls.lock().remove(&call_id);
                    return Err(Error::io(e).into());
                }
            }
        } else {
            match rx.recv() {
                Ok(v) => v,
                Err(e) => {
                    self.calls.lock().remove(&call_id);
                    return Err(Error::io(e).into());
                }
            }
        };
        if let Ok(e) = RpcError::try_from(&result) {
            Err(e)
        } else {
            Ok(result)
        }
    }
    fn is_connected(&self) -> bool {
        self.connected
            .as_ref()
            .map_or(true, |b| b.load(atomic::Ordering::Relaxed))
    }
}
