use crate::borrow::Cow;
use crate::client::AsyncClient;
use crate::comm::{Flush, TtlBufWriter};
#[cfg(feature = "rpc")]
use crate::common::now_ns;
use crate::common::{BrokerInfo, BrokerStats};
#[cfg(feature = "rpc")]
use crate::common::{ClientInfo, ClientList};
use crate::SECONDARY_SEP;
use crate::{Error, ErrorKind, GREETINGS, PROTOCOL_VERSION};
use crate::{EventChannel, OpConfirm};
use crate::{Frame, FrameData, FrameKind, FrameOp, QoS};
use crate::{ERR_ACCESS, ERR_DATA, ERR_NOT_SUPPORTED};
use crate::{OP_ACK, RESPONSE_OK};
use async_trait::async_trait;
use ipnetwork::IpNetwork;
use log::{debug, error, trace, warn};
#[cfg(feature = "rpc")]
use serde::{Deserialize, Serialize};
use std::collections::{hash_map, HashMap, HashSet};
use std::fmt;
use std::marker::Unpin;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::atomic;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use std::time::Instant;
use submap::{AclMap, BroadcastMap, SubMap};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
#[cfg(not(target_os = "windows"))]
use tokio::net::{UnixListener, UnixStream};
#[cfg(feature = "rpc")]
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time;

#[cfg(feature = "rpc")]
use crate::rpc::{Rpc, RpcClient, RpcError, RpcEvent, RpcHandlers, RpcResult};
#[cfg(feature = "rpc")]
use serde_value::Value;

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

type BrokerClient = Arc<BusRtClient>;

macro_rules! make_confirm_channel {
    ($qos: expr) => {
        if $qos.needs_ack() {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let _r = tx.send(Ok(()));
            Ok(Some(rx))
        } else {
            Ok(None)
        }
    };
}

macro_rules! safe_send_frame {
    ($db: expr, $tgt: expr, $frame: expr, $timeout: expr) => {
        if $tgt.tx.is_full() {
            if $tgt.kind == BusRtClientKind::Internal {
                if let Some(timeout) = $timeout {
                    warn!(
                        "internal client {} queue is full, blocking for {:?}",
                        $tgt.name, timeout
                    );
                    time::timeout(timeout, $tgt.tx.send($frame))
                        .await?
                        .map_err(Into::into)
                } else {
                    warn!("internal client {} queue is full, blocking", $tgt.name);
                    $tgt.tx.send($frame).await.map_err(Into::into)
                }
            } else {
                warn!("client {} queue is full, force unregistering", $tgt.name);
                $db.unregister_client(&$tgt).await;
                $tgt.tx.close();
                Err(Error::not_delivered())
            }
        } else {
            $tgt.tx.send($frame).await.map_err(Into::into)
        }
    };
}

macro_rules! send {
    ($db:expr, $client:expr, $target:expr, $header: expr,
     $buf:expr, $payload_pos:expr, $len: expr, $realtime: expr, $timeout: expr) => {{
        $client.r_frames.fetch_add(1, atomic::Ordering::SeqCst);
        $client.r_bytes.fetch_add($len, atomic::Ordering::SeqCst);
        $db.r_frames.fetch_add(1, atomic::Ordering::SeqCst);
        $db.r_bytes.fetch_add($len, atomic::Ordering::SeqCst);
        trace!("bus/rt message from {} to {}", $client, $target);
        let client = {
            $db.clients.read().unwrap().get($target).map(|c| {
                c.w_frames.fetch_add(1, atomic::Ordering::SeqCst);
                c.w_bytes.fetch_add($len, atomic::Ordering::SeqCst);
                $db.w_frames.fetch_add(1, atomic::Ordering::SeqCst);
                $db.w_bytes.fetch_add($len, atomic::Ordering::SeqCst);
                c.clone()
            })
        };
        if let Some(client) = client {
            let frame = Arc::new(FrameData {
                kind: FrameKind::Message,
                sender: Some($client.name.clone()),
                topic: None,
                header: $header,
                buf: $buf,
                payload_pos: $payload_pos,
                realtime: $realtime,
            });
            safe_send_frame!($db, client, frame, $timeout)
        } else {
            Err(Error::not_registered())
        }
    }};
}

macro_rules! send_broadcast {
    ($db:expr, $client:expr, $target:expr, $header: expr,
     $buf:expr, $payload_pos:expr, $len: expr, $realtime: expr, $timeout: expr) => {{
        $client.r_frames.fetch_add(1, atomic::Ordering::SeqCst);
        $client.r_bytes.fetch_add($len, atomic::Ordering::SeqCst);
        $db.r_frames.fetch_add(1, atomic::Ordering::SeqCst);
        $db.r_bytes.fetch_add($len, atomic::Ordering::SeqCst);
        trace!("bus/rt broadcast message from {} to {}", $client, $target);
        #[allow(clippy::mutable_key_type)]
        let subs = { $db.broadcasts.read().unwrap().get_clients_by_mask($target) };
        if !subs.is_empty() {
            let frame = Arc::new(FrameData {
                kind: FrameKind::Broadcast,
                sender: Some($client.name.clone()),
                topic: None,
                header: $header,
                buf: $buf,
                payload_pos: $payload_pos,
                realtime: $realtime,
            });
            $db.w_frames
                .fetch_add(subs.len() as u64, atomic::Ordering::SeqCst);
            $db.w_bytes
                .fetch_add($len * subs.len() as u64, atomic::Ordering::SeqCst);
            for sub in subs {
                sub.w_frames.fetch_add(1, atomic::Ordering::SeqCst);
                sub.w_bytes.fetch_add($len, atomic::Ordering::SeqCst);
                let _r = safe_send_frame!($db, sub, frame.clone(), $timeout);
            }
        }
    }};
}

