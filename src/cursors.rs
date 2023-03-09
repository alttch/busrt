use crate::rpc::RpcError;
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::atomic;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use uuid::Uuid;

#[derive(Default)]
pub struct Map(pub Arc<RwLock<BTreeMap<uuid::Uuid, Box<dyn Cursor + Send + Sync>>>>);

impl Map {
    pub async fn add<C>(&self, c: C) -> Uuid
    where
        C: Cursor + Send + Sync + 'static,
    {
        let u = Uuid::new_v4();
        self.0.write().await.insert(u, Box::new(c));
        u
    }
    pub async fn next(&self, cursor_id: &Uuid) -> Result<Option<Vec<u8>>, RpcError> {
        if let Some(cursor) = self.0.read().await.get(cursor_id) {
            Ok(cursor.next().await?)
        } else {
            Err(RpcError::not_found(None))
        }
    }
    pub async fn next_bulk(
        &self,
        cursor_id: &Uuid,
        count: usize,
    ) -> Result<Option<Vec<u8>>, RpcError> {
        if let Some(cursor) = self.0.read().await.get(cursor_id) {
            Ok(Some(cursor.next_bulk(count).await?))
        } else {
            Err(RpcError::not_found(None))
        }
    }
    pub fn spawn_cleaner(&self, interval: Duration) -> JoinHandle<()> {
        let cursors = self.0.clone();
        let mut int = tokio::time::interval(interval);
        tokio::spawn(async move {
            loop {
                int.tick().await;
                cursors.write().await.retain(|_, v| v.meta().is_alive());
            }
        })
    }
}

#[async_trait]
pub trait Cursor {
    async fn next(&self) -> Result<Option<Vec<u8>>, RpcError>;
    async fn next_bulk(&self, count: usize) -> Result<Vec<u8>, RpcError>;
    fn meta(&self) -> &Meta;
}

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
    pub fn is_alive(&self) -> bool {
        !self.is_finished() && !self.is_expired()
    }
}
