//! Event bus for inter-agent communication.
//!
//! The bus is the central nervous system for all messages in AgentZero.
//! Agents, channels, and system components publish and subscribe to events
//! on the bus. The `EventBus` trait abstracts over transport so a future
//! multi-node implementation (e.g. iroh QUIC) can be swapped in without
//! changing any agent code.
//!
//! `FileBackedBus` wraps the in-memory broadcast with append-only JSONL
//! persistence so events survive process restarts (useful for research
//! pipelines and audit trails).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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

    /// Replay events since a given event ID (exclusive). Returns events in order.
    /// If `since_id` is `None`, replays all events. If the bus does not support
    /// persistence, returns an empty vec.
    async fn replay_since(
        &self,
        _topic: Option<&str>,
        _since_id: Option<&str>,
    ) -> anyhow::Result<Vec<Event>> {
        Ok(Vec::new())
    }

    /// Delete events older than the given age. Returns count of deleted events.
    /// No-op for buses that don't support persistence.
    async fn gc_older_than(&self, _max_age: std::time::Duration) -> anyhow::Result<usize> {
        Ok(0)
    }
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

/// File-backed event bus: wraps `InMemoryBus` with append-only JSONL persistence.
///
/// Every published event is serialized to a JSONL log file before being
/// broadcast to live subscribers. Use [`FileBackedBus::replay`] to read
/// back all persisted events (e.g. after a restart).
pub struct FileBackedBus {
    inner: InMemoryBus,
    log_path: PathBuf,
    writer: tokio::sync::Mutex<tokio::io::BufWriter<tokio::fs::File>>,
}

impl FileBackedBus {
    /// Open (or create) a JSONL event log at `path` with the given broadcast capacity.
    pub async fn open(path: impl AsRef<Path>, capacity: usize) -> anyhow::Result<Self> {
        let log_path = path.as_ref().to_path_buf();
        if let Some(parent) = log_path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }

        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .await?;

        Ok(Self {
            inner: InMemoryBus::new(capacity),
            log_path,
            writer: tokio::sync::Mutex::new(tokio::io::BufWriter::new(file)),
        })
    }

    /// Replay all persisted events, optionally filtered by topic prefix.
    pub async fn replay(&self, topic_filter: Option<&str>) -> anyhow::Result<Vec<Event>> {
        use tokio::io::AsyncBufReadExt;

        let file = tokio::fs::File::open(&self.log_path).await?;
        let reader = tokio::io::BufReader::new(file);
        let mut lines = reader.lines();
        let mut events = Vec::new();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Event>(&line) {
                Ok(evt) => {
                    if let Some(prefix) = topic_filter {
                        if !evt.topic.starts_with(prefix) {
                            continue;
                        }
                    }
                    events.push(evt);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "skipping malformed event line in log");
                }
            }
        }

        Ok(events)
    }

    /// Return the path to the JSONL log file.
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }
}

#[async_trait]
impl EventBus for FileBackedBus {
    async fn publish(&self, event: Event) -> anyhow::Result<()> {
        use tokio::io::AsyncWriteExt;

        // Persist first, then broadcast.
        let mut line = serde_json::to_string(&event)?;
        line.push('\n');

        {
            let mut w = self.writer.lock().await;
            w.write_all(line.as_bytes()).await?;
            w.flush().await?;
        }

        self.inner.publish(event).await
    }

    fn subscribe(&self) -> Box<dyn EventSubscriber> {
        self.inner.subscribe()
    }

    fn subscriber_count(&self) -> usize {
        self.inner.subscriber_count()
    }

