//! SQLite-backed event bus with WAL mode for concurrent access.
//!
//! Events are persisted to a SQLite table with auto-increment IDs for
//! ordered replay. In-process delivery uses `tokio::sync::broadcast`
//! for zero-latency notification. Multi-process consumers poll with
//! `last_seen_id` for cross-process event propagation.

use agentzero_core::{Event, EventBus, EventSubscriber};
use async_trait::async_trait;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;

/// SQLite-backed event bus.
///
/// Combines durable SQLite storage with in-process `tokio::sync::broadcast`
/// for real-time subscriber notification. The database uses WAL mode for
/// concurrent read access from multiple processes.
pub struct SqliteEventBus {
    conn: Mutex<Connection>,
    tx: broadcast::Sender<Event>,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl SqliteEventBus {
    /// Open or create an event bus backed by the given SQLite database.
    pub fn open(path: impl AsRef<Path>, capacity: usize) -> anyhow::Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let conn = Connection::open(&db_path)?;
        // WAL mode for concurrent readers.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                rowid INTEGER PRIMARY KEY AUTOINCREMENT,
                event_id TEXT NOT NULL,
                topic TEXT NOT NULL,
                source TEXT NOT NULL,
                payload TEXT NOT NULL,
                privacy_boundary TEXT NOT NULL DEFAULT '',
                correlation_id TEXT,
                timestamp_ms INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_events_topic ON events(topic);
            CREATE INDEX IF NOT EXISTS idx_events_source ON events(source);
            CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp_ms);
            CREATE INDEX IF NOT EXISTS idx_events_event_id ON events(event_id);",
        )?;

        let (tx, _) = broadcast::channel(capacity);

        Ok(Self {
            conn: Mutex::new(conn),
            tx,
            db_path,
        })
    }

    /// Replay events filtered by topic and/or since a given event ID.
    pub fn replay(
        &self,
        topic: Option<&str>,
        since_id: Option<&str>,
    ) -> anyhow::Result<Vec<Event>> {
        let conn = self.conn.lock().expect("event bus db lock poisoned");

        // Find the rowid to start after, if since_id is provided.
        let start_rowid: i64 = if let Some(sid) = since_id {
            conn.query_row(
                "SELECT rowid FROM events WHERE event_id = ?1",
                [sid],
                |row| row.get(0),
            )
            .unwrap_or(0)
        } else {
            0
        };

        let mut events = Vec::new();

        if let Some(t) = topic {
            let mut stmt = conn.prepare(
                "SELECT event_id, topic, source, payload, privacy_boundary, correlation_id, timestamp_ms
                 FROM events WHERE rowid > ?1 AND topic = ?2 ORDER BY rowid ASC",
            )?;
            let rows = stmt.query_map(rusqlite::params![start_rowid, t], row_to_event)?;
            for row in rows {
                events.push(row?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT event_id, topic, source, payload, privacy_boundary, correlation_id, timestamp_ms
                 FROM events WHERE rowid > ?1 ORDER BY rowid ASC",
            )?;
            let rows = stmt.query_map([start_rowid], row_to_event)?;
            for row in rows {
                events.push(row?);
            }
        }

        Ok(events)
    }

    /// Replay events matching a multi-axis filter with SQL-level optimization.
    ///
    /// Uses `WHERE source = ?` and `topic LIKE ?%` for efficient indexed queries
    /// rather than client-side filtering. Ideal for horizontal scaling where a
    /// restarting node needs to catch up on events from specific sources.
    pub fn replay_with_filter(
        &self,
        filter: &agentzero_core::event_bus::EventFilter,
        since_id: Option<&str>,
    ) -> anyhow::Result<Vec<Event>> {
        let conn = self.conn.lock().expect("event bus db lock poisoned");

        let start_rowid: i64 = if let Some(sid) = since_id {
            conn.query_row(
                "SELECT rowid FROM events WHERE event_id = ?1",
                [sid],
                |row| row.get(0),
            )
            .unwrap_or(0)
        } else {
            0
        };

        let mut events = Vec::new();

        match (&filter.source, &filter.topic_prefix) {
            (Some(src), Some(prefix)) => {
                let topic_like = format!("{prefix}%");
                let mut stmt = conn.prepare(
                    "SELECT event_id, topic, source, payload, privacy_boundary, correlation_id, timestamp_ms
                     FROM events WHERE rowid > ?1 AND source = ?2 AND topic LIKE ?3 ORDER BY rowid ASC",
                )?;
                let rows = stmt.query_map(
                    rusqlite::params![start_rowid, src, topic_like],
                    row_to_event,
                )?;
                for row in rows {
                    events.push(row?);
                }
            }
            (Some(src), None) => {
                let mut stmt = conn.prepare(
                    "SELECT event_id, topic, source, payload, privacy_boundary, correlation_id, timestamp_ms
                     FROM events WHERE rowid > ?1 AND source = ?2 ORDER BY rowid ASC",
                )?;
                let rows = stmt.query_map(rusqlite::params![start_rowid, src], row_to_event)?;
                for row in rows {
                    events.push(row?);
                }
            }
            (None, Some(prefix)) => {
                let topic_like = format!("{prefix}%");
                let mut stmt = conn.prepare(
                    "SELECT event_id, topic, source, payload, privacy_boundary, correlation_id, timestamp_ms
                     FROM events WHERE rowid > ?1 AND topic LIKE ?2 ORDER BY rowid ASC",
                )?;
                let rows =
                    stmt.query_map(rusqlite::params![start_rowid, topic_like], row_to_event)?;
                for row in rows {
                    events.push(row?);
                }
            }
            (None, None) => {
                return self.replay(None, since_id);
            }
        }

        Ok(events)
    }

    /// Delete events older than the given duration. Returns count deleted.
    pub fn gc(&self, max_age: Duration) -> anyhow::Result<usize> {
        let cutoff_ms =
            agentzero_core::event_bus::now_ms_public().saturating_sub(max_age.as_millis() as u64);
        let conn = self.conn.lock().expect("event bus db lock poisoned");
        let deleted = conn.execute("DELETE FROM events WHERE timestamp_ms < ?1", [cutoff_ms])?;
        Ok(deleted)
    }
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    Ok(Event {
        id: row.get(0)?,
        topic: row.get(1)?,
        source: row.get(2)?,
        payload: Arc::from(row.get::<_, String>(3)?),
        privacy_boundary: row.get::<_, String>(4)?,
        correlation_id: row.get(5)?,
        timestamp_ms: row.get::<_, i64>(6)? as u64,
    })
}