macro_rules! publish {
    ($db:expr, $client:expr, $topic:expr, $header: expr,
     $buf:expr, $payload_pos:expr, $len: expr, $realtime: expr, $timeout: expr) => {{
        $client.r_frames.fetch_add(1, atomic::Ordering::SeqCst);
        $client.r_bytes.fetch_add($len, atomic::Ordering::SeqCst);
        $db.r_frames.fetch_add(1, atomic::Ordering::SeqCst);
        $db.r_bytes.fetch_add($len, atomic::Ordering::SeqCst);
        trace!("bus/rt topic publish from {} to {}", $client, $topic);
        #[allow(clippy::mutable_key_type)]
        let subs = { $db.subscriptions.read().unwrap().get_subscribers($topic) };
        if !subs.is_empty() {
            let frame = Arc::new(FrameData {
                kind: FrameKind::Publish,
                sender: Some($client.name.clone()),
                topic: Some($topic.to_owned()),
                header: $header,
                buf: $buf,
                payload_pos: $payload_pos,
                realtime: $realtime,
            });
            $db.w_frames
                .fetch_add(subs.len() as u64, atomic::Ordering::SeqCst);
            $db.w_bytes
                .fetch_add($len * subs.len() as u64, atomic::Ordering::SeqCst);
            for sub in subs {
                sub.w_frames.fetch_add(1, atomic::Ordering::SeqCst);
                sub.w_bytes.fetch_add($len, atomic::Ordering::SeqCst);
                let _r = safe_send_frame!($db, sub, frame.clone(), $timeout);
            }
        }
    }};
}

