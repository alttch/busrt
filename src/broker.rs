#[cfg(feature = "rpc")]
use crate::common::now_ns;
use log::{error, info, trace};
#[cfg(feature = "rpc")]
use serde::{Deserialize, Serialize};
use std::collections::{hash_map, HashMap};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::Unpin;
use std::net::SocketAddr;
use std::sync::atomic;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use submap::{BroadcastMap, SubMap};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};
#[cfg(feature = "rpc")]
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time;

use crate::{Error, ErrorKind, GREETINGS, PROTOCOL_VERSION};

use crate::ERR_DATA;
use crate::ERR_NOT_SUPPORTED;
use crate::RESPONSE_OK;

use crate::OP_ACK;

use crate::borrow::Cow;
use crate::client::AsyncClient;
#[cfg(feature = "broker-api")]
use crate::common::{ClientInfo, ClientList};
use crate::{EventChannel, OpConfirm};
use crate::{Frame, FrameData, FrameKind, FrameOp, QoS};

#[cfg(feature = "rpc")]
use crate::rpc::{Rpc, RpcClient};
#[cfg(feature = "broker-api")]
use crate::rpc::{RpcError, RpcEvent, RpcHandlers, RpcResult};
#[cfg(feature = "broker-api")]
use serde_value::Value;

use async_trait::async_trait;

pub const DEFAULT_QUEUE_SIZE: usize = 8192;

pub const BROKER_INFO_TOPIC: &str = ".broker/info";
pub const BROKER_WARN_TOPIC: &str = ".broker/warn";
pub const BROKER_NAME: &str = ".broker";

#[allow(dead_code)]
const BROKER_RPC_NOT_INIT_ERR: &str = "broker core RPC client not initialized";

macro_rules! pretty_error {
    ($name: expr, $err:expr) => {
        if $err.kind() != ErrorKind::Eof {
            error!("client {} error: {}", $name, $err);
        }
    };
}

type BrokerClient = Arc<ElbusClient>;

macro_rules! make_confirm_channel {
    ($qos: expr) => {
        match $qos {
            QoS::No => Ok(None),
            QoS::Processed => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let _r = tx.send(Ok(()));
                Ok(Some(rx))
            }
        }
    };
}

macro_rules! send {
    ($db:expr, $client:expr, $target:expr, $header: expr, $buf:expr, $payload_pos:expr) => {{
        trace!("elbus message from {} to {}", $client, $target);
        let tx = {
            $db.clients
                .read()
                .unwrap()
                .get($target)
                .map(|c| c.tx.clone())
        };
        if let Some(tx) = tx {
            let frame = Arc::new(FrameData {
                kind: FrameKind::Message,
                sender: Some($client.name.clone()),
                topic: None,
                header: $header,
                buf: $buf,
                payload_pos: $payload_pos,
            });
            tx.send(frame).await.map_err(Into::into)
        } else {
            Err(Error::not_registered())
        }
    }};
}

macro_rules! send_broadcast {
    ($db:expr, $client:expr, $target:expr, $header: expr, $buf:expr, $payload_pos:expr) => {{
        trace!("elbus broadcast message from {} to {}", $client, $target);
        let subs = { $db.broadcasts.read().unwrap().get_clients_by_mask($target) };
        if !subs.is_empty() {
            let frame = Arc::new(FrameData {
                kind: FrameKind::Broadcast,
                sender: Some($client.name.clone()),
                topic: None,
                header: $header,
                buf: $buf,
                payload_pos: $payload_pos,
            });
            for sub in subs {
                let _r = sub.tx.send(frame.clone()).await;
            }
        }
    }};
}

macro_rules! publish {
    ($db:expr, $client:expr, $topic:expr, $header: expr, $buf:expr, $payload_pos:expr) => {{
        trace!("elbus topic publish from {} to {}", $client, $topic);
        let subs = { $db.subscriptions.read().unwrap().get_subscribers($topic) };
        if !subs.is_empty() {
            let frame = Arc::new(FrameData {
                kind: FrameKind::Publish,
                sender: Some($client.name.clone()),
                topic: Some($topic.to_owned()),
                header: $header,
                buf: $buf,
                payload_pos: $payload_pos,
            });
            for sub in subs {
                let _r = sub.tx.send(frame.clone()).await;
            }
        }
    }};
}

