//! Event bus abstraction for cross-node event distribution.
//!
//! `InMemoryEventBus` uses `tokio::broadcast` for single-process use.
//! A Redis-backed implementation can be added behind the `redis` feature flag.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;

/// A serializable event published on the bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusEvent {
    pub channel: String,
    pub payload: String,
}

/// Trait for event bus implementations.
#[async_trait::async_trait]
pub trait EventBus: Send + Sync + 'static {
    /// Publish an event to a channel.
    async fn publish(&self, channel: &str, payload: &str) -> anyhow::Result<()>;

    /// Subscribe to a channel. Returns a receiver that yields events.
    fn subscribe(&self, channel: &str) -> EventReceiver;
}

/// A receiver handle for bus events.
pub struct EventReceiver {
    inner: broadcast::Receiver<BusEvent>,
    channel_filter: String,
}

impl EventReceiver {
    /// Receive the next event on this channel.
    pub async fn recv(&mut self) -> Option<BusEvent> {
        loop {
            match self.inner.recv().await {
                Ok(event) if event.channel == self.channel_filter => return Some(event),
                Ok(_) => continue, // skip events for other channels
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(lagged = n, "event bus receiver lagged, skipping events");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

/// In-memory event bus backed by `tokio::broadcast`.
pub struct InMemoryEventBus {
    sender: broadcast::Sender<BusEvent>,
}

impl InMemoryEventBus {
    /// Create a new in-memory event bus with the given channel capacity.
    pub fn new(capacity: usize) -> Arc<Self> {
        let (sender, _) = broadcast::channel(capacity);
        Arc::new(Self { sender })
    }
}

#[async_trait::async_trait]
impl EventBus for InMemoryEventBus {
    async fn publish(&self, channel: &str, payload: &str) -> anyhow::Result<()> {
        let event = BusEvent {
            channel: channel.to_string(),
            payload: payload.to_string(),
        };
        // send returns Err if there are no receivers; that's fine.
        let _ = self.sender.send(event);
        Ok(())
    }

    fn subscribe(&self, channel: &str) -> EventReceiver {
        EventReceiver {
            inner: self.sender.subscribe(),
            channel_filter: channel.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_and_receive() {
        let bus = InMemoryEventBus::new(16);
        let mut rx = bus.subscribe("test-channel");

        bus.publish("test-channel", "hello")
            .await
            .expect("publish should succeed");

        let event = rx.recv().await.expect("should receive event");
        assert_eq!(event.channel, "test-channel");
        assert_eq!(event.payload, "hello");
    }

    #[tokio::test]
    async fn subscribe_filters_by_channel() {
        let bus = InMemoryEventBus::new(16);
        let mut rx_a = bus.subscribe("channel-a");

        bus.publish("channel-b", "wrong").await.expect("publish b");
        bus.publish("channel-a", "right").await.expect("publish a");

        let event = rx_a.recv().await.expect("should receive event");
        assert_eq!(event.payload, "right");
    }

    #[tokio::test]
    async fn multiple_subscribers_receive_same_event() {
        let bus = InMemoryEventBus::new(16);
        let mut rx1 = bus.subscribe("ch");
        let mut rx2 = bus.subscribe("ch");

        bus.publish("ch", "data").await.expect("publish");

        let e1 = rx1.recv().await.expect("rx1 should receive");
        let e2 = rx2.recv().await.expect("rx2 should receive");
        assert_eq!(e1.payload, "data");
        assert_eq!(e2.payload, "data");
    }

    #[tokio::test]
    async fn publish_with_no_subscribers_succeeds() {
        let bus = InMemoryEventBus::new(16);
        // No subscribers — publish should not error.
        let result = bus.publish("ch", "orphan").await;
        assert!(result.is_ok());
    }
}