pub struct Client {
    name: String,
    client: Arc<BusRtClient>,
    db: Arc<BrokerDb>,
    rx: Option<EventChannel>,
    secondary_counter: atomic::AtomicUsize,
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
    async fn subscribe_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<OpConfirm, Error> {
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
    async fn unsubscribe_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<OpConfirm, Error> {
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
        let len = payload.len() as u64;
        send!(
            self.db,
            self.client,
            target,
            None,
            payload.to_vec(),
            0,
            len,
            qos.is_realtime(),
            self.get_timeout()
        )?;
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
        let len = (payload.len() + header.len()) as u64;
        send!(
            self.db,
            self.client,
            target,
            Some(header.to_vec()),
            payload.to_vec(),
            0,
            len,
            qos.is_realtime(),
            self.get_timeout()
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
        let len = payload.len() as u64;
        send_broadcast!(
            self.db,
            self.client,
            target,
            None,
            payload.to_vec(),
            0,
            len,
            qos.is_realtime(),
            self.get_timeout()
        );
        make_confirm_channel!(qos)
    }
    #[inline]
    async fn publish(
        &mut self,
        topic: &str,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error> {
        let len = payload.len() as u64;
        publish!(
            self.db,
            self.client,
            topic,
            None,
            payload.to_vec(),
            0,
            len,
            qos.is_realtime(),
            self.get_timeout()
        );
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
    /// When an internal client is dropped, it is automatically dropped from the broker db, but no
    /// announce is sent. It is better to manually call "unregister" method before.
    #[inline]
    pub async fn unregister(&self) {
        self.client
            .registered
            .store(false, atomic::Ordering::SeqCst);
        self.db.unregister_client(&self.client).await;
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if self.client.registered.load(atomic::Ordering::SeqCst) {
            self.db.drop_client(&self.client);
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BusRtClientKind {
    Internal,
    LocalIpc,
    Tcp,
}

impl BusRtClientKind {
    #[allow(dead_code)]
    fn as_str(&self) -> &str {
        match self {
            BusRtClientKind::Internal => "internal",
            BusRtClientKind::LocalIpc => "local_ipc",
            BusRtClientKind::Tcp => "tcp",
        }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
struct BusRtClient {
    name: String,
    digest: submap::digest::Sha256Digest,
    primary_name: String,
    kind: BusRtClientKind,
    source: Option<String>,
    port: Option<String>,
    disconnect_trig: triggered::Trigger,
    tx: async_channel::Sender<Frame>,
    registered: atomic::AtomicBool,
    r_frames: atomic::AtomicU64,
    r_bytes: atomic::AtomicU64,
    w_frames: atomic::AtomicU64,
    w_bytes: atomic::AtomicU64,
    primary: bool,
    secondaries: parking_lot::Mutex<HashSet<String>>,
}

impl fmt::Display for BusRtClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl BusRtClient {
    pub fn new(
        name: &str,
        primary_name: &str,
        queue_size: usize,
        kind: BusRtClientKind,
        source: Option<String>,
        port: Option<String>,
    ) -> (Self, EventChannel, triggered::Listener) {
        let digest = submap::digest::sha256(name);
        let (tx, rx) = async_channel::bounded(queue_size);
        let primary = name == primary_name;
        let (disconnect_trig, disconnect_listener) = triggered::trigger();
        (
            Self {
                name: name.to_owned(),
                digest,
                primary_name: primary_name.to_owned(),
                kind,
                source,
                port,
                disconnect_trig,
                tx,
                registered: atomic::AtomicBool::new(false),
                r_frames: atomic::AtomicU64::new(0),
                r_bytes: atomic::AtomicU64::new(0),
                w_frames: atomic::AtomicU64::new(0),
                w_bytes: atomic::AtomicU64::new(0),
                primary,
                secondaries: <_>::default(),
            },
            rx,
            disconnect_listener,
        )
    }
}

impl PartialEq for BusRtClient {
    fn eq(&self, other: &Self) -> bool {
        self.digest == other.digest
    }
}

impl Ord for BusRtClient {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.digest.cmp(&other.digest)
    }
}

impl PartialOrd for BusRtClient {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for BusRtClient {}

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
    r_frames: atomic::AtomicU64,
    r_bytes: atomic::AtomicU64,
    w_frames: atomic::AtomicU64,
    w_bytes: atomic::AtomicU64,
    startup_time: Instant,
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
            r_frames: atomic::AtomicU64::new(0),
            r_bytes: atomic::AtomicU64::new(0),
            w_frames: atomic::AtomicU64::new(0),
            w_bytes: atomic::AtomicU64::new(0),
            startup_time: Instant::now(),
        }
    }
}

impl BrokerDb {
    fn stats(&self) -> BrokerStats {
        BrokerStats {
            uptime: self.startup_time.elapsed().as_secs(),
            r_frames: self.r_frames.load(atomic::Ordering::SeqCst),
            r_bytes: self.r_bytes.load(atomic::Ordering::SeqCst),
            w_frames: self.w_frames.load(atomic::Ordering::SeqCst),
            w_bytes: self.w_bytes.load(atomic::Ordering::SeqCst),
        }
    }
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
    #[inline]
    async fn register_client(&self, client: Arc<BusRtClient>) -> Result<(), Error> {
        #[cfg(feature = "rpc")]
        // copy name for the announce
        let name = client.name.clone();
        #[cfg(feature = "rpc")]
        let primary = client.primary;
        self.insert_client(client)?;
        #[cfg(feature = "rpc")]
        if primary {
            if let Err(e) = self.announce(BrokerEvent::reg(&name)).await {
                error!("{}", e);
            }
        }
        Ok(())
    }
    fn insert_client(&self, client: Arc<BusRtClient>) -> Result<(), Error> {
        let mut clients = self.clients.write().unwrap();
        let primary_client = if client.primary {
            None
        } else {
            Some(
                clients
                    .get_mut(&client.primary_name)
                    .map(|c| c.clone())
                    .ok_or_else(Error::not_registered)?,
            )
        };
        if let hash_map::Entry::Vacant(x) = clients.entry(client.name.clone()) {
            if let Some(pc) = primary_client {
                pc.secondaries.lock().insert(client.name.clone());
            }
            {
                let mut bdb = self.broadcasts.write().unwrap();
                bdb.register_client(&client.name, &client);
            }
            {
                let mut sdb = self.subscriptions.write().unwrap();
                sdb.register_client(&client);
                sdb.subscribe(BROKER_WARN_TOPIC, &client);
            }
            client.registered.store(true, atomic::Ordering::SeqCst);
            x.insert(client);
        } else {
            return Err(Error::busy(format!(
                "the client is already registred: {}",
                client.name
            )));
        }
        Ok(())
    }
    fn trigger_disconnect(&self, name: &str) -> Result<(), Error> {
        if let Some(client) = self.clients.read().unwrap().get(name) {
            if client.kind == BusRtClientKind::Internal {
                Err(Error::not_supported("the client is internal"))
            } else {
                client.disconnect_trig.trigger();
                Ok(())
            }
        } else {
            Err(Error::not_registered())
        }
    }
    #[inline]
    async fn unregister_client(&self, client: &Arc<BusRtClient>) {
        self.drop_client(client);
        #[cfg(feature = "rpc")]
        if client.primary {
            if let Err(e) = self.announce(BrokerEvent::unreg(&client.name)).await {
                error!("{}", e);
            }
        }
    }
    fn drop_client(&self, client: &Arc<BusRtClient>) {
        self.subscriptions
            .write()
            .unwrap()
            .unregister_client(client);
        self.broadcasts
            .write()
            .unwrap()
            .unregister_client(&client.name, client);
        self.clients.write().unwrap().remove(&client.name);
        if client.primary {
            let mut secondaries = client.secondaries.lock();
            for secondary in secondaries.iter() {
                let sec = self.clients.read().unwrap().get(secondary).cloned();
                if let Some(sec) = sec {
                    if sec.kind != BusRtClientKind::Internal {
                        sec.disconnect_trig.trigger();
                    }
                    self.drop_client(&sec);
                }
            }
            secondaries.clear();
        } else if let Some(primary) = self.clients.read().unwrap().get(&client.primary_name) {
            primary.secondaries.lock().remove(&client.name);
        }
    }
}

pub type AaaMap = Arc<parking_lot::Mutex<HashMap<String, ClientAaa>>>;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    buf_size: usize,
    buf_ttl: Duration,
    timeout: Duration,
    aaa_map: Option<AaaMap>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            buf_size: crate::DEFAULT_BUF_SIZE,
            buf_ttl: crate::DEFAULT_BUF_TTL,
            timeout: crate::DEFAULT_TIMEOUT,
            aaa_map: None,
        }
    }
}

impl ServerConfig {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
    #[inline]
    pub fn buf_size(mut self, size: usize) -> Self {
        self.buf_size = size;
        self
    }
    #[inline]
    pub fn buf_ttl(mut self, ttl: Duration) -> Self {
        self.buf_ttl = ttl;
        self
    }
    #[inline]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    #[inline]
    pub fn aaa_map(mut self, aaa_map: AaaMap) -> Self {
        self.aaa_map.replace(aaa_map);
        self
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct ClientAaa {
    hosts_allow: HashSet<IpNetwork>,
    allow_p2p_to: AclMap,
    allow_p2p_any: bool,
    allow_publish_to: AclMap,
    allow_publish_any: bool,
    allow_subscribe_to: AclMap,
    allow_subscribe_any: bool,
    allow_broadcast_to: AclMap,
    allow_broadcast_any: bool,
}

impl Default for ClientAaa {
    fn default() -> Self {
        let mut hosts_allow = HashSet::new();
        hosts_allow.insert(IpNetwork::V4("0.0.0.0/0".parse().unwrap()));
        Self {
            hosts_allow,
            allow_p2p_to: AclMap::new().separator('.').wildcard("*").match_any("?"),
            allow_p2p_any: true,
            allow_publish_to: AclMap::new().separator('/').wildcard("#").match_any("+"),
            allow_publish_any: true,
            allow_subscribe_to: AclMap::new().separator('/').wildcard("#").match_any("+"),
            allow_subscribe_any: true,
            allow_broadcast_to: AclMap::new().separator('.').wildcard("*").match_any("?"),
            allow_broadcast_any: true,
        }
    }
}

impl ClientAaa {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
    #[inline]
    pub fn hosts_allow(mut self, hosts: Vec<IpNetwork>) -> Self {
        self.hosts_allow = hosts.iter().copied().collect();
        self
    }
    /// peer masks as
    ///
    /// group.?.*
    /// group.subgroup.client
    /// group.?.client
    /// group.*
    #[inline]
    pub fn allow_p2p_to(mut self, peer_masks: &[&str]) -> Self {
        self.allow_p2p_any = false;
        for peer_mask in peer_masks {
            if *peer_mask == "*" {
                self.allow_p2p_any = true;
            }
            self.allow_p2p_to.insert(peer_mask);
        }
        self
    }
    #[inline]
    pub fn deny_p2p(mut self) -> Self {
        self.allow_p2p_any = false;
        self.allow_p2p_to = AclMap::new();
        self
    }
    /// topic masks as
    ///
    /// topic/+/#
    /// topic/subtopic/subsubtopic
    /// topic/+/subtopic
    /// topic/#
    #[inline]
    pub fn allow_publish_to(mut self, topic_masks: &[&str]) -> Self {
        self.allow_publish_any = false;
        for topic_mask in topic_masks {
            if *topic_mask == "#" {
                self.allow_publish_any = true;
            }
            self.allow_publish_to.insert(topic_mask);
        }
        self
    }
    #[inline]
    pub fn deny_publish(mut self) -> Self {
        self.allow_publish_any = false;
        self.allow_publish_to = AclMap::new();
        self
    }
    /// topic masks as
    ///
    /// topic/+/#
    /// topic/subtopic/subsubtopic
    /// topic/+/subtopic
    /// topic/#
    #[inline]
    pub fn allow_subscribe_to(mut self, topic_masks: &[&str]) -> Self {
        self.allow_subscribe_any = false;
        for topic_mask in topic_masks {
            if *topic_mask == "#" {
                self.allow_subscribe_any = true;
            }
            self.allow_subscribe_to.insert(topic_mask);
        }
        self
    }
    #[inline]
    pub fn deny_subscribe(mut self) -> Self {
        self.allow_subscribe_any = false;
        self.allow_subscribe_to = AclMap::new();
        self
    }
    /// peer masks as
    ///
    /// group.?.*
    /// group.subgroup.client
    /// group.?.client
    /// group.*
    #[inline]
    pub fn allow_broadcast_to(mut self, peer_masks: &[&str]) -> Self {
        self.allow_broadcast_any = false;
        for peer_mask in peer_masks {
            if *peer_mask == "*" {
                self.allow_broadcast_any = true;
            }
            self.allow_broadcast_to.insert(peer_mask);
        }
        self
    }
    #[inline]
    pub fn deny_broadcast(mut self) -> Self {
        self.allow_broadcast_any = false;
        self.allow_broadcast_to = AclMap::new();
        self
    }
    #[inline]
    fn connect_allowed(&self, addr: IpAddr) -> bool {
        for h in &self.hosts_allow {
            if h.contains(addr) {
                return true;
            }
        }
        false
    }
}

pub struct Broker {
    db: Arc<BrokerDb>,
    services: Vec<JoinHandle<()>>,
    queue_size: usize,
}

#[cfg(feature = "rpc")]
struct BrokerRpcHandlers {
    db: Arc<BrokerDb>,
}

#[cfg(feature = "rpc")]
const RPC_OK: [u8; 5] = [129, 162, 111, 107, 195];

#[cfg(feature = "rpc")]
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
            rmp_serde::from_slice(event.payload())?
        };
        match event.parse_method()? {
            "test" => {
                if !params.is_empty() {
                    return Err(RpcError::params(None));
                }
                Ok(Some(RPC_OK.to_vec()))
            }
            "info" => {
                if !params.is_empty() {
                    return Err(RpcError::params(None));
                }
                Ok(Some(rmp_serde::to_vec_named(&Broker::info())?))
            }
            "stats" => {
                if !params.is_empty() {
                    return Err(RpcError::params(None));
                }
                Ok(Some(rmp_serde::to_vec_named(&self.db.stats())?))
            }
            "client.list" => {
                if !params.is_empty() {
                    return Err(RpcError::params(None));
                }
                let db = self.db.clients.read().unwrap();
                let mut clients: Vec<ClientInfo> = db
                    .values()
                    .into_iter()
                    .filter(|c| c.primary)
                    .map(|v| ClientInfo {
                        name: &v.name,
                        kind: v.kind.as_str(),
                        source: v.source.as_deref(),
                        port: v.port.as_deref(),
                        r_frames: v.r_frames.load(atomic::Ordering::SeqCst),
                        r_bytes: v.r_bytes.load(atomic::Ordering::SeqCst),
                        w_frames: v.w_frames.load(atomic::Ordering::SeqCst),
                        w_bytes: v.w_bytes.load(atomic::Ordering::SeqCst),
                        queue: v.tx.len(),
                        instances: v.secondaries.lock().len() + 1,
                    })
                    .collect();
                clients.sort();
                Ok(Some(rmp_serde::to_vec_named(&ClientList { clients })?))
            }
            _ => Err(RpcError::method(None)),
        }
    }
    async fn handle_notification(&self, _event: RpcEvent) {}
    async fn handle_frame(&self, _frame: Frame) {}
}

#[cfg(not(target_os = "windows"))]
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
fn prepare_tcp_source(addr: &SocketAddr) -> Option<String> {
    Some(addr.to_string())
}

#[allow(clippy::unnecessary_wraps)]
fn prepare_unix_source(_addr: &tokio::net::unix::SocketAddr) -> Option<String> {
    None
}

macro_rules! spawn_server {
    ($self: expr, $path: expr, $listener: expr, $config: expr,
     $kind: expr, $prepare: ident, $prepare_source: ident) => {{
        let socket_path = $path.to_owned();
        let db = $self.db.clone();
        let queue_size = $self.queue_size;
        let service = tokio::spawn(async move {
            loop {
                match $listener.accept().await {
                    Ok((stream, addr)) => {
                        trace!("bus/rt client connected from {:?} to {}", addr, socket_path);
                        if let Err(e) = $prepare(&stream) {
                            error!("{}", e);
                            continue;
                        }
                        let (reader, writer) = stream.into_split();
                        let reader = BufReader::with_capacity($config.buf_size, reader);
                        let writer = TtlBufWriter::new(
                            writer,
                            $config.buf_size,
                            $config.buf_ttl,
                            $config.timeout,
                        );
                        let cdb = db.clone();
                        let name = socket_path.clone();
                        let client_source = $prepare_source(&addr);
                        let client_path = socket_path.clone();
                        let aaa_map = $config.aaa_map.clone();
                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_peer(PeerHandlerParams {
                                db: cdb,
                                reader,
                                writer,
                                timeout: $config.timeout,
                                aaa_map,
                                ip: addr.into(),
                                queue_size,
                                kind: $kind,
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
    W: AsyncWriteExt + Unpin + Send + Sync + 'static,
{
    db: Arc<BrokerDb>,
    reader: R,
    writer: TtlBufWriter<W>,
    timeout: Duration,
    aaa_map: Option<AaaMap>,
    ip: ClientIp,
    queue_size: usize,
    kind: BusRtClientKind,
    source: Option<String>,
    source_port: Option<String>,
}

enum ClientIp {
    No,
    Addr(IpAddr),
}

impl From<tokio::net::unix::SocketAddr> for ClientIp {
    fn from(_addr: tokio::net::unix::SocketAddr) -> Self {
        Self::No
    }
}

impl From<std::net::SocketAddr> for ClientIp {
    fn from(addr: std::net::SocketAddr) -> Self {
        Self::Addr(addr.ip())
    }
}

impl Default for Broker {
    fn default() -> Self {
        Self {
            db: <_>::default(),
            services: <_>::default(),
            queue_size: DEFAULT_QUEUE_SIZE,
        }
    }
}

impl Broker {
    pub fn new() -> Self {
        Self::default()
    }
    #[inline]
    pub fn stats(&self) -> BrokerStats {
        self.db.stats()
    }
    #[inline]
    pub fn info<'a>() -> BrokerInfo<'a> {
        BrokerInfo {
            author: crate::AUTHOR,
            version: crate::VERSION,
        }
    }
    #[cfg(feature = "rpc")]
    pub async fn init_default_core_rpc(&self) -> Result<(), Error> {
        let client = self.register_client(BROKER_NAME).await?;
        let handlers = BrokerRpcHandlers {
            db: self.db.clone(),
        };
        let rpc_client = RpcClient::new(client, handlers);
        self.set_core_rpc_client(rpc_client).await;
        Ok(())
    }
    #[inline]
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
        let client_primary_name = name
            .find(SECONDARY_SEP)
            .map_or_else(|| name, |pos| &name[..pos]);
        let (c, rx, _) = BusRtClient::new(
            name,
            client_primary_name,
            self.queue_size,
            BusRtClientKind::Internal,
            None,
            None,
        );
        let client = Arc::new(c);
        self.db.register_client(client.clone()).await?;
        Ok(Client {
            name: name.to_owned(),
            client,
            db: self.db.clone(),
            rx: Some(rx),
            secondary_counter: atomic::AtomicUsize::new(0),
        })
    }
    /// # Panics
    ///
    /// Will panic if the client's secondaries mutex is poisoned
    pub async fn register_secondary_for(&self, client: &Client) -> Result<Client, Error> {
        if client.client.primary {
            let secondary_id = client
                .secondary_counter
                .fetch_add(1, atomic::Ordering::SeqCst);
            let secondary_name = format!("{}{}{}", client.client.name, SECONDARY_SEP, secondary_id);
            self.register_client(&secondary_name).await
        } else {
            Err(Error::not_supported("not a primary client"))
        }
    }
    #[inline]
    pub async fn unregister_client(&self, client: &Client) {
        self.db.unregister_client(&client.client).await;
    }
    #[inline]
    /// Force disconnect a client
    ///
    /// Errors
    ///
    /// NotSupported - if attempted to disconnect an internal client
    /// NotRegistered - if a client is not registered (may be usually safely omitted)
    pub fn force_disconnect(&self, name: &str) -> Result<(), Error> {
        self.db.trigger_disconnect(name)
    }
    #[cfg(not(target_os = "windows"))]
    pub async fn spawn_unix_server(
        &mut self,
        path: &str,
        config: ServerConfig,
    ) -> Result<(), Error> {
        let _r = tokio::fs::remove_file(path).await;
        let listener = UnixListener::bind(path)?;
        spawn_server!(
            self,
            path,
            listener,
            config,
            BusRtClientKind::LocalIpc,
            prepare_unix_stream,
            prepare_unix_source
        );
        Ok(())
    }
    pub async fn spawn_tcp_server(
        &mut self,
        path: &str,
        config: ServerConfig,
    ) -> Result<(), Error> {
        let listener = TcpListener::bind(path).await?;
        spawn_server!(
            self,
            path,
            listener,
            config,
            BusRtClientKind::Tcp,
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
    /// Requires rpc feature + broker core rpc client to be set
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
                    QoS::No,
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
        W: AsyncWriteExt + Unpin + Send + Sync + 'static,
    {
        let timeout = params.timeout;
        let mut reader = params.reader;
        let mut writer = params.writer;
        let queue_size = params.queue_size;
        let db = params.db;
        macro_rules! write_and_flush {
            ($buf: expr) => {
                time::timeout(timeout, writer.write($buf, Flush::Instant)).await??;
            };
        }
        let mut buf = GREETINGS.to_vec();
        buf.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        write_and_flush!(&buf);
        let mut buf = [0u8; 3];
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
        let mut buf = [0u8; 2];
        time::timeout(timeout, reader.read_exact(&mut buf)).await??;
        let len = u16::from_le_bytes(buf);
        let mut buf = vec![0; len as usize];
        time::timeout(timeout, reader.read_exact(&mut buf)).await??;
        let client_name = std::str::from_utf8(&buf)?.to_owned();
        if client_name.is_empty() || client_name.starts_with('.') {
            write_and_flush!(&[ERR_DATA]);
            return Err(Error::data(format!("Invalid client name: {}", client_name)));
        }
        let client_primary_name = client_name
            .find(SECONDARY_SEP)
            .map_or_else(|| client_name.as_str(), |pos| &client_name[..pos]);
        let aaa = if let Some(aaa_map) = params.aaa_map {
            let aaa = aaa_map.lock().get(client_primary_name).cloned();
            if let Some(ref a) = aaa {
                if let ClientIp::Addr(addr) = params.ip {
                    if !a.connect_allowed(addr) {
                        write_and_flush!(&[ERR_ACCESS]);
                        return Err(Error::access(format!(
                            "Client {} is not allowed to connect from {}",
                            client_name, addr
                        )));
                    }
                }
            } else {
                write_and_flush!(&[ERR_ACCESS]);
                return Err(Error::access(format!(
                    "Client not in AAA map: {}",
                    client_name
                )));
            }
            aaa
        } else {
            None
        };
        let (client, rx, disconnect_listener) = {
            let (c, rx, disconnect_listener) = BusRtClient::new(
                &client_name,
                client_primary_name,
                queue_size,
                params.kind,
                params.source,
                params.source_port,
            );
            let client = Arc::new(c);
            if let Err(e) = db.register_client(client.clone()).await {
                write_and_flush!(&[e.kind as u8]);
                return Err(e);
            }
            write_and_flush!(&[RESPONSE_OK]);
            (client, rx, disconnect_listener)
        };
        debug!("bus/rt client registered: {}", client_name);
        let pinger_fut = Self::handle_pinger(&client_name, client.tx.clone(), timeout);
        let reader_fut = Self::handle_reader(&db, client.clone(), &mut reader, timeout, aaa);
        let writer_fut = Self::handle_writer(rx, &mut writer, timeout);
        macro_rules! finish_peer {
            () => {
                db.unregister_client(&client).await;
                debug!("bus/rt client disconnected: {}", client_name);
            };
        }
        tokio::select! {
            result = reader_fut => {
                finish_peer!();
                result
            }
            result = writer_fut => {
                finish_peer!();
                result
            }
            result = pinger_fut => {
                finish_peer!();
                result
            }
            _ = disconnect_listener => {
                debug!("disconnected by the broker: {}", client_name);
                finish_peer!();
                Ok(())
            }
        }
    }

    async fn handle_pinger(
        client_name: &str,
        tx: async_channel::Sender<Frame>,
        timeout: Duration,
    ) -> Result<(), Error> {
        loop {
            time::sleep(timeout).await;
            if tx.is_full() {
                warn!("client {} queue is full, force unregistering", client_name);
                return Err(Error::io("client queue overflow"));
            }
            tx.send(Arc::new(FrameData::new_nop())).await?;
        }
    }

    // TODO send ack only after the client received message (QoS2)
    #[allow(clippy::too_many_lines)]
    async fn handle_reader<R>(
        db: &BrokerDb,
        client: Arc<BusRtClient>,
        reader: &mut R,
        timeout: Duration,
        aaa: Option<ClientAaa>,
    ) -> Result<(), Error>
    where
        R: AsyncReadExt + Unpin,
    {
        loop {
            let mut header_buf = [0u8; 9];
            let r_len = reader.read(&mut header_buf).await?;
            if r_len == 0 {
                return Ok(());
            } else if r_len < 9 {
                time::timeout(timeout, reader.read_exact(&mut header_buf[r_len..])).await??;
            }
            let flags = header_buf[4];
            if flags == 0 {
                // OP_NOP
                trace!("{} ping", client);
                continue;
            }
            let op_id = &header_buf[0..4];
            let op: FrameOp = (flags & 0b0011_1111).try_into()?;
            let qos: QoS = (flags >> 6 & 0b0011_1111).try_into()?;
            let len = u32::from_le_bytes(header_buf[5..9].try_into().unwrap());
            let mut buf = vec![0; len as usize];
            time::timeout(timeout, reader.read_exact(&mut buf)).await??;
            macro_rules! send_ack {
                ($code:expr, $realtime: expr) => {
                    let mut buf = Vec::with_capacity(6);
                    buf.push(OP_ACK);
                    buf.extend_from_slice(op_id);
                    buf.push($code);
                    client.w_frames.fetch_add(1, atomic::Ordering::SeqCst);
                    client
                        .w_bytes
                        .fetch_add(buf.len() as u64, atomic::Ordering::SeqCst);
                    db.w_frames.fetch_add(1, atomic::Ordering::SeqCst);
                    db.w_bytes
                        .fetch_add(buf.len() as u64, atomic::Ordering::SeqCst);
                    client
                        .tx
                        .send(Arc::new(FrameData {
                            kind: FrameKind::Prepared,
                            sender: None,
                            topic: None,
                            header: None,
                            buf,
                            payload_pos: 0,
                            realtime: $realtime,
                        }))
                        .await?;
                };
            }
            match op {
                FrameOp::SubscribeTopic => {
                    client.r_frames.fetch_add(1, atomic::Ordering::SeqCst);
                    client
                        .r_bytes
                        .fetch_add(u64::from(len), atomic::Ordering::SeqCst);
                    db.r_frames.fetch_add(1, atomic::Ordering::SeqCst);
                    db.r_bytes
                        .fetch_add(u64::from(len), atomic::Ordering::SeqCst);
                    let sp = buf.split(|c| *c == 0);
                    let mut topics = Vec::new();
                    for t in sp {
                        let topic = std::str::from_utf8(t)?;
                        let allowed = if let Some(ref aaa) = aaa {
                            aaa.allow_subscribe_any || aaa.allow_subscribe_to.matches(topic)
                        } else {
                            true
                        };
                        if allowed {
                            topics.push(topic);
                        } else if qos.needs_ack() {
                            send_ack!(ERR_ACCESS, qos.is_realtime());
                            continue;
                        } else {
                            continue;
                        }
                    }
                    {
                        let mut sdb = db.subscriptions.write().unwrap();
                        for t in topics {
                            sdb.subscribe(t, &client);
                            trace!("bus/rt client {} subscribed to topic {}", client, t);
                        }
                    }
                    if qos.needs_ack() {
                        send_ack!(RESPONSE_OK, qos.is_realtime());
                    }
                }
                FrameOp::UnsubscribeTopic => {
                    client.r_frames.fetch_add(1, atomic::Ordering::SeqCst);
                    client
                        .r_bytes
                        .fetch_add(u64::from(len), atomic::Ordering::SeqCst);
                    db.r_frames.fetch_add(1, atomic::Ordering::SeqCst);
                    db.r_bytes
                        .fetch_add(u64::from(len), atomic::Ordering::SeqCst);
                    let sp = buf.split(|c| *c == 0);
                    {
                        let mut sdb = db.subscriptions.write().unwrap();
                        for t in sp {
                            let topic = std::str::from_utf8(t)?;
                            sdb.unsubscribe(topic, &client);
                            trace!("bus/rt client {} unsubscribed from topic {}", client, topic);
                        }
                    }
                    if qos.needs_ack() {
                        send_ack!(RESPONSE_OK, qos.is_realtime());
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
                            let len = buf.len() as u64;
                            let realtime = qos.is_realtime();
                            let allowed = if let Some(ref aaa) = aaa {
                                aaa.allow_p2p_any || aaa.allow_p2p_to.matches(target)
                            } else {
                                true
                            };
                            if allowed {
                                if let Err(e) = send!(
                                    db,
                                    client,
                                    target,
                                    None,
                                    buf,
                                    payload_pos,
                                    len,
                                    realtime,
                                    Some(timeout)
                                ) {
                                    if qos.needs_ack() {
                                        send_ack!(e.kind as u8, realtime);
                                    }
                                } else if qos.needs_ack() {
                                    send_ack!(RESPONSE_OK, realtime);
                                }
                            } else if qos.needs_ack() {
                                send_ack!(ERR_ACCESS, qos.is_realtime());
                            }
                        }
                        FrameOp::Broadcast => {
                            let allowed = if let Some(ref aaa) = aaa {
                                aaa.allow_broadcast_any || aaa.allow_broadcast_to.matches(target)
                            } else {
                                true
                            };
                            if allowed {
                                let len = buf.len() as u64;
                                let realtime = qos.is_realtime();
                                send_broadcast!(
                                    db,
                                    client,
                                    target,
                                    None,
                                    buf,
                                    payload_pos,
                                    len,
                                    realtime,
                                    Some(timeout)
                                );
                                if qos.needs_ack() {
                                    send_ack!(RESPONSE_OK, realtime);
                                }
                            } else if qos.needs_ack() {
                                send_ack!(ERR_ACCESS, qos.is_realtime());
                            }
                        }
                        FrameOp::PublishTopic => {
                            let allowed = if let Some(ref aaa) = aaa {
                                aaa.allow_publish_any || aaa.allow_publish_to.matches(target)
                            } else {
                                true
                            };
                            if allowed {
                                let len = buf.len() as u64;
                                let realtime = qos.is_realtime();
                                publish!(
                                    db,
                                    client,
                                    target,
                                    None,
                                    buf,
                                    payload_pos,
                                    len,
                                    realtime,
                                    Some(timeout)
                                );
                                if qos.needs_ack() {
                                    send_ack!(RESPONSE_OK, realtime);
                                }
                            } else if qos.needs_ack() {
                                send_ack!(ERR_ACCESS, qos.is_realtime());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    async fn handle_writer<W>(
        rx: EventChannel,
        writer: &mut TtlBufWriter<W>,
        timeout: Duration,
    ) -> Result<(), Error>
    where
        W: AsyncWriteExt + Unpin + Send + Sync + 'static,
    {
        while let Ok(frame) = rx.recv().await {
            macro_rules! write_data {
                ($data: expr, $flush: expr) => {
                    time::timeout(timeout, writer.write($data, $flush)).await??;
                };
            }
            if frame.kind == FrameKind::Prepared {
                write_data!(&frame.buf, frame.realtime.into());
            } else {
                let sender = frame.sender.as_ref().map(String::as_bytes);
                let topic = frame.topic.as_ref().map(String::as_bytes);
                #[allow(clippy::redundant_closure_for_method_calls)]
                let mut extra_len = sender.map_or(0, |v| v.len() + 1);
                if let Some(t) = topic.as_ref() {
                    extra_len += t.len() + 1;
                }
                if let Some(header) = frame.header.as_ref() {
                    extra_len += header.len();
                }
                let mut buf = Vec::with_capacity(6 + extra_len);
                buf.push(frame.kind as u8); // byte 0
                let frame_len = extra_len + frame.buf.len() - frame.payload_pos;
                #[allow(clippy::cast_possible_truncation)]
                buf.extend_from_slice(&(frame_len as u32).to_le_bytes()); // bytes 1-4
                buf.push(if frame.realtime { 1 } else { 0 }); // byte 5 - reserved
                if let Some(s) = sender {
                    buf.extend_from_slice(s);
                    buf.push(0x00);
                }
                if let Some(t) = topic.as_ref() {
                    buf.extend_from_slice(t);
                    buf.push(0x00);
                };
                write_data!(&buf, Flush::No);
                if let Some(header) = frame.header() {
                    write_data!(header, Flush::No);
                }
                write_data!(frame.payload(), frame.realtime.into());
            }
        }
        Ok(())
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        for service in &self.services {
            service.abort();
        }
    }
}
