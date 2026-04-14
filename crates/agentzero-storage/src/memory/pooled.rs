use crate::StorageKey;
use agentzero_core::{MemoryEntry, MemoryStore};
use anyhow::Context;
use async_trait::async_trait;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::fs;
use std::path::Path;

use super::sqlite::{run_migrations, MEMORY_SCHEMA};

/// Connection-pooled SQLite memory store.
///
/// Uses `r2d2` to manage a pool of connections, eliminating `Mutex` contention
/// under concurrent requests. Each method borrows a connection from the pool
/// for the duration of the query.
pub struct PooledMemoryStore {
    pool: Pool<SqliteConnectionManager>,
}

impl PooledMemoryStore {
    /// Open a pooled memory store at `path` with the given pool size.
    ///
    /// `pool_size` is clamped to 1..=16. Encryption key (SQLCipher) is applied
    /// via a connection customizer that runs `PRAGMA key` on each new connection.
    pub fn open(
        path: impl AsRef<Path>,
        key: Option<&StorageKey>,
        pool_size: u32,
    ) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let pool_size = pool_size.clamp(1, 16);
        let manager = SqliteConnectionManager::file(path);

        let hex_key = key.map(hex_encode_key);

        let pool = Pool::builder()
            .max_size(pool_size)
            .connection_customizer(Box::new(ConnectionInit {
                hex_key: hex_key.clone(),
            }))
            .build(manager)
            .context("failed to build r2d2 connection pool")?;

        // Run schema migrations on a single connection at startup.
        {
            let conn = pool.get().context("failed to get init connection")?;
            if let Err(e) = conn.execute(MEMORY_SCHEMA, []) {
                if is_key_mismatch(&e) {
                    agentzero_core::tracing::warn!(
                        path = %path.display(),
                        "memory database encrypted with a different key; \
                         recreating (conversation history will be lost)"
                    );
                    drop(conn);
                    drop(pool);
                    fs::remove_file(path).context("failed to remove stale memory database")?;
                    return PooledMemoryStore::open(path, key, pool_size);
                }
                return Err(map_db_open_error(e, path)).context("failed to create memory table");
            }
            run_migrations(&conn)?;
        }

        Ok(Self { pool })
    }
}

fn hex_encode_key(key: &StorageKey) -> String {
    key.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

fn is_key_mismatch(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _)
            if f.code == rusqlite::ffi::ErrorCode::NotADatabase
    )
}

fn map_db_open_error(e: rusqlite::Error, path: &Path) -> anyhow::Error {
    if is_key_mismatch(&e) {
        return anyhow::anyhow!(
            "database at '{}' is encrypted with a different key; \
             delete the file to create a fresh database",
            path.display()
        );
    }
    anyhow::Error::from(e)
}

/// r2d2 connection customizer: runs `PRAGMA key` on each new connection.
#[derive(Debug)]
struct ConnectionInit {
    hex_key: Option<String>,
}

impl r2d2::CustomizeConnection<rusqlite::Connection, rusqlite::Error> for ConnectionInit {
    fn on_acquire(&self, conn: &mut rusqlite::Connection) -> Result<(), rusqlite::Error> {
        if let Some(ref hex_key) = self.hex_key {
            conn.execute_batch(&format!("PRAGMA key = \"x'{hex_key}'\""))?;
        }
        // Enable WAL mode for better concurrent read performance.
        conn.execute_batch("PRAGMA journal_mode = WAL")?;
        Ok(())
    }
}

/// Map a query row (columns 0–9) to a [`MemoryEntry`].
fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryEntry> {
    let embedding: Option<Vec<f32>> = row
        .get::<_, Option<Vec<u8>>>(9)
        .unwrap_or_default()
        .map(|bytes| agentzero_core::embedding::bytes_to_embedding(&bytes));

    Ok(MemoryEntry {
        role: row.get(0)?,
        content: row.get(1)?,
        privacy_boundary: row.get::<_, String>(2).unwrap_or_default(),
        source_channel: row.get::<_, Option<String>>(3).unwrap_or_default(),
        conversation_id: row.get::<_, String>(4).unwrap_or_default(),
        created_at: row.get::<_, Option<String>>(5).ok().flatten(),
        expires_at: row.get::<_, Option<i64>>(6).unwrap_or_default(),
        org_id: row.get::<_, String>(7).unwrap_or_default(),
        agent_id: row.get::<_, String>(8).unwrap_or_default(),
        embedding,
        content_hash: String::new(),
    })
}

