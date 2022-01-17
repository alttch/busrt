use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::Mutex;

pub struct TtlBufWriter<W> {
    writer: Arc<Mutex<BufWriter<W>>>,
    tx: async_channel::Sender<bool>,
}

impl<W> TtlBufWriter<W>
where
    W: AsyncWriteExt + Unpin + Send + Sync + 'static,
{
    pub fn new(writer: W, cap: usize, ttl: Duration, timeout: Duration) -> Self {
        let writer = Arc::new(Mutex::new(BufWriter::with_capacity(cap, writer)));
        let wf = writer.clone();
        let (tx, rx) = async_channel::bounded::<bool>(1);
        tokio::spawn(async move {
            while let Ok(x) = rx.recv().await {
                tokio::time::sleep(ttl).await;
                let mut writer = wf.lock().await;
                let _r = tokio::time::timeout(timeout, writer.flush()).await;
                if !x {
                    break;
                }
            }
        });
        Self { writer, tx }
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
        let _ = self.tx.try_send(false);
    }
}
