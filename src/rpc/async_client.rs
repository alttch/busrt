use super::{
    prepare_call_payload, RpcError, RpcEvent, RpcEventKind, RpcResult, RPC_ERROR,
    RPC_ERROR_CODE_METHOD_NOT_FOUND, RPC_NOTIFICATION, RPC_REPLY,
};
use crate::borrow::Cow;
use crate::client::AsyncClient;
use crate::EventChannel;
use crate::{Error, Frame, FrameKind, OpConfirm, QoS};
use async_trait::async_trait;
use log::{error, trace, warn};
#[cfg(not(feature = "rt"))]
use parking_lot::Mutex as SyncMutex;
#[cfg(feature = "rt")]
use parking_lot_rt::Mutex as SyncMutex;
use std::collections::BTreeMap;
use std::sync::atomic;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_task_pool::{Pool, Task};

/// By default, RPC frame and notification handlers are launched in background which allows
/// non-blocking event processing, however events can be processed in random order
///
/// RPC options allow to launch handlers in blocking mode. In this case handlers must process
/// events as fast as possible (e.g. send them to processing channels) and avoid using any RPC
/// client functions from inside.
///
/// WARNING: when handling frames in blocking mode, it is forbidden to use the current RPC client
/// directly or with any kind of bounded channels, otherwise the RPC client may get stuck!
///
/// See https://busrt.readthedocs.io/en/latest/rpc_blocking.html
#[derive(Default, Clone, Debug)]
pub struct Options {
    blocking_notifications: bool,
    blocking_frames: bool,
    task_pool: Option<Arc<Pool>>,
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
    #[inline]
    /// See <https://crates.io/crates/tokio-task-pool>
    pub fn with_task_pool(mut self, pool: Pool) -> Self {
        self.task_pool = Some(Arc::new(pool));
        self
    }
}

#[allow(clippy::module_name_repetitions)]
#[async_trait]
pub trait RpcHandlers {
    #[allow(unused_variables)]
    async fn handle_call(&self, event: RpcEvent) -> RpcResult {
        Err(RpcError::method(None))
    }
    #[allow(unused_variables)]
    async fn handle_notification(&self, event: RpcEvent) {}
    #[allow(unused_variables)]
    async fn handle_frame(&self, frame: Frame) {}
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
}

type CallMap = Arc<SyncMutex<BTreeMap<u32, oneshot::Sender<RpcEvent>>>>;

#[async_trait]
pub trait Rpc {
    /// When created, busrt client is wrapped with Arc<Mutex<_>> to let it be sent into
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
    call_id: SyncMutex<u32>,
    timeout: Option<Duration>,
    client: Arc<Mutex<dyn AsyncClient>>,
    processor_fut: Arc<SyncMutex<JoinHandle<()>>>,
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
    macro_rules! spawn {
        ($task_id: expr, $fut: expr) => {
            if let Some(ref pool) = opts.task_pool {
                let task = Task::new($fut).with_id($task_id);
                if let Err(e) = pool.spawn_task(task).await {
                    error!("Unable to spawn RPC task: {}", e);
                }
            } else {
                tokio::spawn($fut);
            }
        };
    }
    while let Ok(frame) = rx.recv().await {
        if frame.kind() == FrameKind::Message {
            match RpcEvent::try_from(frame) {
                Ok(event) => match event.kind() {
                    RpcEventKind::Notification => {
                        trace!("RPC notification from {}", event.frame().sender());
                        if opts.blocking_notifications {
                            handlers.handle_notification(event).await;
                        } else {
                            let h = handlers.clone();
                            spawn!("rpc.notification", async move {
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
                        spawn!("rpc.request", async move {
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
                                                .zc_send(&target, $payload, (&[][..]).into(), qos)
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
            }
        } else if opts.blocking_frames {
            handlers.handle_frame(frame).await;
        } else {
            let h = handlers.clone();
            spawn!("rpc.frame", async move {
                h.handle_frame(frame).await;
            });
        }
    }
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
        let processor_fut = Arc::new(SyncMutex::new(tokio::spawn(processor(
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
            call_id: SyncMutex::new(0),
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
            .map_or(true, |b| b.load(atomic::Ordering::Relaxed))
    }
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        self.pinger_fut.as_ref().map(JoinHandle::abort);
        self.processor_fut.lock().abort();
    }
}