    async fn replay_since(
        &self,
        topic: Option<&str>,
        since_id: Option<&str>,
    ) -> anyhow::Result<Vec<Event>> {
        let all = self.replay(topic).await?;
        if let Some(sid) = since_id {
            // Find the position after the given event ID.
            if let Some(pos) = all.iter().position(|e| e.id == sid) {
                Ok(all[pos + 1..].to_vec())
            } else {
                Ok(all)
            }
        } else {
            Ok(all)
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
    now_ms_public()
}

/// Public helper for current time in milliseconds (used by storage crate for GC).
pub fn now_ms_public() -> u64 {
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

// ---------------------------------------------------------------------------
// TypedTopic — compile-time type-safe pub/sub wrapper
// ---------------------------------------------------------------------------

/// A compile-time type-safe topic that handles serialization/deserialization
/// of messages automatically.
///
/// ```ignore
/// let topic = TypedTopic::<AnnounceMessage>::new("agent.announce");
///
/// // Publish — M is serialized to JSON automatically
/// topic.publish(&bus, "agent-1", &msg).await?;
///
/// // Subscribe — returns typed messages
/// let mut sub = topic.subscribe(&bus);
/// let msg: AnnounceMessage = sub.recv().await?;
/// ```
///
/// This wraps the existing string-based EventBus without replacing it.
/// String-based topics continue to work for backward compatibility.
pub struct TypedTopic<M> {
    name: String,
    _phantom: std::marker::PhantomData<M>,
}

impl<M: Serialize + for<'de> Deserialize<'de> + Send> TypedTopic<M> {
    /// Create a typed topic with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// The topic name string.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Publish a typed message to the bus.
    pub async fn publish(&self, bus: &dyn EventBus, source: &str, msg: &M) -> anyhow::Result<()> {
        let payload = serde_json::to_string(msg)
            .map_err(|e| anyhow::anyhow!("failed to serialize typed topic message: {e}"))?;
        bus.publish(Event::new(&self.name, source, payload)).await
    }

    /// Publish a typed message with a privacy boundary.
    pub async fn publish_with_boundary(
        &self,
        bus: &dyn EventBus,
        source: &str,
        msg: &M,
        boundary: &str,
    ) -> anyhow::Result<()> {
        let payload = serde_json::to_string(msg)
            .map_err(|e| anyhow::anyhow!("failed to serialize typed topic message: {e}"))?;
        let event = Event::new(&self.name, source, payload).with_boundary(boundary);
        bus.publish(event).await
    }

    /// Create a typed subscriber that deserializes messages from this topic.
    pub fn subscribe(&self, bus: &dyn EventBus) -> TypedSubscriber<M> {
        TypedSubscriber {
            inner: bus.subscribe(),
            topic_name: self.name.clone(),
            _phantom: std::marker::PhantomData,
        }
    }
}

/// A subscriber that automatically deserializes messages into the expected type.
pub struct TypedSubscriber<M> {
    inner: Box<dyn EventSubscriber>,
    topic_name: String,
    _phantom: std::marker::PhantomData<M>,
}

impl<M: for<'de> Deserialize<'de> + Send> TypedSubscriber<M> {
    /// Receive the next typed message on this topic.
    ///
    /// Filters events to this topic and deserializes the payload.
    /// Non-matching topics are silently skipped.
    /// Deserialization errors are returned as `Err`.
    pub async fn recv(&mut self) -> anyhow::Result<M> {
        loop {
            let event = self.inner.recv().await?;
            if event.topic == self.topic_name {
                let msg: M = serde_json::from_str(&event.payload).map_err(|e| {
                    anyhow::anyhow!(
                        "typed topic '{}' deserialization error: {e}",
                        self.topic_name
                    )
                })?;
                return Ok(msg);
            }
        }
    }
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

    // --- FileBackedBus tests ---

    fn temp_event_log(suffix: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let ts = now_ms();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "agentzero-core-events-{}-{ts}-{seq}-{suffix}.jsonl",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn file_backed_publish_and_replay() {
        let path = temp_event_log("roundtrip");
        let bus = FileBackedBus::open(&path, 16).await.expect("open");

        bus.publish(Event::new("task.research.raw", "researcher", "step-1"))
            .await
            .unwrap();
        bus.publish(Event::new("task.research.raw", "researcher", "step-2"))
            .await
            .unwrap();
        bus.publish(Event::new("task.alert", "system", "disk-low"))
            .await
            .unwrap();

        let all = bus.replay(None).await.unwrap();
        assert_eq!(all.len(), 3);

        let research = bus.replay(Some("task.research.")).await.unwrap();
        assert_eq!(research.len(), 2);
        assert_eq!(research[0].payload, "step-1");
        assert_eq!(research[1].payload, "step-2");

        tokio::fs::remove_file(path).await.ok();
    }

    #[tokio::test]
    async fn file_backed_live_subscribers() {
        let path = temp_event_log("live");
        let bus = FileBackedBus::open(&path, 16).await.expect("open");

        let mut sub = bus.subscribe();
        bus.publish(Event::new("t", "s", "hello")).await.unwrap();

        let event = sub.recv().await.unwrap();
        assert_eq!(event.payload, "hello");

        tokio::fs::remove_file(path).await.ok();
    }

    #[tokio::test]
    async fn file_backed_survives_reopen() {
        let path = temp_event_log("reopen");

        {
            let bus = FileBackedBus::open(&path, 16).await.expect("open");
            bus.publish(Event::new("pipeline.a", "agent", "first"))
                .await
                .unwrap();
            bus.publish(Event::new("pipeline.a", "agent", "second"))
                .await
                .unwrap();
        }

        {
            let bus = FileBackedBus::open(&path, 16).await.expect("reopen");
            let events = bus.replay(Some("pipeline.")).await.unwrap();
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].payload, "first");
            assert_eq!(events[1].payload, "second");

            // New events append.
            bus.publish(Event::new("pipeline.a", "agent", "third"))
                .await
                .unwrap();
            let events = bus.replay(None).await.unwrap();
            assert_eq!(events.len(), 3);
        }

        tokio::fs::remove_file(path).await.ok();
    }

    #[tokio::test]
    async fn file_backed_subscriber_count() {
        let path = temp_event_log("subcount");
        let bus = FileBackedBus::open(&path, 16).await.expect("open");

        assert_eq!(bus.subscriber_count(), 0);
        let _s1 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);
        let _s2 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);

        tokio::fs::remove_file(path).await.ok();
    }

