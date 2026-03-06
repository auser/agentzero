//! Event bus for inter-agent communication.
//!
//! The bus is the central nervous system for all messages in AgentZero.
//! Agents, channels, and system components publish and subscribe to events
//! on the bus. The `EventBus` trait abstracts over transport so a future
//! multi-node implementation (e.g. iroh QUIC) can be swapped in without
//! changing any agent code.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

/// A message on the bus. Agents produce and consume these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unique event identifier.
    pub id: String,
    /// Topic string for pub/sub routing (e.g. "task.image.complete").
    pub topic: String,
    /// Who published this event (agent_id, channel name, or "system").
    pub source: String,
    /// Event payload — typically JSON, but can be any string.
    pub payload: String,
    /// Privacy boundary inherited from the source (e.g. "local_only", "any").
    pub privacy_boundary: String,
    /// Unix timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// Traces a chain of events back to the original trigger.
    /// All events in an agent chain share the same correlation_id.
    pub correlation_id: Option<String>,
}

impl Event {
    /// Create a new event with a generated id and current timestamp.
    pub fn new(
        topic: impl Into<String>,
        source: impl Into<String>,
        payload: impl Into<String>,
    ) -> Self {
        Self {
            id: new_event_id(),
            topic: topic.into(),
            source: source.into(),
            payload: payload.into(),
            privacy_boundary: String::new(),
            timestamp_ms: now_ms(),
            correlation_id: None,
        }
    }

    /// Set the correlation id (builder pattern).
    pub fn with_correlation(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(id.into());
        self
    }

    /// Set the privacy boundary (builder pattern).
    pub fn with_boundary(mut self, boundary: impl Into<String>) -> Self {
        self.privacy_boundary = boundary.into();
        self
    }
}

/// Trait for the event bus — abstracts over in-memory vs distributed transports.
#[async_trait]
pub trait EventBus: Send + Sync {
    /// Publish an event to all subscribers.
    async fn publish(&self, event: Event) -> anyhow::Result<()>;

    /// Create a new subscriber that receives all future events.
    fn subscribe(&self) -> Box<dyn EventSubscriber>;

    /// Number of active subscribers.
    fn subscriber_count(&self) -> usize;
}

/// Subscriber that can filter and receive events.
#[async_trait]
pub trait EventSubscriber: Send {
    /// Receive the next event.
    async fn recv(&mut self) -> anyhow::Result<Event>;

    /// Receive the next event whose topic starts with the given prefix.
    /// Events that don't match are silently skipped.
    async fn recv_filtered(&mut self, topic_prefix: &str) -> anyhow::Result<Event> {
        loop {
            let event = self.recv().await?;
            if event.topic.starts_with(topic_prefix) {
                return Ok(event);
            }
        }
    }
}

/// In-memory bus backed by `tokio::sync::broadcast`.
pub struct InMemoryBus {
    tx: broadcast::Sender<Event>,
}

impl InMemoryBus {
    /// Create a new bus with the given channel capacity.
    /// Lagged receivers will skip missed events (lossy).
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Default capacity suitable for most single-process deployments.
    pub fn default_capacity() -> Self {
        Self::new(256)
    }
}

#[async_trait]
impl EventBus for InMemoryBus {
    async fn publish(&self, event: Event) -> anyhow::Result<()> {
        // send returns Err if there are no receivers — that's fine,
        // the event is simply dropped.
        let _ = self.tx.send(event);
        Ok(())
    }

    fn subscribe(&self) -> Box<dyn EventSubscriber> {
        Box::new(InMemorySubscriber {
            rx: self.tx.subscribe(),
        })
    }

    fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

/// Subscriber for the in-memory bus.
pub struct InMemorySubscriber {
    rx: broadcast::Receiver<Event>,
}

#[async_trait]
impl EventSubscriber for InMemorySubscriber {
    async fn recv(&mut self) -> anyhow::Result<Event> {
        loop {
            match self.rx.recv().await {
                Ok(event) => return Ok(event),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "event bus subscriber lagged, skipping events");
                    // Continue to receive the next available event.
                }
                Err(broadcast::error::RecvError::Closed) => {
                    anyhow::bail!("event bus closed");
                }
            }
        }
    }
}

/// Glob-style topic matching: "task.image.*" matches "task.image.complete".
pub fn topic_matches(pattern: &str, topic: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.ends_with(".*") {
        let prefix = &pattern[..pattern.len() - 1];
        topic.starts_with(prefix)
    } else {
        pattern == topic
    }
}

