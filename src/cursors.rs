use crate::rpc::RpcError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use uuid::Uuid;

/// A helper cursor payload structure which implements serialize/deserialize with serde Can be
/// replaced with a custom one
#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct Payload {
    u: uuid::Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    n: Option<usize>,
}

impl From<Uuid> for Payload {
    #[inline]
    fn from(u: Uuid) -> Self {
        Self { u, n: None }
    }
}

impl Payload {
    #[inline]
    pub fn uuid(&self) -> &Uuid {
        &self.u
    }
    #[inline]
    pub fn bulk_number(&self) -> usize {
        self.n.unwrap_or(1)
    }
    #[inline]
    pub fn set_bulk_number(&mut self, n: usize) {
        self.n = Some(n);
    }
    #[inline]
    pub fn clear_bulk_number(&mut self) {
        self.n = None;
    }
}

/// A helper map to handle multiple cursors
pub struct Map {
    data: Arc<RwLock<BTreeMap<uuid::Uuid, Box<dyn Cursor + Send + Sync>>>>,
    cleaner: Option<JoinHandle<()>>,
}

impl Map {
    /// creates cursor map object, cleaner task is automatically spawned
    pub fn new(cleaner_interval: Duration) -> Self {
        let mut map = Self {
            data: <_>::default(),
            cleaner: <_>::default(),
        };
        let cleaner = map.spawn_cleaner(cleaner_interval);
        map.cleaner.replace(cleaner);
        map
    }
    /// Add a new cursor to the map and return its UUID
    pub async fn add<C>(&self, c: C) -> Uuid
    where
        C: Cursor + Send + Sync + 'static,
    {
        let u = Uuid::new_v4();
        self.data.write().await.insert(u, Box::new(c));
        u
    }
    /// Remove a cursor from the map
    ///
    /// (usually should not be called, unless there is no cleaner worker spawned)
    pub async fn remove<C>(&self, u: &Uuid) {
        self.data.write().await.remove(u);
    }
    /// Call "next" method of the cursor, specified by UUID
    pub async fn next(&self, cursor_id: &Uuid) -> Result<Option<Vec<u8>>, RpcError> {
        if let Some(cursor) = self.data.read().await.get(cursor_id) {
            Ok(cursor.next().await?)
        } else {
            Err(RpcError::not_found(None))
        }
    }
    /// Call "next_bulk" method of the cursor, specified by UUID
    pub async fn next_bulk(
        &self,
        cursor_id: &Uuid,
        count: usize,
    ) -> Result<Option<Vec<u8>>, RpcError> {
        if let Some(cursor) = self.data.read().await.get(cursor_id) {
            Ok(Some(cursor.next_bulk(count).await?))
        } else {
            Err(RpcError::not_found(None))
        }
    }
    fn spawn_cleaner(&self, interval: Duration) -> JoinHandle<()> {
        let cursors = self.data.clone();
        let mut int = tokio::time::interval(interval);
        tokio::spawn(async move {
            loop {
                int.tick().await;
                cursors.write().await.retain(|_, v| v.meta().is_alive());
            }
        })
    }
}

impl Drop for Map {
    fn drop(&mut self) {
        if let Some(cleaner) = self.cleaner.take() {
            cleaner.abort();
        }
    }
}

/// The cursor trait
#[async_trait]
pub trait Cursor {
    async fn next(&self) -> Result<Option<Vec<u8>>, RpcError>;
    async fn next_bulk(&self, count: usize) -> Result<Vec<u8>, RpcError>;
    fn meta(&self) -> &Meta;
}

/// The cursor meta object, used by cursors::Map to manage finished/expired cursors
pub struct Meta {
    finished: atomic::AtomicBool,
    expires: Instant,
}

impl Meta {
    #[inline]
    pub fn new(ttl: Duration) -> Self {
        Self {
            expires: Instant::now() + ttl,
            finished: <_>::default(),
        }
    }
    #[inline]
    pub fn is_finished(&self) -> bool {
        self.finished.load(atomic::Ordering::SeqCst)
    }
    #[inline]
    pub fn is_expired(&self) -> bool {
        self.expires < Instant::now()
    }
    #[inline]
    pub fn mark_finished(&self) {
        self.finished.store(true, atomic::Ordering::SeqCst);
    }
    #[inline]
    pub fn is_alive(&self) -> bool {
        !self.is_finished() && !self.is_expired()
    }
}
