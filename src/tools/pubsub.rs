use crate::{Error, Frame};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

pub struct Publication {
    subtopic_pos: usize,
    frame: Frame,
}

impl Publication {
    #[inline]
    pub fn frame(&self) -> &Frame {
        &self.frame
    }
    #[inline]
    pub fn sender(&self) -> &str {
        self.frame.sender()
    }
    /// # Panics
    ///
    /// Will not panic as all processed frames always have topics
    #[inline]
    pub fn topic(&self) -> &str {
        self.frame.topic().unwrap()
    }
    /// # Panics
    ///
    /// Will not panic as all processed frames always have topics
    #[inline]
    pub fn subtopic(&self) -> &str {
        &self.frame.topic().as_ref().unwrap()[self.subtopic_pos..]
    }
    #[inline]
    pub fn payload(&self) -> &[u8] {
        self.frame.payload()
    }
    #[inline]
    pub fn header(&self) -> Option<&[u8]> {
        self.frame.header()
    }
    #[inline]
    pub fn is_realtime(&self) -> bool {
        self.frame.is_realtime()
    }
}

/// Topic publications broker
///
/// The helper class to process topics in blocking mode
///
/// Processes topics and sends frames to handler channels
#[derive(Default)]
pub struct TopicBroker {
    prefixes: BTreeMap<String, async_channel::Sender<Publication>>,
    topics: BTreeMap<String, async_channel::Sender<Publication>>,
}

impl TopicBroker {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
    /// Process a topic (returns tx, rx channel for a handler)
    #[inline]
    pub fn register_topic(
        &mut self,
        topic: &str,
    ) -> Result<
        (
            async_channel::Sender<Publication>,
            async_channel::Receiver<Publication>,
        ),
        Error,
    > {
        let (tx, rx) = async_channel::unbounded();
        self.register_topic_tx(topic, tx.clone())?;
        Ok((tx, rx))
    }
    /// Process a topic with the pre-defined channel
    #[inline]
    pub fn register_topic_tx(
        &mut self,
        topic: &str,
        tx: async_channel::Sender<Publication>,
    ) -> Result<(), Error> {
        if let Entry::Vacant(o) = self.topics.entry(topic.to_owned()) {
            o.insert(tx);
            Ok(())
        } else {
            Err(Error::busy("topic already registered"))
        }
    }
    /// Process subtopic by prefix (returns tx, rx channel for a handler)
    #[inline]
    pub fn register_prefix(
        &mut self,
        prefix: &str,
    ) -> Result<
        (
            async_channel::Sender<Publication>,
            async_channel::Receiver<Publication>,
        ),
        Error,
    > {
        let (tx, rx) = async_channel::unbounded();
        self.register_prefix_tx(prefix, tx.clone())?;
        Ok((tx, rx))
    }
    /// Process subtopic by prefix with the pre-defined channel
    #[inline]
    pub fn register_prefix_tx(
        &mut self,
        prefix: &str,
        tx: async_channel::Sender<Publication>,
    ) -> Result<(), Error> {
        if let Entry::Vacant(o) = self.prefixes.entry(prefix.to_owned()) {
            o.insert(tx);
            Ok(())
        } else {
            Err(Error::busy("topic prefix already registered"))
        }
    }
    /// The frame is returned back if not processed
    #[inline]
    pub async fn process(&self, frame: Frame) -> Result<Option<Frame>, Error> {
        if let Some(topic) = frame.topic() {
            if let Some(tx) = self.topics.get(topic) {
                tx.send(Publication {
                    subtopic_pos: 0,
                    frame,
                })
                .await?;
                return Ok(None);
            }
            for (pfx, tx) in &self.prefixes {
                if topic.starts_with(pfx) {
                    tx.send(Publication {
                        subtopic_pos: pfx.len(),
                        frame,
                    })
                    .await?;
                    return Ok(None);
                }
            }
        }
        Ok(Some(frame))
    }
}
