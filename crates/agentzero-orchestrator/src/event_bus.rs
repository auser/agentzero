//! Event bus abstraction for cross-node event distribution.
//!
//! `InMemoryEventBus` uses `tokio::broadcast` for single-process use.
//! `FileBackedEventBus` wraps the broadcast with append-only JSONL persistence
//! so events survive process restarts (useful for research pipelines).
//! A Redis-backed implementation can be added behind the `redis` feature flag.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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

/// A timestamped event written to (and read from) JSONL files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedEvent {
    /// Unix timestamp (seconds since epoch) of when the event was published.
    pub timestamp: u64,
    /// Channel the event was published on.
    pub channel: String,
    /// Arbitrary JSON or text payload.
    pub payload: String,
}

/// File-backed event bus that appends events to JSONL for durability.
///
/// Real-time subscribers still get events via `tokio::broadcast`. On publish,
/// each event is also appended (as a single JSON line) to the configured file.
/// Use [`FileBackedEventBus::replay`] to read back historical events.
pub struct FileBackedEventBus {
    sender: broadcast::Sender<BusEvent>,
    log_path: PathBuf,
    writer: tokio::sync::Mutex<tokio::io::BufWriter<tokio::fs::File>>,
}

impl FileBackedEventBus {
    /// Open (or create) a JSONL event log at `path`.
    pub async fn open(path: impl AsRef<Path>, capacity: usize) -> anyhow::Result<Arc<Self>> {
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

        let (sender, _) = broadcast::channel(capacity);

        Ok(Arc::new(Self {
            sender,
            log_path,
            writer: tokio::sync::Mutex::new(tokio::io::BufWriter::new(file)),
        }))
    }

    /// Replay all persisted events, optionally filtered by channel.
    pub async fn replay(
        &self,
        channel_filter: Option<&str>,
    ) -> anyhow::Result<Vec<PersistedEvent>> {
        use tokio::io::AsyncBufReadExt;

        let file = tokio::fs::File::open(&self.log_path).await?;
        let reader = tokio::io::BufReader::new(file);
        let mut lines = reader.lines();
        let mut events = Vec::new();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<PersistedEvent>(&line) {
                Ok(evt) => {
                    if let Some(filter) = channel_filter {
                        if evt.channel != filter {
                            continue;
                        }
                    }
                    events.push(evt);
                }
                Err(e) => {
                    tracing::warn!(line = %line, error = %e, "skipping malformed event line");
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

#[async_trait::async_trait]
impl EventBus for FileBackedEventBus {
    async fn publish(&self, channel: &str, payload: &str) -> anyhow::Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};
        use tokio::io::AsyncWriteExt;

        let event = BusEvent {
            channel: channel.to_string(),
            payload: payload.to_string(),
        };

        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let persisted = PersistedEvent {
            timestamp: ts,
            channel: channel.to_string(),
            payload: payload.to_string(),
        };

        // Append to JSONL log.
        let mut line = serde_json::to_string(&persisted)?;
        line.push('\n');

        {
            let mut w = self.writer.lock().await;
            w.write_all(line.as_bytes()).await?;
            w.flush().await?;
        }

        // Broadcast to live subscribers (ok if none).
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

    // --- FileBackedEventBus tests ---

    fn temp_log_path(suffix: &str) -> std::path::PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "agentzero-events-{}-{nanos}-{seq}-{suffix}.jsonl",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn file_backed_publish_and_replay() {
        let path = temp_log_path("roundtrip");
        let bus = FileBackedEventBus::open(&path, 16).await.expect("open");

        bus.publish("research", "step-1 done").await.unwrap();
        bus.publish("research", "step-2 done").await.unwrap();
        bus.publish("alerts", "disk-low").await.unwrap();

        let all = bus.replay(None).await.unwrap();
        assert_eq!(all.len(), 3);

        let research = bus.replay(Some("research")).await.unwrap();
        assert_eq!(research.len(), 2);
        assert_eq!(research[0].payload, "step-1 done");
        assert_eq!(research[1].payload, "step-2 done");

        let alerts = bus.replay(Some("alerts")).await.unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].payload, "disk-low");

        tokio::fs::remove_file(path).await.ok();
    }

    #[tokio::test]
    async fn file_backed_live_subscribers_receive() {
        let path = temp_log_path("live");
        let bus = FileBackedEventBus::open(&path, 16).await.expect("open");

        let mut rx = bus.subscribe("ch");
        bus.publish("ch", "hello").await.unwrap();

        let event = rx.recv().await.expect("should receive");
        assert_eq!(event.payload, "hello");

        tokio::fs::remove_file(path).await.ok();
    }

    #[tokio::test]
    async fn file_backed_survives_reopen() {
        let path = temp_log_path("reopen");

        // First session: write events.
        {
            let bus = FileBackedEventBus::open(&path, 16).await.expect("open");
            bus.publish("pipeline", "a").await.unwrap();
            bus.publish("pipeline", "b").await.unwrap();
        }

        // Second session: reopen and replay.
        {
            let bus = FileBackedEventBus::open(&path, 16).await.expect("reopen");
            let events = bus.replay(Some("pipeline")).await.unwrap();
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].payload, "a");
            assert_eq!(events[1].payload, "b");

            // New events append.
            bus.publish("pipeline", "c").await.unwrap();
            let events = bus.replay(Some("pipeline")).await.unwrap();
            assert_eq!(events.len(), 3);
        }

        tokio::fs::remove_file(path).await.ok();
    }

    #[tokio::test]
    async fn file_backed_replay_empty_file() {
        let path = temp_log_path("empty");
        let bus = FileBackedEventBus::open(&path, 16).await.expect("open");
        let events = bus.replay(None).await.unwrap();
        assert!(events.is_empty());
        tokio::fs::remove_file(path).await.ok();
    }
}
