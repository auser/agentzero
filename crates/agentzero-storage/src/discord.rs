//! SQLite-backed Discord message history with keyword search.
//!
//! Stores messages from the `DiscordHistoryChannel` for later retrieval
//! via the `DiscordSearchTool`.

use anyhow::Context;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS discord_messages (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id  TEXT NOT NULL,
    author_id   TEXT NOT NULL,
    author_name TEXT NOT NULL,
    content     TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_discord_content ON discord_messages(content);
CREATE INDEX IF NOT EXISTS idx_discord_channel ON discord_messages(channel_id);
CREATE INDEX IF NOT EXISTS idx_discord_created ON discord_messages(created_at);

CREATE TABLE IF NOT EXISTS discord_name_cache (
    user_id     TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    cached_at   INTEGER NOT NULL
);
";

/// A stored Discord message.
#[derive(Debug, Clone)]
pub struct DiscordMessage {
    pub channel_id: String,
    pub author_id: String,
    pub author_name: String,
    pub content: String,
    pub created_at: u64,
}

/// SQLite-backed store for Discord message history.
pub struct DiscordHistoryStore {
    conn: Mutex<Connection>,
}

impl DiscordHistoryStore {
    /// Open (or create) the Discord history database at the given path.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open discord history db: {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .context("failed to set WAL mode")?;
        conn.execute_batch(SCHEMA)
            .context("failed to create discord schema")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Insert a message into the history store.
    pub fn insert(&self, msg: &DiscordMessage) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("discord db lock poisoned");
        conn.execute(
            "INSERT INTO discord_messages (channel_id, author_id, author_name, content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                msg.channel_id,
                msg.author_id,
                msg.author_name,
                msg.content,
                msg.created_at,
            ],
        )
        .context("failed to insert discord message")?;
        Ok(())
    }

    /// Search messages by keyword. Returns up to `limit` results ordered by recency.
    pub fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<DiscordMessage>> {
        let conn = self.conn.lock().expect("discord db lock poisoned");
        let pattern = format!("%{query}%");
        let mut stmt = conn
            .prepare(
                "SELECT channel_id, author_id, author_name, content, created_at
                 FROM discord_messages
                 WHERE content LIKE ?1
                 ORDER BY created_at DESC
                 LIMIT ?2",
            )
            .context("failed to prepare search query")?;
        let rows = stmt
            .query_map(rusqlite::params![pattern, limit], |row| {
                Ok(DiscordMessage {
                    channel_id: row.get(0)?,
                    author_id: row.get(1)?,
                    author_name: row.get(2)?,
                    content: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .context("failed to execute search")?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row.context("failed to read row")?);
        }
        Ok(results)
    }

    /// Cache a Discord user ID → display name mapping.
    pub fn cache_name(&self, user_id: &str, display_name: &str) -> anyhow::Result<()> {
        let now = now_epoch();
        let conn = self.conn.lock().expect("discord db lock poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO discord_name_cache (user_id, display_name, cached_at)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![user_id, display_name, now],
        )
        .context("failed to cache name")?;
        Ok(())
    }

    /// Look up a cached display name for a user ID. Returns None if expired or missing.
    pub fn get_cached_name(&self, user_id: &str, ttl_secs: u64) -> anyhow::Result<Option<String>> {
        let conn = self.conn.lock().expect("discord db lock poisoned");
        let cutoff = now_epoch().saturating_sub(ttl_secs);
        let result = conn.query_row(
            "SELECT display_name FROM discord_name_cache
             WHERE user_id = ?1 AND cached_at >= ?2",
            rusqlite::params![user_id, cutoff],
            |row| row.get(0),
        );
        match result {
            Ok(name) => Ok(Some(name)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get total message count.
    pub fn message_count(&self) -> anyhow::Result<u64> {
        let conn = self.conn.lock().expect("discord db lock poisoned");
        let count: u64 = conn.query_row("SELECT COUNT(*) FROM discord_messages", [], |row| {
            row.get(0)
        })?;
        Ok(count)
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static CTR: AtomicU64 = AtomicU64::new(0);

    fn temp_db() -> (DiscordHistoryStore, std::path::PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = CTR.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-discord-{}-{nanos}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create dir");
        let path = dir.join("discord.db");
        let store = DiscordHistoryStore::open(&path).expect("open db");
        (store, dir)
    }

    fn make_msg(content: &str) -> DiscordMessage {
        DiscordMessage {
            channel_id: "ch-1".to_string(),
            author_id: "user-1".to_string(),
            author_name: "Alice".to_string(),
            content: content.to_string(),
            created_at: now_epoch(),
        }
    }

    #[test]
    fn insert_and_search() {
        let (store, dir) = temp_db();
        store.insert(&make_msg("hello world")).expect("insert");
        store.insert(&make_msg("goodbye world")).expect("insert");
        store.insert(&make_msg("unrelated")).expect("insert");

        let results = store.search("world", 10).expect("search");
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|m| m.content.contains("world")));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn search_respects_limit() {
        let (store, dir) = temp_db();
        for i in 0..10 {
            store
                .insert(&make_msg(&format!("message {i} about testing")))
                .expect("insert");
        }
        let results = store.search("testing", 3).expect("search");
        assert_eq!(results.len(), 3);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn name_cache_roundtrip() {
        let (store, dir) = temp_db();
        store.cache_name("123", "Alice").expect("cache");
        let name = store.get_cached_name("123", 3600).expect("get");
        assert_eq!(name.as_deref(), Some("Alice"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn name_cache_returns_none_for_unknown() {
        let (store, dir) = temp_db();
        let name = store.get_cached_name("999", 3600).expect("get");
        assert!(name.is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn message_count_tracks_inserts() {
        let (store, dir) = temp_db();
        assert_eq!(store.message_count().expect("count"), 0);
        store.insert(&make_msg("one")).expect("insert");
        store.insert(&make_msg("two")).expect("insert");
        assert_eq!(store.message_count().expect("count"), 2);
        std::fs::remove_dir_all(dir).ok();
    }
}