pub struct Client {
    name: String,
    client: Arc<ElbusClient>,
    db: Arc<BrokerDb>,
    rx: Option<EventChannel>,
}

#[async_trait]
impl AsyncClient for Client {
    /// # Panics
    ///
    /// Will panic if the mutex is poisoned
    async fn subscribe(&mut self, topic: &str, qos: QoS) -> Result<OpConfirm, Error> {
        if self
            .db
            .subscriptions
            .write()
            .unwrap()
            .subscribe(topic, &self.client)
        {
            make_confirm_channel!(qos)
        } else {
            Err(Error::not_registered())
        }
    }
    /// # Panics
    ///
    /// Will panic if the mutex is poisoned
    async fn subscribe_bulk(&mut self, topics: Vec<&str>, qos: QoS) -> Result<OpConfirm, Error> {
        let mut db = self.db.subscriptions.write().unwrap();
        for topic in topics {
            if !db.subscribe(topic, &self.client) {
                return Err(Error::not_registered());
            }
        }
        make_confirm_channel!(qos)
    }
    /// # Panics
    ///
    /// Will panic if the mutex is poisoned
    async fn unsubscribe(&mut self, topic: &str, qos: QoS) -> Result<OpConfirm, Error> {
        if self
            .db
            .subscriptions
            .write()
            .unwrap()
            .unsubscribe(topic, &self.client)
        {
            make_confirm_channel!(qos)
        } else {
            Err(Error::not_registered())
        }
    }
    /// # Panics
    ///
    /// Will panic if the mutex is poisoned
    async fn unsubscribe_bulk(&mut self, topics: Vec<&str>, qos: QoS) -> Result<OpConfirm, Error> {
        let mut db = self.db.subscriptions.write().unwrap();
        for topic in topics {
            if !db.unsubscribe(topic, &self.client) {
                return Err(Error::not_registered());
            }
        }
        make_confirm_channel!(qos)
    }
    #[inline]
    async fn send(
        &mut self,
        target: &str,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error> {
        send!(self.db, self.client, target, None, payload.to_vec(), 0)?;
        make_confirm_channel!(qos)
    }
    #[inline]
    async fn zc_send(
        &mut self,
        target: &str,
        header: Cow<'async_trait>,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error> {
        send!(
            self.db,
            self.client,
            target,
            Some(header.to_vec()),
            payload.to_vec(),
            0
        )?;
        make_confirm_channel!(qos)
    }
    #[inline]
    async fn send_broadcast(
        &mut self,
        target: &str,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error> {
        send_broadcast!(self.db, self.client, target, None, payload.to_vec(), 0);
        make_confirm_channel!(qos)
    }
    #[inline]
    async fn publish(
        &mut self,
        topic: &str,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error> {
        publish!(self.db, self.client, topic, None, payload.to_vec(), 0);
        make_confirm_channel!(qos)
    }
    #[inline]
    fn take_event_channel(&mut self) -> Option<EventChannel> {
        self.rx.take()
    }
    #[inline]
    async fn ping(&mut self) -> Result<(), Error> {
        Ok(())
    }
    #[inline]
    fn is_connected(&self) -> bool {
        true
    }
    #[inline]
    fn get_timeout(&self) -> Option<Duration> {
        None
    }
    #[inline]
    fn get_connected_beacon(&self) -> Option<Arc<atomic::AtomicBool>> {
        None
    }
    #[inline]
    fn get_name(&self) -> &str {
        self.name.as_str()
    }
}

impl Client {
    #[inline]
    pub async fn unregister(&self) {
        self.db.unregister_client(&self.client).await;
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.db.drop_client(&self.client);
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ElbusClientType {
    Internal,
    LocalIpc,
    Tcp,
}

impl ElbusClientType {
    #[allow(dead_code)]
    fn as_str(&self) -> &str {
        match self {
            ElbusClientType::Internal => "internal",
            ElbusClientType::LocalIpc => "local_ipc",
            ElbusClientType::Tcp => "tcp",
        }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
struct ElbusClient {
    name: String,
    tp: ElbusClientType,
    source: Option<String>,
    port: Option<String>,
    tx: async_channel::Sender<Frame>,
}

impl fmt::Display for ElbusClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl ElbusClient {
    pub fn new(
        name: &str,
        queue_size: usize,
        tp: ElbusClientType,
        source: Option<String>,
        port: Option<String>,
    ) -> (Self, EventChannel) {
        let (tx, rx) = async_channel::bounded(queue_size);
        (
            Self {
                name: name.to_owned(),
                tp,
                source,
                port,
                tx,
            },
            rx,
        )
    }
}

impl PartialEq for ElbusClient {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for ElbusClient {}

impl Hash for ElbusClient {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

#[cfg_attr(feature = "rpc", derive(Serialize, Deserialize))]
#[derive(Eq, PartialEq, Clone)]
#[allow(clippy::module_name_repetitions)]
pub struct BrokerEvent<'a> {
    s: &'a str,
    #[cfg_attr(feature = "rpc", serde(skip_serializing_if = "Option::is_none"))]
    d: Option<&'a str>,
    t: u64,
    #[cfg_attr(feature = "rpc", serde(skip))]
    topic: &'a str,
}

impl<'a> BrokerEvent<'a> {
    pub fn new(s: &'a str, d: Option<&'a str>, topic: &'a str) -> Self {
        Self { s, d, t: 0, topic }
    }
    pub fn shutdown() -> Self {
        Self {
            s: "shutdown",
            d: None,
            t: 0,
            topic: BROKER_WARN_TOPIC,
        }
    }
    pub fn reg(name: &'a str) -> Self {
        Self {
            s: "reg",
            d: Some(name),
            t: 0,
            topic: BROKER_INFO_TOPIC,
        }
    }
    pub fn unreg(name: &'a str) -> Self {
        Self {
            s: "unreg",
            d: Some(name),
            t: 0,
            topic: BROKER_INFO_TOPIC,
        }
    }
    pub fn subject(&self) -> &str {
        self.s
    }
    pub fn data(&self) -> Option<&str> {
        self.d
    }
    pub fn time(&self) -> u64 {
        self.t
    }
}

struct BrokerDb {
    clients: RwLock<HashMap<String, BrokerClient>>,
    broadcasts: RwLock<BroadcastMap<BrokerClient>>,
    subscriptions: RwLock<SubMap<BrokerClient>>,
    #[cfg(feature = "rpc")]
    rpc_client: Arc<Mutex<Option<RpcClient>>>,
}

impl Default for BrokerDb {
    fn default() -> Self {
        Self {
            clients: <_>::default(),
            broadcasts: RwLock::new(
                BroadcastMap::new()
                    .separator('.')
                    .match_any("?")
                    .wildcard("*"),
            ),
            subscriptions: RwLock::new(SubMap::new().separator('/').match_any("+").wildcard("#")),
            #[cfg(feature = "rpc")]
            rpc_client: <_>::default(),
        }
    }
}

impl BrokerDb {
    #[cfg(feature = "rpc")]
    #[inline]
    async fn announce(&self, mut event: BrokerEvent<'_>) -> Result<(), Error> {
        if let Some(rpc_client) = self.rpc_client.lock().await.as_ref() {
            event.t = now_ns();
            rpc_client
                .client()
                .lock()
                .await
                .publish(
                    event.topic,
                    rmp_serde::to_vec_named(&event).map_err(Error::data)?.into(),
                    QoS::No,
                )
                .await?;
        }
        Ok(())
    }
    async fn register_client(&self, client: Arc<ElbusClient>) -> Result<(), Error> {
        #[cfg(feature = "rpc")]
        // copy name for the announce
        let name = client.name.clone();
        if let hash_map::Entry::Vacant(x) = self.clients.write().unwrap().entry(client.name.clone())
        {
            {
                let mut bdb = self.broadcasts.write().unwrap();
                bdb.register_client(&client.name, &client);
            }
            {
                let mut sdb = self.subscriptions.write().unwrap();
                sdb.register_client(&client);
                sdb.subscribe(BROKER_WARN_TOPIC, &client);
            }
            x.insert(client);
        } else {
            return Err(Error::busy(format!(
                "the client is already registred: {}",
                client.name
            )));
        }
        #[cfg(feature = "rpc")]
        if let Err(e) = self.announce(BrokerEvent::reg(&name)).await {
            error!("{}", e);
        }
        Ok(())
    }
    #[inline]
    async fn unregister_client(&self, client: &Arc<ElbusClient>) {
        self.drop_client(client);
        #[cfg(feature = "rpc")]
        if let Err(e) = self.announce(BrokerEvent::unreg(&client.name)).await {
            error!("{}", e);
        }
    }
    fn drop_client(&self, client: &Arc<ElbusClient>) {
        self.subscriptions
            .write()
            .unwrap()
            .unregister_client(client);
        self.broadcasts
            .write()
            .unwrap()
            .unregister_client(&client.name, client);
        self.clients.write().unwrap().remove(&client.name);
    }
}
pub struct Broker {
    db: Arc<BrokerDb>,
    services: Vec<JoinHandle<()>>,
    queue_size: usize,
}

#[cfg(feature = "broker-api")]
struct BrokerRpcHandlers {
    db: Arc<BrokerDb>,
}

#[cfg(feature = "broker-api")]
#[async_trait]
impl RpcHandlers for BrokerRpcHandlers {
    async fn handle_call(&self, event: RpcEvent) -> RpcResult {
        let method = event.parse_method()?;
        if method == "benchmark.test" {
            return Ok(Some(event.payload().to_vec()));
        }
        let payload = event.payload();
        let params: HashMap<String, Value> = if payload.is_empty() {
            HashMap::new()
        } else {
            rmp_serde::from_read_ref(event.payload())?
        };
        match event.parse_method()? {
            "client.list" => {
                if !params.is_empty() {
                    return Err(RpcError::params());
                }
                let db = self.db.clients.read().unwrap();
                let mut clients: Vec<ClientInfo> = db
                    .values()
                    .into_iter()
                    .map(|v| ClientInfo {
                        name: &v.name,
                        tp: v.tp.as_str(),
                        source: v.source.as_deref(),
                        port: v.port.as_deref(),
                    })
                    .collect();
                clients.sort();
                Ok(Some(rmp_serde::to_vec_named(&ClientList { clients })?))
            }
            _ => Err(RpcError::method()),
        }
    }
    async fn handle_notification(&self, _event: RpcEvent) {}
    async fn handle_frame(&self, _frame: Frame) {}
}

#[allow(clippy::unnecessary_wraps)]
#[inline]
fn prepare_unix_stream(_stream: &UnixStream) -> Result<(), Error> {
    Ok(())
}

#[inline]
fn prepare_tcp_stream(stream: &TcpStream) -> Result<(), Error> {
    stream.set_nodelay(true).map_err(Into::into)
}

#[allow(clippy::unnecessary_wraps)]
fn prepare_tcp_source(addr: SocketAddr) -> Option<String> {
    Some(addr.to_string())
}

#[allow(clippy::unnecessary_wraps)]
fn prepare_unix_source(_addr: tokio::net::unix::SocketAddr) -> Option<String> {
    None
}

macro_rules! spawn_server {
    ($self: expr, $path: expr, $listener: expr, $buf_size: expr, $timeout: expr, $tp: expr,
     $prepare: ident, $prepare_source: ident) => {{
        let socket_path = $path.to_owned();
        let db = $self.db.clone();
        let queue_size = $self.queue_size;
        let service = tokio::spawn(async move {
            loop {
                match $listener.accept().await {
                    Ok((stream, addr)) => {
                        trace!(
                            "elbus tcp client connected from {:?} to {}",
                            addr,
                            socket_path
                        );
                        if let Err(e) = $prepare(&stream) {
                            error!("{}", e);
                            continue;
                        }
                        let (reader, writer) = stream.into_split();
                        let reader = BufReader::with_capacity($buf_size, reader);
                        let writer = BufWriter::with_capacity($buf_size, writer);
                        let cdb = db.clone();
                        let name = socket_path.clone();
                        let client_source = $prepare_source(addr);
                        let client_path = socket_path.clone();
                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_peer(PeerHandlerParams {
                                db: cdb,
                                reader,
                                writer,
                                timeout: $timeout,
                                queue_size,
                                tp: $tp,
                                source: client_source,
                                source_port: Some(client_path),
                            })
                            .await
                            {
                                pretty_error!(name, e);
                            }
                        });
                    }
                    Err(e) => error!("{}", e),
                }
            }
        });
        $self.services.push(service);
    }};
}

struct PeerHandlerParams<R, W>
where
    R: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin + Send + 'static,
{
    db: Arc<BrokerDb>,
    reader: R,
    writer: W,
    timeout: Duration,
    queue_size: usize,
    tp: ElbusClientType,
    source: Option<String>,
    source_port: Option<String>,
}

impl Broker {
    pub async fn create() -> Self {
        let broker_db: Arc<BrokerDb> = <_>::default();
        let mut broker = Self {
            #[cfg(feature = "broker-api")]
            db: broker_db.clone(),
            #[cfg(not(feature = "broker-api"))]
            db: broker_db,
            services: <_>::default(),
            queue_size: 0,
        };
        // avoid warning if rpc feature is not set
        broker.queue_size = DEFAULT_QUEUE_SIZE;
        #[cfg(feature = "broker-api")]
        {
            let client = broker
                .register_client(BROKER_NAME)
                .await
                .expect("can not register broker RPC");
            let handlers = BrokerRpcHandlers { db: broker_db };
            let rpc_client = RpcClient::new(client, handlers);
            broker.db.rpc_client.lock().await.replace(rpc_client);
        }
        broker
    }
    pub fn set_queue_size(&mut self, queue_size: usize) {
        self.queue_size = queue_size;
    }
    #[cfg(feature = "rpc")]
    #[inline]
    pub async fn set_core_rpc_client(&self, client: RpcClient) {
        self.db.rpc_client.lock().await.replace(client);
    }
    #[cfg(feature = "rpc")]
    #[inline]
    pub fn core_rpc_client(&self) -> Arc<Mutex<Option<RpcClient>>> {
        self.db.rpc_client.clone()
    }
    #[cfg(feature = "rpc")]
    /// Publish announce for clients
    pub async fn announce(&self, event: BrokerEvent<'_>) -> Result<(), Error> {
        self.db.announce(event).await
    }
    pub async fn register_client(&self, name: &str) -> Result<Client, Error> {
        let (c, rx) =
            ElbusClient::new(name, self.queue_size, ElbusClientType::Internal, None, None);
        let client = Arc::new(c);
        self.db.register_client(client.clone()).await?;
        Ok(Client {
            name: name.to_owned(),
            client,
            db: self.db.clone(),
            rx: Some(rx),
        })
    }
    #[inline]
    pub async fn unregister_client(&self, client: &Client) {
        self.db.unregister_client(&client.client).await;
    }
    pub async fn spawn_unix_server(
        &mut self,
        path: &str,
        buf_size: usize,
        timeout: Duration,
    ) -> Result<(), Error> {
        let _r = tokio::fs::remove_file(path).await;
        let listener = UnixListener::bind(path)?;
        spawn_server!(
            self,
            path,
            listener,
            buf_size,
            timeout,
            ElbusClientType::LocalIpc,
            prepare_unix_stream,
            prepare_unix_source
        );
        Ok(())
    }
    pub async fn spawn_tcp_server(
        &mut self,
        path: &str,
        buf_size: usize,
        timeout: Duration,
    ) -> Result<(), Error> {
        let listener = TcpListener::bind(path).await?;
        spawn_server!(
            self,
            path,
            listener,
            buf_size,
            timeout,
            ElbusClientType::Tcp,
            prepare_tcp_stream,
            prepare_tcp_source
        );
        Ok(())
    }
    #[allow(clippy::items_after_statements)]
    /// Broker fifo channel is useful for shell scripts and allows to send:
    ///
    /// echo TARGET MESSAGE > /path/to/fifo # a one-to-one or broadcast message
    /// echo '=TOPIC' MESSAGE # publish to a topic
    /// echo TARGET .MESSAGE # RPC notification
    /// echo TARGET :method param=value param=value # RPC call, the payload will be sent as msgpack
    ///
    /// Requires either broker-api feature or rpc feature + broker core rpc client to be set
    #[cfg(feature = "rpc")]
    pub async fn spawn_fifo(&mut self, path: &str, buf_size: usize) -> Result<(), Error> {
        let rpc_client = self.db.rpc_client.clone();
        if rpc_client.lock().await.is_none() {
            return Err(Error::not_supported(BROKER_RPC_NOT_INIT_ERR));
        }
        let _r = tokio::fs::remove_file(path).await;
        unix_named_pipe::create(path, Some(0o622))?;
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::AsyncBufReadExt;
        // chown fifo as it's usually created with 644
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o622)).await?;
        let fd = unix_named_pipe::open_read(path)?;
        let socket_path = path.to_owned();
        let service = tokio::spawn(async move {
            let f = tokio::fs::File::from_std(fd);
            let reader = BufReader::with_capacity(buf_size, f);
            let mut lines = reader.lines();
            let sleep_step = Duration::from_millis(100);
            loop {
                while let Some(line) = match lines.next_line().await {
                    Ok(v) => v,
                    Err(e) => {
                        error!("{}: {}", socket_path, e);
                        None
                    }
                } {
                    if let Err(e) = Self::send_fifo_cmd(&rpc_client, line).await {
                        error!("{}: {}", socket_path, e);
                    }
                }
                tokio::time::sleep(sleep_step).await;
            }
        });
        self.services.push(service);
        Ok(())
    }
    #[cfg(feature = "rpc")]
    async fn send_fifo_cmd(
        rpc_c: &Arc<Mutex<Option<RpcClient>>>,
        line: String,
    ) -> Result<(), Error> {
        let cmd = line.trim();
        let mut c = rpc_c.lock().await;
        let rpc = if let Some(rpc) = c.as_mut() {
            rpc
        } else {
            return Err(Error::not_supported(BROKER_RPC_NOT_INIT_ERR));
        };
        // topic
        if let Some(s) = cmd.strip_prefix('=') {
            let mut sp = s.split(' ');
            let topic = sp
                .next()
                .ok_or_else(|| Error::data("topic not specified"))?;
            let payload = sp
                .next()
                .ok_or_else(|| Error::data("payload not specified"))?;
            rpc.client()
                .lock()
                .await
                .publish(topic, payload.as_bytes().into(), QoS::No)
                .await?;
            Ok(())
        } else {
            let mut sp = line.split(' ');
            let target = sp
                .next()
                .ok_or_else(|| Error::data("target not specified"))?;
            let payload = sp
                .next()
                .ok_or_else(|| Error::data("payload not specified"))?;
            // rpc notification
            if let Some(s) = payload.strip_prefix('.') {
                rpc.notify(target, s.as_bytes().into(), QoS::No).await?;
                Ok(())
            } else if let Some(method) = payload.strip_prefix(':') {
                let s = sp.collect::<Vec<&str>>();
                let params = crate::common::str_to_params_map(&s)?;
                rpc.call0(
                    target,
                    method,
                    rmp_serde::to_vec_named(&params)
                        .map_err(Error::data)?
                        .into(),
                )
                .await?;
                Ok(())
            } else {
                // regular message
                // broadcast
                if target.contains(&['*', '?'][..]) {
                    rpc.client()
                        .lock()
                        .await
                        .send_broadcast(target, payload.as_bytes().into(), QoS::No)
                        .await?;
                    Ok(())
                } else {
                    rpc.client()
                        .lock()
                        .await
                        .send(target, payload.as_bytes().into(), QoS::No)
                        .await?;
                    Ok(())
                }
            }
        }
    }
    #[allow(clippy::too_many_lines)]
    async fn handle_peer<R, W>(params: PeerHandlerParams<R, W>) -> Result<(), Error>
    where
        R: AsyncReadExt + Unpin,
        W: AsyncWriteExt + Unpin + Send + 'static,
    {
        let timeout = params.timeout;
        let mut reader = params.reader;
        let mut writer = params.writer;
        let queue_size = params.queue_size;
        let db = params.db;
        macro_rules! write_and_flush {
            ($buf: expr) => {
                time::timeout(timeout, writer.write_all($buf)).await??;
                time::timeout(timeout, writer.flush()).await??;
            };
        }
        let mut buf = GREETINGS.to_vec();
        buf.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        write_and_flush!(&buf);
        let mut buf = vec![0; 3];
        time::timeout(timeout, reader.read_exact(&mut buf)).await??;
        if buf[0] != GREETINGS[0] {
            write_and_flush!(&[ERR_NOT_SUPPORTED]);
            return Err(Error::not_supported("invalid protocol"));
        }
        if u16::from_le_bytes(buf[1..3].try_into().unwrap()) != PROTOCOL_VERSION {
            write_and_flush!(&[ERR_NOT_SUPPORTED]);
            return Err(Error::not_supported("unsupported protocol version"));
        }
        write_and_flush!(&[RESPONSE_OK]);
        let mut buf = vec![0; 2];
        time::timeout(timeout, reader.read_exact(&mut buf)).await??;
        let len = u16::from_le_bytes(buf.try_into().unwrap());
        let mut buf = vec![0; len as usize];
        time::timeout(timeout, reader.read_exact(&mut buf)).await??;
        let client_name = std::str::from_utf8(&buf)?.to_owned();
        if client_name.is_empty() || client_name.starts_with('.') {
            write_and_flush!(&[ERR_DATA]);
            return Err(Error::data("Invalid client name"));
        }
        let (client, rx) = {
            let (c, rx) = ElbusClient::new(
                &client_name,
                queue_size,
                params.tp,
                params.source,
                params.source_port,
            );
            let client = Arc::new(c);
            if let Err(e) = db.register_client(client.clone()).await {
                write_and_flush!(&[e.kind as u8]);
                return Err(e);
            }
            write_and_flush!(&[RESPONSE_OK]);
            (client, rx)
        };
        info!("elbus client registered: {}", client_name);
        let w_name = client_name.clone();
        let writer_fut = tokio::spawn(async move {
            while let Ok(frame) = rx.recv().await {
                macro_rules! write_data {
                    ($data: expr) => {
                        if !$data.is_empty() {
                            match time::timeout(timeout, writer.write_all($data)).await {
                                Ok(result) => {
                                    if let Err(e) = result {
                                        pretty_error!(w_name, Into::<Error>::into(&e));
                                        break;
                                    }
                                }
                                Err(_) => {
                                    error!("client {} error: timeout", w_name);
                                    break;
                                }
                            }
                        }
                    };
                }
                macro_rules! flush {
                    () => {
                        match time::timeout(timeout, writer.flush()).await {
                            Ok(result) => {
                                if let Err(e) = result {
                                    pretty_error!(w_name, Into::<Error>::into(&e));
                                    break;
                                }
                            }
                            Err(_) => {
                                error!("client {} error: timeout", w_name);
                                break;
                            }
                        }
                    };
                }
                if frame.kind == FrameKind::Prepared {
                    write_data!(&frame.buf);
                    flush!();
                } else {
                    let sender = frame.sender.as_ref().unwrap().as_bytes();
                    let topic = frame.topic.as_ref().map(String::as_bytes);
                    let mut extra_len = sender.len();
                    if let Some(t) = topic.as_ref() {
                        extra_len += t.len() + 1;
                    }
                    if let Some(header) = frame.header.as_ref() {
                        extra_len += header.len();
                    }
                    let mut buf = Vec::with_capacity(7 + extra_len);
                    buf.push(frame.kind as u8); // byte 0
                    let frame_len = extra_len + frame.buf.len() - frame.payload_pos + 1;
                    #[allow(clippy::cast_possible_truncation)]
                    buf.extend_from_slice(&(frame_len as u32).to_le_bytes()); // bytes 1-4
                    buf.push(0x00); // byte 5 - reserved
                    buf.extend_from_slice(sender);
                    buf.push(0x00);
                    if let Some(t) = topic.as_ref() {
                        buf.extend_from_slice(t);
                        buf.push(0x00);
                    };
                    write_data!(&buf);
                    if let Some(header) = frame.header() {
                        write_data!(header);
                    }
                    write_data!(frame.payload());
                    flush!();
                }
            }
        });
        let result = Self::handle_reader(&db, client.clone(), &mut reader, timeout).await;
        writer_fut.abort();
        db.unregister_client(&client).await;
        info!("elbus client disconnected: {}", client_name);
        result
    }

    // TODO send ack only after the client received message (QoS2)
    #[allow(clippy::too_many_lines)]
    async fn handle_reader<R>(
        db: &BrokerDb,
        client: Arc<ElbusClient>,
        reader: &mut R,
        timeout: Duration,
    ) -> Result<(), Error>
    where
        R: AsyncReadExt + Unpin,
    {
        loop {
            let mut buf = vec![0; 9];
            reader.read_exact(&mut buf).await?;
            let flags = buf[4];
            if flags == 0 {
                // OP_NOP
                trace!("{} ping", client);
                continue;
            }
            let op_id = &buf[0..4];
            let op: FrameOp = (flags & 0b0011_1111).try_into()?;
            let qos: QoS = (flags >> 6 & 0b0011_1111).try_into()?;
            let len = u32::from_le_bytes(buf[5..9].try_into().unwrap());
            let mut buf = vec![0; len as usize];
            time::timeout(timeout, reader.read_exact(&mut buf)).await??;
            macro_rules! send_ack {
                ($code:expr) => {
                    let mut buf = Vec::with_capacity(6);
                    buf.push(OP_ACK);
                    buf.extend_from_slice(op_id);
                    buf.push($code);
                    client
                        .tx
                        .send(Arc::new(FrameData {
                            kind: FrameKind::Prepared,
                            sender: None,
                            topic: None,
                            header: None,
                            buf,
                            payload_pos: 0,
                        }))
                        .await?;
                };
            }
            match op {
                FrameOp::SubscribeTopic => {
                    let sp = buf.split(|c| *c == 0);
                    {
                        let mut sdb = db.subscriptions.write().unwrap();
                        for t in sp {
                            let topic = std::str::from_utf8(t)?;
                            sdb.subscribe(topic, &client);
                            trace!("elbus client {} subscribed to topic {}", client, topic);
                        }
                    }
                    if qos == QoS::Processed {
                        send_ack!(RESPONSE_OK);
                    }
                }
                FrameOp::UnsubscribeTopic => {
                    let sp = buf.split(|c| *c == 0);
                    {
                        let mut sdb = db.subscriptions.write().unwrap();
                        for t in sp {
                            let topic = std::str::from_utf8(t)?;
                            sdb.unsubscribe(topic, &client);
                            trace!("elbus client {} unsubscribed from topic {}", client, topic);
                        }
                    }
                    if qos == QoS::Processed {
                        send_ack!(RESPONSE_OK);
                    }
                }
                _ => {
                    let mut sp = buf.splitn(2, |c| *c == 0);
                    let tgt = sp.next().ok_or_else(|| Error::data("broken frame"))?;
                    let target = std::str::from_utf8(tgt)?;
                    sp.next().ok_or_else(|| Error::data("broken frame"))?;
                    let payload_pos = tgt.len() + 1;
                    drop(sp);
                    match op {
                        FrameOp::Message => {
                            if let Err(e) = send!(db, client, target, None, buf, payload_pos) {
                                if qos == QoS::Processed {
                                    send_ack!(e.kind as u8);
                                }
                            } else if qos == QoS::Processed {
                                send_ack!(RESPONSE_OK);
                            }
                        }
                        FrameOp::Broadcast => {
                            send_broadcast!(db, client, target, None, buf, payload_pos);
                            if qos == QoS::Processed {
                                send_ack!(RESPONSE_OK);
                            }
                        }
                        FrameOp::PublishTopic => {
                            publish!(db, client, target, None, buf, payload_pos);
                            if qos == QoS::Processed {
                                send_ack!(RESPONSE_OK);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        for service in &self.services {
            service.abort();
        }
    }
}
