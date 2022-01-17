use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

pub struct TtlBufWriter<W> {
    writer: Arc<Mutex<BufWriter<W>>>,
    tx: async_channel::Sender<bool>,
    dtx: Option<oneshot::Sender<bool>>,
    flusher: JoinHandle<()>,
}

impl<W> TtlBufWriter<W>
where
    W: AsyncWriteExt + Unpin + Send + Sync + 'static,
{
    pub fn new(writer: W, cap: usize, ttl: Duration, timeout: Duration) -> Self {
        let writer = Arc::new(Mutex::new(BufWriter::with_capacity(cap, writer)));
        let wf = writer.clone();
        let (tx, rx) = async_channel::bounded::<bool>(1);
        let flusher = tokio::spawn(async move {
            while let Ok(_) = rx.recv().await {
                tokio::time::sleep(ttl).await;
                let mut writer = tokio::time::timeout(timeout, wf.lock()).await.unwrap();
                let _r = tokio::time::timeout(timeout, writer.flush()).await;
            }
        });
        let (dtx, drx) = oneshot::channel();
        let wf = writer.clone();
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
    pub async fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        let result = self.writer.lock().await.write_all(buf).await;
        if self.tx.is_empty() {
            let _ = self.tx.send(true).await;
        }
        result
    }
    pub async fn flush(&mut self) -> std::io::Result<()> {
        self.writer.lock().await.flush().await
    }
}

impl<W> Drop for TtlBufWriter<W> {
    fn drop(&mut self) {
        self.flusher.abort();
        self.dtx.take().unwrap().send(true).unwrap();
    }
}