#[async_trait]
impl MemoryStore for PooledMemoryStore {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        let conn = self.pool.get().context("pool: failed to get connection")?;
        conn.execute(
            "INSERT INTO memory(role, content, privacy_boundary, source_channel, conversation_id, expires_at, org_id, agent_id) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![entry.role, entry.content, entry.privacy_boundary, entry.source_channel, entry.conversation_id, entry.expires_at, entry.org_id, entry.agent_id],
        )?;
        Ok(())
    }

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.pool.get().context("pool: failed to get connection")?;
        let mut stmt = conn.prepare(
            "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id, embedding
             FROM memory
             WHERE expires_at IS NULL OR expires_at > unixepoch()
             ORDER BY id DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], row_to_entry)?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out.reverse();
        Ok(out)
    }

    async fn recent_for_boundary(
        &self,
        limit: usize,
        boundary: &str,
        source_channel: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.pool.get().context("pool: failed to get connection")?;
        let mut stmt = conn.prepare(
            "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id, embedding
             FROM memory
             WHERE (expires_at IS NULL OR expires_at > unixepoch())
               AND (privacy_boundary = '' OR privacy_boundary = ?1
                    OR privacy_boundary IN ('any', 'inherit')
                    OR (?1 = 'local_only' AND privacy_boundary = 'encrypted_only'))
               AND (?2 IS NULL OR source_channel IS NULL OR source_channel = ?2)
             ORDER BY id DESC
             LIMIT ?3",
        )?;
        let boundary_owned = boundary.to_string();
        let source_owned = source_channel.map(|s| s.to_string());
        let rows = stmt.query_map(
            params![boundary_owned, source_owned, limit as i64],
            row_to_entry,
        )?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out.reverse();
        Ok(out)
    }

    async fn recent_for_conversation(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.pool.get().context("pool: failed to get connection")?;
        let mut stmt = conn.prepare(
            "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id, embedding
             FROM memory
             WHERE conversation_id = ?1
               AND (expires_at IS NULL OR expires_at > unixepoch())
             ORDER BY id DESC
             LIMIT ?2",
        )?;
        let cid = conversation_id.to_string();
        let rows = stmt.query_map(params![cid, limit as i64], row_to_entry)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out.reverse();
        Ok(out)
    }

    async fn fork_conversation(&self, from_id: &str, new_id: &str) -> anyhow::Result<()> {
        let conn = self.pool.get().context("pool: failed to get connection")?;
        conn.execute(
            "INSERT INTO memory(role, content, privacy_boundary, source_channel, conversation_id, expires_at, org_id, agent_id, embedding)
             SELECT role, content, privacy_boundary, source_channel, ?2, expires_at, org_id, agent_id, embedding
             FROM memory
             WHERE conversation_id = ?1
               AND (expires_at IS NULL OR expires_at > unixepoch())
             ORDER BY id",
            params![from_id, new_id],
        )?;
        Ok(())
    }

    async fn gc_expired(&self) -> anyhow::Result<u64> {
        let conn = self.pool.get().context("pool: failed to get connection")?;
        let deleted = conn.execute(
            "DELETE FROM memory WHERE expires_at IS NOT NULL AND expires_at <= unixepoch()",
            [],
        )?;
        Ok(deleted as u64)
    }

    async fn recent_for_timerange(
        &self,
        since: Option<i64>,
        until: Option<i64>,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.pool.get().context("pool: failed to get connection")?;
        let sql = format!(
            "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id, embedding
             FROM memory
             WHERE (expires_at IS NULL OR expires_at > unixepoch())
               {}
               {}
             ORDER BY id DESC
             LIMIT ?1",
            if since.is_some() { "AND created_at >= ?2" } else { "" },
            if until.is_some() { if since.is_some() { "AND created_at <= ?3" } else { "AND created_at <= ?2" } } else { "" },
        );
        let mut stmt = conn.prepare(&sql)?;

        let mut out = Vec::new();
        match (since, until) {
            (Some(s), Some(u)) => {
                let rows = stmt.query_map(params![limit as i64, s, u], row_to_entry)?;
                for row in rows {
                    out.push(row?);
                }
            }
            (Some(s), None) => {
                let rows = stmt.query_map(params![limit as i64, s], row_to_entry)?;
                for row in rows {
                    out.push(row?);
                }
            }
            (None, Some(u)) => {
                let rows = stmt.query_map(params![limit as i64, u], row_to_entry)?;
                for row in rows {
                    out.push(row?);
                }
            }
            (None, None) => {
                let rows = stmt.query_map(params![limit as i64], row_to_entry)?;
                for row in rows {
                    out.push(row?);
                }
            }
        }
        out.reverse();
        Ok(out)
    }

    async fn list_conversations(&self) -> anyhow::Result<Vec<String>> {
        let conn = self.pool.get().context("pool: failed to get connection")?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT conversation_id FROM memory WHERE conversation_id != '' ORDER BY conversation_id",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::PooledMemoryStore;
    use agentzero_core::{MemoryEntry, MemoryStore};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path() -> PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "agentzero-pooled-{}-{nanos}-{seq}.db",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn pooled_append_and_recent_roundtrip() {
        let db_path = temp_db_path();
        let store = PooledMemoryStore::open(&db_path, None, 4).expect("pooled store should open");

        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "first".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        store
            .append(MemoryEntry {
                role: "assistant".to_string(),
                content: "second".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        let recent = store.recent(2).await.unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].content, "first");
        assert_eq!(recent[1].content, "second");

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn pooled_concurrent_writes() {
        let db_path = temp_db_path();
        let store = std::sync::Arc::new(
            PooledMemoryStore::open(&db_path, None, 4).expect("pooled store should open"),
        );

        let mut handles = Vec::new();
        for i in 0..10 {
            let s = store.clone();
            handles.push(tokio::spawn(async move {
                s.append(MemoryEntry {
                    role: "user".to_string(),
                    content: format!("msg-{i}"),
                    ..Default::default()
                })
                .await
                .expect("concurrent append should succeed");
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        let recent = store.recent(100).await.unwrap();
        assert_eq!(recent.len(), 10);

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn pooled_conversation_roundtrip() {
        let db_path = temp_db_path();
        let store = PooledMemoryStore::open(&db_path, None, 2).expect("pooled store should open");

        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "conv msg".to_string(),
                conversation_id: "run-123".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        let entries = store.recent_for_conversation("run-123", 10).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "conv msg");

        let empty = store.recent_for_conversation("run-999", 10).await.unwrap();
        assert!(empty.is_empty());

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn pooled_pool_size_clamped() {
        let db_path = temp_db_path();
        // Size 0 gets clamped to 1, size 100 gets clamped to 16.
        let store = PooledMemoryStore::open(&db_path, None, 0).expect("pool_size=0 should open");
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "ok".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        fs::remove_file(db_path).ok();
    }
}