    // --- TypedTopic tests ---

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestMessage {
        text: String,
        count: u32,
    }

    #[tokio::test]
    async fn typed_topic_publish_subscribe_roundtrip() {
        let bus = InMemoryBus::new(16);
        let topic = TypedTopic::<TestMessage>::new("test.typed");

        let mut sub = topic.subscribe(&bus);

        let msg = TestMessage {
            text: "hello".into(),
            count: 42,
        };
        topic
            .publish(&bus, "test-source", &msg)
            .await
            .expect("publish should succeed");

        let received = sub.recv().await.expect("recv should succeed");
        assert_eq!(received, msg);
    }

    #[tokio::test]
    async fn typed_topic_filters_other_topics() {
        let bus = InMemoryBus::new(16);
        let topic = TypedTopic::<TestMessage>::new("my.topic");

        let mut sub = topic.subscribe(&bus);

        // Publish to a different topic — should be filtered out
        bus.publish(Event::new(
            "other.topic",
            "src",
            r#"{"text":"nope","count":0}"#,
        ))
        .await
        .unwrap();

        // Publish to our topic
        let msg = TestMessage {
            text: "yes".into(),
            count: 1,
        };
        topic.publish(&bus, "src", &msg).await.unwrap();

        let received = sub.recv().await.unwrap();
        assert_eq!(received.text, "yes");
    }

    #[tokio::test]
    async fn typed_topic_deserialization_error() {
        let bus = InMemoryBus::new(16);
        let topic_name = "bad.payload";
        let topic = TypedTopic::<TestMessage>::new(topic_name);

        let mut sub = topic.subscribe(&bus);

        // Publish invalid JSON for this type
        bus.publish(Event::new(topic_name, "src", "not valid json"))
            .await
            .unwrap();

        let result = sub.recv().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("deserialization error"));
    }

    #[tokio::test]
    async fn typed_topic_with_boundary() {
        let bus = InMemoryBus::new(16);
        let topic = TypedTopic::<TestMessage>::new("secure.topic");

        let msg = TestMessage {
            text: "secret".into(),
            count: 0,
        };
        topic
            .publish_with_boundary(&bus, "agent", &msg, "local_only")
            .await
            .expect("publish should succeed");

        // Verify the event was published with the boundary
        let mut raw_sub = bus.subscribe();
        topic
            .publish_with_boundary(&bus, "agent", &msg, "encrypted_only")
            .await
            .unwrap();
        let event = raw_sub.recv().await.unwrap();
        assert_eq!(event.privacy_boundary, "encrypted_only");
    }
}
