use crate::{Error, Frame};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

pub type PublicationSender = async_channel::Sender<Publication>;
pub type PublicationReceiver = async_channel::Receiver<Publication>;

pub struct Publication {
    subtopic_pos: usize,
    frame: Frame,
    handler_id: usize,
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
    #[inline]
    pub fn primary_sender(&self) -> &str {
        self.frame.primary_sender()
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
    #[inline]
    pub fn handler_id(&self) -> usize {
        self.handler_id
    }
}

/// Topic publications broker
///
/// The helper class to process topics in blocking mode
///
/// Processes topics and sends frames to handler channels
#[derive(Default)]
pub struct TopicBroker {
    prefixes: BTreeMap<String, (PublicationSender, usize)>,
    topics: BTreeMap<String, (PublicationSender, usize)>,
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
        channel_size: usize,
    ) -> Result<(PublicationSender, PublicationReceiver), Error> {
        let (tx, rx) = async_channel::bounded(channel_size);
        self.register_topic_tx(topic, tx.clone())?;
        Ok((tx, rx))
    }
    /// Process a topic (returns tx, rx channel for a handler)
    ///
    /// handler id - a custom handler id (to use in multi-handlers)
    #[inline]
    pub fn register_topic_with_handler_id(
        &mut self,
        topic: &str,
        handler_id: usize,
        channel_size: usize,
    ) -> Result<(PublicationSender, PublicationReceiver), Error> {
        let (tx, rx) = async_channel::bounded(channel_size);
        self.register_topic_tx_with_handler_id(topic, handler_id, tx.clone())?;
        Ok((tx, rx))
    }
    /// Process a topic with the pre-defined channel
    #[inline]
    pub fn register_topic_tx(&mut self, topic: &str, tx: PublicationSender) -> Result<(), Error> {
        if let Entry::Vacant(o) = self.topics.entry(topic.to_owned()) {
            o.insert((tx, 0));
            Ok(())
        } else {
            Err(Error::busy("topic already registered"))
        }
    }
    /// Process a topic with the pre-defined channel
    ///
    /// handler id - a custom handler id (to use in multi-handlers)
    #[inline]
    pub fn register_topic_tx_with_handler_id(
        &mut self,
        topic: &str,
        handler_id: usize,
        tx: PublicationSender,
    ) -> Result<(), Error> {
        if let Entry::Vacant(o) = self.topics.entry(topic.to_owned()) {
            o.insert((tx, handler_id));
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
        channel_size: usize,
    ) -> Result<(PublicationSender, PublicationReceiver), Error> {
        let (tx, rx) = async_channel::bounded(channel_size);
        self.register_prefix_tx(prefix, tx.clone())?;
        Ok((tx, rx))
    }
    /// Process subtopic by prefix (returns tx, rx channel for a handler)
    ///
    /// handler id - a custom handler id (to use in multi-handlers)
    #[inline]
    pub fn register_prefix_with_handler_id(
        &mut self,
        prefix: &str,
        handler_id: usize,
        channel_size: usize,
    ) -> Result<(PublicationSender, PublicationReceiver), Error> {
        let (tx, rx) = async_channel::bounded(channel_size);
        self.register_prefix_tx_with_handler_id(prefix, handler_id, tx.clone())?;
        Ok((tx, rx))
    }
    /// Process subtopic by prefix with the pre-defined channel
    #[inline]
    pub fn register_prefix_tx(&mut self, prefix: &str, tx: PublicationSender) -> Result<(), Error> {
        if let Entry::Vacant(o) = self.prefixes.entry(prefix.to_owned()) {
            o.insert((tx, 0));
            Ok(())
        } else {
            Err(Error::busy("topic prefix already registered"))
        }
    }
    /// Process subtopic by prefix with the pre-defined channel
    #[inline]
    pub fn register_prefix_tx_with_handler_id(
        &mut self,
        prefix: &str,
        handler_id: usize,
        tx: PublicationSender,
    ) -> Result<(), Error> {
        if let Entry::Vacant(o) = self.prefixes.entry(prefix.to_owned()) {
            o.insert((tx, handler_id));
            Ok(())
        } else {
            Err(Error::busy("topic prefix already registered"))
        }
    }
    /// The frame is returned back if not processed
    #[inline]
    pub async fn process(&self, frame: Frame) -> Result<Option<Frame>, Error> {
        if let Some(topic) = frame.topic() {
            if let Some((tx, handler_id)) = self.topics.get(topic) {
                tx.send(Publication {
                    subtopic_pos: 0,
                    frame,
                    handler_id: *handler_id,
                })
                .await?;
                return Ok(None);
            }
            for (pfx, (tx, handler_id)) in &self.prefixes {
                if topic.starts_with(pfx) {
                    tx.send(Publication {
                        subtopic_pos: pfx.len(),
                        frame,
                        handler_id: *handler_id,
                    })
                    .await?;
                    return Ok(None);
                }
            }
        }
        Ok(Some(frame))
    }
}