#[async_trait]
impl EventBus for SqliteEventBus {
    async fn publish(
        &self,
        event: Event,
    ) -> anyhow::Result<agentzero_core::event_bus::PublishResult> {
        // Persist to SQLite first.
        {
            let conn = self.conn.lock().expect("event bus db lock poisoned");
            conn.execute(
                "INSERT INTO events (event_id, topic, source, payload, privacy_boundary, correlation_id, timestamp_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    event.id,
                    event.topic,
                    event.source,
                    &*event.payload,
                    event.privacy_boundary,
                    event.correlation_id,
                    event.timestamp_ms as i64,
                ],
            )?;
        }

        // Then broadcast to in-process subscribers.
        let delivered = self.tx.send(event).unwrap_or(0);
        Ok(agentzero_core::event_bus::PublishResult { delivered })
    }

    fn subscribe(&self) -> Box<dyn EventSubscriber> {
        Box::new(SqliteEventSubscriber {
            rx: self.tx.subscribe(),
        })
    }

    fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }

    async fn replay_since(
        &self,
        topic: Option<&str>,
        since_id: Option<&str>,
    ) -> anyhow::Result<Vec<Event>> {
        self.replay(topic, since_id)
    }

    async fn gc_older_than(&self, max_age: Duration) -> anyhow::Result<usize> {
        self.gc(max_age)
    }
}

