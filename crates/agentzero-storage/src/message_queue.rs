//! Durable per-agent message queue with ACK/NACK delivery guarantees.
//!
//! Built on SQLite for persistence. Messages survive subscriber offline periods
//! and are delivered at-least-once with configurable retry and dead-letter handling.
//!
//! This complements the existing broadcast-based `EventBus` (good for audit/logging)
//! with queue semantics (good for inter-agent messaging).

use anyhow::Context;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// A queued message with delivery tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedMessage {
    pub id: i64,
    pub queue: String,
    pub sender: String,
    pub payload: String,
    pub correlation_id: Option<String>,
    pub created_ms: u64,
    pub delivery_count: u32,
    pub acked: bool,
}

/// Durable message queue backed by SQLite.
pub struct MessageQueue {
    conn: Mutex<Connection>,
    #[allow(dead_code)]
    db_path: PathBuf,
    /// Maximum delivery attempts before moving to dead-letter.
    max_retries: u32,
}

impl MessageQueue {
    /// Open or create a message queue at the given path.
    pub fn open(path: impl AsRef<Path>, max_retries: u32) -> anyhow::Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(&db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                queue TEXT NOT NULL,
                sender TEXT NOT NULL,
                payload TEXT NOT NULL,
                correlation_id TEXT,
                created_ms INTEGER NOT NULL,
                delivery_count INTEGER NOT NULL DEFAULT 0,
                acked INTEGER NOT NULL DEFAULT 0,
                dead_letter INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_msg_queue ON messages(queue, acked, dead_letter);
            CREATE INDEX IF NOT EXISTS idx_msg_correlation ON messages(correlation_id);",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
            db_path,
            max_retries,
        })
    }

    /// Enqueue a message to a specific agent's queue.
    pub fn send(
        &self,
        queue: &str,
        sender: &str,
        payload: &str,
        correlation_id: Option<&str>,
    ) -> anyhow::Result<i64> {
        let now_ms = now_millis();
        let conn = self.conn.lock().expect("mq lock poisoned");
        conn.execute(
            "INSERT INTO messages (queue, sender, payload, correlation_id, created_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![queue, sender, payload, correlation_id, now_ms as i64],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Receive the next unacknowledged message for a queue.
    /// Increments `delivery_count`. Returns `None` if the queue is empty.
    pub fn receive(&self, queue: &str) -> anyhow::Result<Option<QueuedMessage>> {
        let conn = self.conn.lock().expect("mq lock poisoned");
        let result = conn.query_row(
            "SELECT id, queue, sender, payload, correlation_id, created_ms, delivery_count
             FROM messages
             WHERE queue = ?1 AND acked = 0 AND dead_letter = 0
             ORDER BY id ASC LIMIT 1",
            [queue],
            |row| {
                Ok(QueuedMessage {
                    id: row.get(0)?,
                    queue: row.get(1)?,
                    sender: row.get(2)?,
                    payload: row.get(3)?,
                    correlation_id: row.get(4)?,
                    created_ms: row.get::<_, i64>(5)? as u64,
                    delivery_count: row.get::<_, i32>(6)? as u32,
                    acked: false,
                })
            },
        );
        match result {
            Ok(mut msg) => {
                msg.delivery_count += 1;
                conn.execute(
                    "UPDATE messages SET delivery_count = ?1 WHERE id = ?2",
                    rusqlite::params![msg.delivery_count as i32, msg.id],
                )?;
                // Move to dead-letter if max retries exceeded.
                if msg.delivery_count > self.max_retries {
                    conn.execute(
                        "UPDATE messages SET dead_letter = 1 WHERE id = ?1",
                        [msg.id],
                    )?;
                    return Ok(None);
                }
                Ok(Some(msg))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to receive message"),
        }
    }

    /// Acknowledge a message (mark as processed, won't be delivered again).
    pub fn ack(&self, message_id: i64) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("mq lock poisoned");
        conn.execute("UPDATE messages SET acked = 1 WHERE id = ?1", [message_id])?;
        Ok(())
    }

    /// Negative-acknowledge: the message will be retried on next `receive()`.
    /// (No-op since delivery_count is already incremented; just don't ack.)
    pub fn nack(&self, _message_id: i64) -> anyhow::Result<()> {
        // delivery_count was already incremented in receive().
        // The message stays unacked and will be picked up again.
        Ok(())
    }

    /// Send a message and wait for a correlated reply.
    /// Returns the reply payload, or error on timeout.
    pub fn send_and_wait(
        &self,
        queue: &str,
        sender: &str,
        payload: &str,
        reply_queue: &str,
        timeout: std::time::Duration,
    ) -> anyhow::Result<String> {
        let correlation_id = format!("req-{}-{}", now_millis(), std::process::id());
        self.send(queue, sender, payload, Some(&correlation_id))?;

        let deadline = std::time::Instant::now() + timeout;
        loop {
            if std::time::Instant::now() > deadline {
                anyhow::bail!("request/reply timeout after {:?}", timeout);
            }
            // Check for correlated reply.
            let conn = self.conn.lock().expect("mq lock poisoned");
            let result = conn.query_row(
                "SELECT id, payload FROM messages
                 WHERE queue = ?1 AND correlation_id = ?2 AND acked = 0 AND dead_letter = 0
                 LIMIT 1",
                rusqlite::params![reply_queue, correlation_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            );
            drop(conn);
            match result {
                Ok((id, reply_payload)) => {
                    self.ack(id)?;
                    return Ok(reply_payload);
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => return Err(e).context("failed to check reply"),
            }
        }
    }

    /// Count pending (unacked, non-dead-letter) messages in a queue.
    pub fn pending_count(&self, queue: &str) -> anyhow::Result<usize> {
        let conn = self.conn.lock().expect("mq lock poisoned");
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE queue = ?1 AND acked = 0 AND dead_letter = 0",
            [queue],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Count dead-letter messages in a queue.
    pub fn dead_letter_count(&self, queue: &str) -> anyhow::Result<usize> {
        let conn = self.conn.lock().expect("mq lock poisoned");
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE queue = ?1 AND dead_letter = 1",
            [queue],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Purge acknowledged messages older than the given duration.
    pub fn gc(&self, max_age: std::time::Duration) -> anyhow::Result<usize> {
        let cutoff_ms = now_millis().saturating_sub(max_age.as_millis() as u64);
        let conn = self.conn.lock().expect("mq lock poisoned");
        let deleted = conn.execute(
            "DELETE FROM messages WHERE acked = 1 AND created_ms < ?1",
            [cutoff_ms as i64],
        )?;
        Ok(deleted)
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_db(suffix: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let ts = now_millis();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "agentzero-mq-{}-{ts}-{seq}-{suffix}.db",
            std::process::id()
        ))
    }

    #[test]
    fn send_and_receive() {
        let path = temp_db("basic");
        let mq = MessageQueue::open(&path, 3).expect("open");

        mq.send("agent-1", "agent-0", "hello", None).expect("send");
        let msg = mq.receive("agent-1").expect("recv").expect("non-empty");
        assert_eq!(msg.queue, "agent-1");
        assert_eq!(msg.sender, "agent-0");
        assert_eq!(msg.payload, "hello");
        assert_eq!(msg.delivery_count, 1);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ack_removes_from_queue() {
        let path = temp_db("ack");
        let mq = MessageQueue::open(&path, 3).expect("open");

        mq.send("q", "s", "msg1", None).expect("send");
        let msg = mq.receive("q").expect("recv").expect("msg");
        mq.ack(msg.id).expect("ack");

        // Queue should now be empty.
        assert!(mq.receive("q").expect("recv").is_none());
        assert_eq!(mq.pending_count("q").expect("count"), 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn nack_allows_redelivery() {
        let path = temp_db("nack");
        let mq = MessageQueue::open(&path, 5).expect("open");

        mq.send("q", "s", "retry-me", None).expect("send");

        // First delivery.
        let msg = mq.receive("q").expect("recv").expect("msg");
        assert_eq!(msg.delivery_count, 1);
        mq.nack(msg.id).expect("nack");

        // Second delivery.
        let msg = mq.receive("q").expect("recv").expect("msg");
        assert_eq!(msg.delivery_count, 2);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn dead_letter_after_max_retries() {
        let path = temp_db("dlq");
        let mq = MessageQueue::open(&path, 2).expect("open");

        mq.send("q", "s", "will-fail", None).expect("send");

        // Exhaust retries.
        let _ = mq.receive("q").expect("recv"); // delivery 1
        let _ = mq.receive("q").expect("recv"); // delivery 2
        let msg = mq.receive("q").expect("recv"); // delivery 3 > max_retries(2)
        assert!(
            msg.is_none(),
            "should be moved to dead-letter after exceeding max retries"
        );

        assert_eq!(mq.pending_count("q").expect("count"), 0);
        assert_eq!(mq.dead_letter_count("q").expect("dlq"), 1);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn separate_queues() {
        let path = temp_db("queues");
        let mq = MessageQueue::open(&path, 3).expect("open");

        mq.send("alice", "bob", "for-alice", None).expect("send");
        mq.send("bob", "alice", "for-bob", None).expect("send");

        let alice_msg = mq.receive("alice").expect("recv").expect("msg");
        assert_eq!(alice_msg.payload, "for-alice");

        let bob_msg = mq.receive("bob").expect("recv").expect("msg");
        assert_eq!(bob_msg.payload, "for-bob");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn correlation_id_preserved() {
        let path = temp_db("corr");
        let mq = MessageQueue::open(&path, 3).expect("open");

        mq.send("q", "s", "request", Some("req-123")).expect("send");
        let msg = mq.receive("q").expect("recv").expect("msg");
        assert_eq!(msg.correlation_id.as_deref(), Some("req-123"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn gc_purges_acked() {
        let path = temp_db("gc");
        let mq = MessageQueue::open(&path, 3).expect("open");

        let id = mq.send("q", "s", "old", None).expect("send");
        mq.ack(id).expect("ack");
        // Small sleep so the acked message's timestamp is strictly in the past.
        std::thread::sleep(std::time::Duration::from_millis(5));
        mq.send("q", "s", "new", None).expect("send");

        // GC with 1ms max age — the acked message is older than 1ms.
        let purged = mq.gc(std::time::Duration::from_millis(1)).expect("gc");
        assert_eq!(purged, 1);
        assert_eq!(mq.pending_count("q").expect("count"), 1);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn request_reply_works() {
        let path = temp_db("rr");
        let mq = std::sync::Arc::new(MessageQueue::open(&path, 3).expect("open"));

        // Simulate responder in a thread.
        let mq2 = mq.clone();
        let responder = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let msg = mq2.receive("server").expect("recv").expect("msg");
            // Send reply to the reply queue with same correlation_id.
            mq2.send(
                "client-reply",
                "server",
                "pong",
                msg.correlation_id.as_deref(),
            )
            .expect("send reply");
            mq2.ack(msg.id).expect("ack");
        });

        let reply = mq
            .send_and_wait(
                "server",
                "client",
                "ping",
                "client-reply",
                std::time::Duration::from_secs(5),
            )
            .expect("should get reply");
        assert_eq!(reply, "pong");

        responder.join().expect("responder should complete");
        let _ = std::fs::remove_file(&path);
    }
}
