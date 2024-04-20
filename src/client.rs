use crate::borrow::Cow;
use crate::{Error, Frame, OpConfirm, QoS};

use async_trait::async_trait;
use std::sync::{atomic, Arc};
use std::time::Duration;

#[allow(clippy::module_name_repetitions)]
#[async_trait]
pub trait AsyncClient: Send + Sync {
    fn take_event_channel(&mut self) -> Option<async_channel::Receiver<Frame>>;
    async fn send(
        &mut self,
        target: &str,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error>;
    async fn zc_send(
        &mut self,
        target: &str,
        header: Cow<'async_trait>,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error>;
    async fn send_broadcast(
        &mut self,
        target: &str,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error>;
    async fn publish(
        &mut self,
        target: &str,
        payload: Cow<'async_trait>,
        qos: QoS,
    ) -> Result<OpConfirm, Error>;
    async fn subscribe(&mut self, topic: &str, qos: QoS) -> Result<OpConfirm, Error>;
    async fn unsubscribe(&mut self, topic: &str, qos: QoS) -> Result<OpConfirm, Error>;
    async fn subscribe_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<OpConfirm, Error>;
    async fn unsubscribe_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<OpConfirm, Error>;
    /// exclude a topic. it is highly recommended to exclude topics first, then call subscribe
    /// operations to avoid receiving unwanted messages. excluding topics is also an additional
    /// heavy operation so use it only when there is no other way.
    async fn exclude(&mut self, topic: &str, qos: QoS) -> Result<OpConfirm, Error>;
    /// unexclude a topic (include back but not subscribe)
    async fn unexclude(&mut self, topic: &str, qos: QoS) -> Result<OpConfirm, Error>;
    /// exclude multiple topics
    async fn exclude_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<OpConfirm, Error>;
    /// unexclude multiple topics (include back but not subscribe)
    async fn unexclude_bulk(&mut self, topics: &[&str], qos: QoS) -> Result<OpConfirm, Error>;
    async fn ping(&mut self) -> Result<(), Error>;
    fn is_connected(&self) -> bool;
    fn get_connected_beacon(&self) -> Option<Arc<atomic::AtomicBool>>;
    fn get_timeout(&self) -> Option<Duration>;
    fn get_name(&self) -> &str;
}

#[macro_export]
macro_rules! empty_payload {
    () => {
        $crate::borrow::Cow::Borrowed(&[])
    };
}