struct SqliteEventSubscriber {
    rx: broadcast::Receiver<Event>,
}

#[async_trait]
impl EventSubscriber for SqliteEventSubscriber {
    async fn recv(&mut self) -> anyhow::Result<Event> {
        loop {
            match self.rx.recv().await {
                Ok(event) => return Ok(event),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    agentzero_core::tracing::warn!(
                        skipped = n,
                        "sqlite event bus subscriber lagged, skipping events"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    anyhow::bail!("sqlite event bus closed");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_db(suffix: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "agentzero-sqlite-events-{}-{ts}-{seq}-{suffix}.db",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn publish_and_subscribe() {
        let path = temp_db("pubsub");
        let bus = SqliteEventBus::open(&path, 16).expect("open");
        let mut sub = bus.subscribe();

        bus.publish(Event::new("test.topic", "src", "hello"))
            .await
            .expect("publish");

        let event = sub.recv().await.expect("recv");
        assert_eq!(event.topic, "test.topic");
        assert_eq!(&*event.payload, "hello");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn replay_all() {
        let path = temp_db("replay");
        let bus = SqliteEventBus::open(&path, 16).expect("open");

        bus.publish(Event::new("a.1", "s", "one"))
            .await
            .expect("publish");
        bus.publish(Event::new("a.2", "s", "two"))
            .await
            .expect("publish");
        bus.publish(Event::new("b.1", "s", "three"))
            .await
            .expect("publish");

        let all = bus.replay(None, None).expect("replay");
        assert_eq!(all.len(), 3);

        let topic_a = bus.replay(Some("a.1"), None).expect("replay filtered");
        assert_eq!(topic_a.len(), 1);
        assert_eq!(&*topic_a[0].payload, "one");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn replay_since_id() {
        let path = temp_db("since");
        let bus = SqliteEventBus::open(&path, 16).expect("open");

        bus.publish(Event::new("t", "s", "first"))
            .await
            .expect("publish");
        let all_before = bus.replay(None, None).expect("replay");
        let first_id = all_before[0].id.clone();

        bus.publish(Event::new("t", "s", "second"))
            .await
            .expect("publish");
        bus.publish(Event::new("t", "s", "third"))
            .await
            .expect("publish");

        let since = bus.replay(None, Some(&first_id)).expect("replay since");
        assert_eq!(since.len(), 2);
        assert_eq!(&*since[0].payload, "second");
        assert_eq!(&*since[1].payload, "third");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn gc_removes_old_events() {
        let path = temp_db("gc");
        let bus = SqliteEventBus::open(&path, 16).expect("open");

        // Publish an event with a very old timestamp.
        let mut old_event = Event::new("old", "s", "ancient");
        old_event.timestamp_ms = 1000; // very old
        bus.publish(old_event).await.expect("publish old");

        // Publish a recent event.
        bus.publish(Event::new("new", "s", "recent"))
            .await
            .expect("publish new");

        // GC events older than 1 second — the old event has timestamp_ms=1000.
        let deleted = bus.gc(Duration::from_secs(1)).expect("gc");
        assert_eq!(deleted, 1);

        let remaining = bus.replay(None, None).expect("replay");
        assert_eq!(remaining.len(), 1);
        assert_eq!(&*remaining[0].payload, "recent");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn survives_reopen() {
        let path = temp_db("reopen");

        {
            let bus = SqliteEventBus::open(&path, 16).expect("open");
            bus.publish(Event::new("t", "s", "persisted"))
                .await
                .expect("publish");
        }

        {
            let bus = SqliteEventBus::open(&path, 16).expect("reopen");
            let events = bus.replay(None, None).expect("replay");
            assert_eq!(events.len(), 1);
            assert_eq!(&*events[0].payload, "persisted");
        }

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn multiple_subscribers() {
        let path = temp_db("multisub");
        let bus = SqliteEventBus::open(&path, 16).expect("open");

        let mut sub1 = bus.subscribe();
        let mut sub2 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);

        bus.publish(Event::new("t", "s", "data"))
            .await
            .expect("publish");

        let e1 = sub1.recv().await.expect("recv1");
        let e2 = sub2.recv().await.expect("recv2");
        assert_eq!(&*e1.payload, "data");
        assert_eq!(&*e2.payload, "data");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn publish_result_delivery_count() {
        let path = temp_db("pubresult");
        let bus = SqliteEventBus::open(&path, 16).expect("open");

        let r0 = bus
            .publish(Event::new("t", "s", "no-sub"))
            .await
            .expect("publish");
        assert_eq!(r0.delivered, 0);

        let _sub = bus.subscribe();
        let r1 = bus
            .publish(Event::new("t", "s", "one-sub"))
            .await
            .expect("publish");
        assert_eq!(r1.delivered, 1);

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn replay_with_filter_source_only() {
        let path = temp_db("filter-src");
        let bus = SqliteEventBus::open(&path, 16).expect("open");

        bus.publish(Event::new("t.a", "agent-1", "a1"))
            .await
            .expect("publish");
        bus.publish(Event::new("t.b", "agent-2", "a2"))
            .await
            .expect("publish");
        bus.publish(Event::new("t.c", "agent-1", "a1b"))
            .await
            .expect("publish");

        let filter = agentzero_core::event_bus::EventFilter::source("agent-1");
        let events = bus.replay_with_filter(&filter, None).expect("replay");
        assert_eq!(events.len(), 2);
        assert_eq!(&*events[0].payload, "a1");
        assert_eq!(&*events[1].payload, "a1b");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn replay_with_filter_topic_prefix() {
        let path = temp_db("filter-topic");
        let bus = SqliteEventBus::open(&path, 16).expect("open");

        bus.publish(Event::new("tool.exec", "s", "t1"))
            .await
            .expect("publish");
        bus.publish(Event::new("channel.msg", "s", "c1"))
            .await
            .expect("publish");
        bus.publish(Event::new("tool.write", "s", "t2"))
            .await
            .expect("publish");

        let filter = agentzero_core::event_bus::EventFilter::topic("tool.");
        let events = bus.replay_with_filter(&filter, None).expect("replay");
        assert_eq!(events.len(), 2);
        assert_eq!(&*events[0].payload, "t1");
        assert_eq!(&*events[1].payload, "t2");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn replay_with_filter_both_axes() {
        let path = temp_db("filter-both");
        let bus = SqliteEventBus::open(&path, 16).expect("open");

        bus.publish(Event::new("tool.exec", "agent-1", "match"))
            .await
            .expect("publish");
        bus.publish(Event::new("tool.exec", "agent-2", "wrong-src"))
            .await
            .expect("publish");
        bus.publish(Event::new("channel.msg", "agent-1", "wrong-topic"))
            .await
            .expect("publish");

        let filter = agentzero_core::event_bus::EventFilter::source_and_topic("agent-1", "tool.");
        let events = bus.replay_with_filter(&filter, None).expect("replay");
        assert_eq!(events.len(), 1);
        assert_eq!(&*events[0].payload, "match");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn replay_with_filter_since_id() {
        let path = temp_db("filter-since");
        let bus = SqliteEventBus::open(&path, 16).expect("open");

        bus.publish(Event::new("tool.a", "agent-1", "first"))
            .await
            .expect("publish");
        let all = bus.replay(None, None).expect("replay");
        let first_id = all[0].id.clone();

        bus.publish(Event::new("tool.b", "agent-1", "second"))
            .await
            .expect("publish");
        bus.publish(Event::new("tool.c", "agent-2", "third"))
            .await
            .expect("publish");

        let filter = agentzero_core::event_bus::EventFilter::source("agent-1");
        let events = bus
            .replay_with_filter(&filter, Some(&first_id))
            .expect("replay");
        assert_eq!(events.len(), 1);
        assert_eq!(&*events[0].payload, "second");

        let _ = std::fs::remove_file(&path);
    }
}
