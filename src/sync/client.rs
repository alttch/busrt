use crate::borrow::Cow;
use crate::{Error, QoS};

use std::sync::{atomic, Arc};
use std::time::Duration;

use crate::{SyncEventChannel, SyncOpConfirm};

#[allow(clippy::module_name_repetitions)]
pub trait SyncClient {
    fn take_event_channel(&mut self) -> Option<SyncEventChannel>;
    fn send(&mut self, target: &str, payload: Cow<'_>, qos: QoS) -> Result<SyncOpConfirm, Error>;
    fn zc_send(
        &mut self,
        target: &str,
        header: Cow<'_>,
        payload: Cow<'_>,
        qos: QoS,
    ) -> Result<SyncOpConfirm, Error>;
    fn send_broadcast(
        &mut self,
        target: &str,
        payload: Cow<'_>,
        qos: QoS,
    ) -> Result<SyncOpConfirm, Error>;
    fn publish(&mut self, topic: &str, payload: Cow<'_>, qos: QoS) -> Result<SyncOpConfirm, Error>;
    #[allow(unused_variables)]
    fn publish_for(
        &mut self,
        topic: &str,
        receiver: &str,
        payload: Cow<'_>,
        qos: QoS,
    ) -> Result<SyncOpConfirm, Error> {
        Err(Error::not_supported("publish_for"))
    }
    fn subscribe(&mut self, topic: &str, qos: QoS) -> Result<SyncOpConfirm, Error>;
    fn unsubscribe(&mut self, topic: &str, qos: QoS) -> Result<SyncOpConfirm, Error>;
    fn subscribe_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<SyncOpConfirm, Error>;
    fn unsubscribe_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<SyncOpConfirm, Error>;
    /// exclude a topic. it is highly recommended to exclude topics first, then call subscribe
    /// operations to avoid receiving unwanted messages. excluding topics is also an additional
    /// heavy operation so use it only when there is no other way.
    fn exclude(&mut self, topic: &str, qos: QoS) -> Result<SyncOpConfirm, Error>;
    /// unexclude a topic (include back but not subscribe)
    fn unexclude(&mut self, topic: &str, qos: QoS) -> Result<SyncOpConfirm, Error>;
    /// exclude multiple topics
    fn exclude_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<SyncOpConfirm, Error>;
    /// unexclude multiple topics (include back but not subscribe)
    fn unexclude_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<SyncOpConfirm, Error>;
    fn ping(&mut self) -> Result<(), Error>;
    fn is_connected(&self) -> bool;
    fn get_connected_beacon(&self) -> Option<Arc<atomic::AtomicBool>>;
    fn get_timeout(&self) -> Option<Duration>;
    fn get_name(&self) -> &str;
}
