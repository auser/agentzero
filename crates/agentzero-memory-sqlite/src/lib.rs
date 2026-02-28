use agentzero_core::{MemoryEntry, MemoryStore};
use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

pub struct SqliteMemoryStore {
    conn: Mutex<Connection>,
}

impl SqliteMemoryStore {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
            [],
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

#[async_trait]
impl MemoryStore for SqliteMemoryStore {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute(
            "INSERT INTO memory(role, content) VALUES(?1, ?2)",
            params![entry.role, entry.content],
        )?;
        Ok(())
    }

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT role, content
             FROM memory
             ORDER BY id DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok(MemoryEntry {
                role: row.get(0)?,
                content: row.get(1)?,
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out.reverse();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteMemoryStore;
    use agentzero_core::{MemoryEntry, MemoryStore};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("agentzero-memory-{nanos}.db"))
    }

    #[tokio::test]
    async fn sqlite_memory_append_and_recent_roundtrip_with_ordering() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path).expect("sqlite store should open");

        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "first".to_string(),
            })
            .await
            .expect("first append should succeed");
        store
            .append(MemoryEntry {
                role: "assistant".to_string(),
                content: "second".to_string(),
            })
            .await
            .expect("second append should succeed");
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "third".to_string(),
            })
            .await
            .expect("third append should succeed");

        let recent = store.recent(2).await.expect("recent query should succeed");
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].content, "second");
        assert_eq!(recent[1].content, "third");

        fs::remove_file(db_path).expect("test db should be removed");
    }

    #[tokio::test]
    async fn sqlite_memory_open_fails_for_directory_path() {
        let dir = std::env::temp_dir();
        let result = SqliteMemoryStore::open(&dir);
        assert!(result.is_err());
    }
}
