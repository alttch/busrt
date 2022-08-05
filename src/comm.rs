use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Flush {
    No,
    Scheduled,
    Instant,
}

impl From<bool> for Flush {
    #[inline]
    fn from(realtime: bool) -> Self {
        if realtime {
            Flush::Instant
        } else {
            Flush::Scheduled
        }
    }
}

pub struct TtlBufWriter<W> {
    writer: Arc<Mutex<BufWriter<W>>>,
    tx: async_channel::Sender<()>,
    dtx: Option<oneshot::Sender<()>>,
    flusher: JoinHandle<()>,
}

impl<W> TtlBufWriter<W>
where
    W: AsyncWriteExt + Unpin + Send + Sync + 'static,
{
    pub fn new(writer: W, cap: usize, ttl: Duration, timeout: Duration) -> Self {
        let writer = Arc::new(Mutex::new(BufWriter::with_capacity(cap, writer)));
        let wf = writer.clone();
        let (tx, rx) = async_channel::bounded::<()>(1);
        // flusher future
        let flusher = tokio::spawn(async move {
            while rx.recv().await.is_ok() {
                async_io::Timer::after(ttl).await;
                if let Ok(mut writer) = tokio::time::timeout(timeout, wf.lock()).await {
                    let _r = tokio::time::timeout(timeout, writer.flush()).await;
                }
            }
        });
        let (dtx, drx) = oneshot::channel();
        let wf = writer.clone();
        // this future works on drop
        tokio::spawn(async move {
            let _r = drx.await;
            let mut writer = wf.lock().await;
            let _r = tokio::time::timeout(timeout, writer.flush()).await;
        });
        Self {
            writer,
            tx,
            dtx: Some(dtx),
            flusher,
        }
    }
    #[inline]
    pub async fn write(&mut self, buf: &[u8], flush: Flush) -> std::io::Result<()> {
        let mut writer = self.writer.lock().await;
        let result = writer.write_all(buf).await;
        if flush == Flush::Instant {
            writer.flush().await?;
        } else if flush == Flush::Scheduled && self.tx.is_empty() {
            let _ = self.tx.send(()).await;
        }
        result
    }
}

impl<W> Drop for TtlBufWriter<W> {
    fn drop(&mut self) {
        self.flusher.abort();
        let _ = self.dtx.take().unwrap().send(());
    }
}