/// Generate a simple unique event ID (timestamp + random suffix).
fn new_event_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = now_ms();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("evt-{ts}-{seq}")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Helper to check if two privacy boundaries are compatible.
/// An event can flow from `source_boundary` to a consumer with `consumer_boundary`
/// only if the consumer's boundary is at least as restrictive.
pub fn is_boundary_compatible(source_boundary: &str, consumer_boundary: &str) -> bool {
    // Boundary hierarchy: local_only > encrypted_only > any > "" (no restriction)
    fn level(b: &str) -> u8 {
        match b {
            "local_only" => 3,
            "encrypted_only" => 2,
            "any" => 1,
            _ => 0,
        }
    }
    // A consumer can receive events from a source if the consumer's restriction
    // level is >= the source's level (i.e., consumer is at least as restrictive).
    // A "local_only" event can only go to "local_only" consumers.
    // An unrestricted event can go anywhere.
    level(consumer_boundary) >= level(source_boundary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn topic_matching() {
        assert!(topic_matches("task.image.*", "task.image.complete"));
        assert!(topic_matches("task.image.*", "task.image.error"));
        assert!(!topic_matches("task.image.*", "task.text.complete"));
        assert!(topic_matches(
            "channel.telegram.message",
            "channel.telegram.message"
        ));
        assert!(!topic_matches(
            "channel.telegram.message",
            "channel.slack.message"
        ));
        assert!(topic_matches("*", "anything.at.all"));
    }

    #[test]
    fn boundary_compatibility() {
        // Unrestricted events go anywhere
        assert!(is_boundary_compatible("", ""));
        assert!(is_boundary_compatible("", "any"));
        assert!(is_boundary_compatible("", "local_only"));

        // "any" events go to "any" or more restrictive
        assert!(is_boundary_compatible("any", "any"));
        assert!(is_boundary_compatible("any", "encrypted_only"));
        assert!(is_boundary_compatible("any", "local_only"));

        // "local_only" events only go to "local_only"
        assert!(is_boundary_compatible("local_only", "local_only"));
        assert!(!is_boundary_compatible("local_only", "any"));
        assert!(!is_boundary_compatible("local_only", "encrypted_only"));
        assert!(!is_boundary_compatible("local_only", ""));

        // "encrypted_only" events go to "encrypted_only" or "local_only"
        assert!(is_boundary_compatible("encrypted_only", "encrypted_only"));
        assert!(is_boundary_compatible("encrypted_only", "local_only"));
        assert!(!is_boundary_compatible("encrypted_only", "any"));
    }

    #[test]
    fn event_builder() {
        let event = Event::new("task.test", "agent-1", r#"{"result":"ok"}"#)
            .with_correlation("corr-123")
            .with_boundary("local_only");

        assert_eq!(event.topic, "task.test");
        assert_eq!(event.source, "agent-1");
        assert_eq!(event.correlation_id.as_deref(), Some("corr-123"));
        assert_eq!(event.privacy_boundary, "local_only");
        assert!(event.timestamp_ms > 0);
        assert!(event.id.starts_with("evt-"));
    }

    #[tokio::test]
    async fn in_memory_bus_publish_subscribe() {
        let bus = InMemoryBus::new(16);
        let mut sub = bus.subscribe();

        bus.publish(Event::new("test.topic", "src", "hello"))
            .await
            .unwrap();

        let event = sub.recv().await.unwrap();
        assert_eq!(event.topic, "test.topic");
        assert_eq!(event.payload, "hello");
    }

    #[tokio::test]
    async fn in_memory_bus_multiple_subscribers() {
        let bus = InMemoryBus::new(16);
        let mut sub1 = bus.subscribe();
        let mut sub2 = bus.subscribe();

        assert_eq!(bus.subscriber_count(), 2);

        bus.publish(Event::new("t", "s", "data")).await.unwrap();

        let e1 = sub1.recv().await.unwrap();
        let e2 = sub2.recv().await.unwrap();
        assert_eq!(e1.payload, "data");
        assert_eq!(e2.payload, "data");
    }

    #[tokio::test]
    async fn filtered_recv() {
        let bus = InMemoryBus::new(16);
        let mut sub = bus.subscribe();

        bus.publish(Event::new("task.text.complete", "a", "text"))
            .await
            .unwrap();
        bus.publish(Event::new("task.image.complete", "b", "image"))
            .await
            .unwrap();

        let event = sub.recv_filtered("task.image.").await.unwrap();
        assert_eq!(event.payload, "image");
    }

    #[tokio::test]
    async fn publish_with_no_subscribers_is_ok() {
        let bus = InMemoryBus::new(16);
        // No subscribers — should not error
        bus.publish(Event::new("orphan", "system", ""))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn bus_closed_returns_error() {
        let bus = Arc::new(InMemoryBus::new(16));
        let mut sub = bus.subscribe();

        // Drop the bus (and its sender)
        drop(bus);

        let result = sub.recv().await;
        assert!(result.is_err());
    }
}
