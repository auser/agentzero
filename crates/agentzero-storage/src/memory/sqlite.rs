use crate::StorageKey;
use agentzero_core::{MemoryEntry, MemoryStore};
use anyhow::Context;
use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

const MEMORY_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS memory (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
)";

pub struct SqliteMemoryStore {
    conn: Mutex<Connection>,
}

impl SqliteMemoryStore {
    pub fn open(path: impl AsRef<Path>, key: Option<&StorageKey>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let conn = Connection::open(path)?;

        if let Some(k) = key {
            let hex_key = hex_encode_key(k);
            conn.execute_batch(&format!("PRAGMA key = \"x'{hex_key}'\""))?;

            // If the DB was previously plaintext, PRAGMA key will succeed but
            // subsequent reads will fail because SQLCipher tries to decrypt
            // plaintext pages. Detect this and auto-migrate.
            if path.exists()
                && fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false)
                && conn
                    .execute_batch("SELECT count(*) FROM sqlite_master")
                    .is_err()
            {
                drop(conn);
                migrate_plaintext_to_encrypted(path, k)?;
                let conn = Connection::open(path)?;
                conn.execute_batch(&format!("PRAGMA key = \"x'{hex_key}'\""))?;
                conn.execute(MEMORY_SCHEMA, [])
                    .context("failed to create memory table after migration")?;
                return Ok(Self {
                    conn: Mutex::new(conn),
                });
            }
        }

        conn.execute(MEMORY_SCHEMA, [])
            .context("failed to create memory table")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

fn hex_encode_key(key: &StorageKey) -> String {
    key.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// Migrate an existing plaintext SQLite database to SQLCipher-encrypted format.
///
/// Opens the plaintext DB (no PRAGMA key), attaches a new encrypted DB,
/// exports all data via `sqlcipher_export`, then swaps the files.
fn migrate_plaintext_to_encrypted(path: &Path, key: &StorageKey) -> anyhow::Result<()> {
    let hex_key = hex_encode_key(key);
    let tmp = path.with_extension("db.encrypting");

    let conn = Connection::open(path).context("failed to open plaintext DB for migration")?;
    conn.execute_batch(&format!(
        "ATTACH DATABASE '{}' AS encrypted KEY \"x'{hex_key}'\"; \
         SELECT sqlcipher_export('encrypted'); \
         DETACH DATABASE encrypted;",
        tmp.display()
    ))
    .context("failed to export plaintext DB to encrypted format")?;
    drop(conn);

    fs::rename(&tmp, path).context("failed to swap encrypted DB into place")?;
    Ok(())
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
    use crate::StorageKey;
    use agentzero_core::{MemoryEntry, MemoryStore};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path() -> PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "agentzero-memory-{}-{nanos}-{seq}.db",
            std::process::id()
        ))
    }

    fn test_key() -> StorageKey {
        StorageKey::from_config_dir(&temp_key_dir()).expect("key should be created")
    }

    fn temp_key_dir() -> PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-keydir-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp key dir should be created");
        dir
    }

    #[tokio::test]
    async fn sqlite_memory_append_and_recent_roundtrip_with_ordering() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("sqlite store should open");

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
        let result = SqliteMemoryStore::open(&dir, None);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn sqlite_memory_open_creates_missing_parent_dirs_success_path() {
        let base = std::env::temp_dir().join(format!(
            "agentzero-memory-parent-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after unix epoch")
                .as_nanos()
        ));
        let db_path = base.join("nested").join("agentzero.db");

        let store = SqliteMemoryStore::open(&db_path, None).expect("sqlite store should open");
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "hello".to_string(),
            })
            .await
            .expect("append should succeed");
        assert!(db_path.exists(), "db file should exist after open");

        fs::remove_file(&db_path).expect("test db should be removed");
        fs::remove_dir_all(base).expect("temp dirs should be removed");
    }

    #[tokio::test]
    async fn sqlite_encrypted_roundtrip() {
        let db_path = temp_db_path();
        let key = test_key();

        // Open encrypted, insert data
        {
            let store =
                SqliteMemoryStore::open(&db_path, Some(&key)).expect("encrypted store should open");
            store
                .append(MemoryEntry {
                    role: "user".to_string(),
                    content: "secret message".to_string(),
                })
                .await
                .expect("append should succeed");
        }

        // Reopen with same key, verify data
        {
            let store = SqliteMemoryStore::open(&db_path, Some(&key))
                .expect("encrypted store should reopen");
            let recent = store.recent(10).await.expect("recent should succeed");
            assert_eq!(recent.len(), 1);
            assert_eq!(recent[0].content, "secret message");
        }

        // Verify DB is not readable as plain SQLite
        {
            let conn = rusqlite::Connection::open(&db_path).expect("raw open should succeed");
            let result = conn.execute_batch("SELECT count(*) FROM sqlite_master");
            assert!(
                result.is_err(),
                "plain open without key should not read encrypted DB"
            );
        }

        fs::remove_file(db_path).expect("test db should be removed");
    }

    #[tokio::test]
    async fn sqlite_encrypted_rejects_wrong_key() {
        let db_path = temp_db_path();
        let key_a = test_key();

        // Create encrypted DB with key A
        {
            let store = SqliteMemoryStore::open(&db_path, Some(&key_a))
                .expect("store should open with key A");
            store
                .append(MemoryEntry {
                    role: "user".to_string(),
                    content: "data".to_string(),
                })
                .await
                .expect("append should succeed");
        }

        // Try to open with different key B — should fail
        let key_b = test_key();
        let result = SqliteMemoryStore::open(&db_path, Some(&key_b));
        assert!(
            result.is_err(),
            "opening encrypted DB with wrong key should fail"
        );

        fs::remove_file(db_path).expect("test db should be removed");
    }

    #[tokio::test]
    async fn sqlite_plaintext_migration_preserves_data() {
        let db_path = temp_db_path();

        // Create a plaintext DB (no encryption key)
        {
            let store =
                SqliteMemoryStore::open(&db_path, None).expect("plaintext store should open");
            store
                .append(MemoryEntry {
                    role: "user".to_string(),
                    content: "before migration".to_string(),
                })
                .await
                .expect("append should succeed");
            store
                .append(MemoryEntry {
                    role: "assistant".to_string(),
                    content: "also before migration".to_string(),
                })
                .await
                .expect("second append should succeed");
        }

        // Reopen with encryption key — should auto-migrate
        let key = test_key();
        {
            let store =
                SqliteMemoryStore::open(&db_path, Some(&key)).expect("migration should succeed");
            let recent = store.recent(10).await.expect("recent should succeed");
            assert_eq!(recent.len(), 2);
            assert_eq!(recent[0].content, "before migration");
            assert_eq!(recent[1].content, "also before migration");
        }

        // Verify DB is now encrypted (plain open can't read it)
        {
            let conn = rusqlite::Connection::open(&db_path).expect("raw open should succeed");
            let result = conn.execute_batch("SELECT count(*) FROM sqlite_master");
            assert!(
                result.is_err(),
                "migrated DB should not be readable without key"
            );
        }

        fs::remove_file(db_path).expect("test db should be removed");
    }

    #[tokio::test]
    async fn recent_zero_returns_empty_vec() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store should open");
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "data".to_string(),
            })
            .await
            .expect("append");
        let recent = store.recent(0).await.expect("recent(0) should succeed");
        assert!(recent.is_empty());
        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn large_limit_returns_all_entries() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store should open");
        for i in 0..5 {
            store
                .append(MemoryEntry {
                    role: "user".to_string(),
                    content: format!("msg-{i}"),
                })
                .await
                .expect("append");
        }
        let recent = store.recent(1000).await.expect("large limit");
        assert_eq!(recent.len(), 5);
        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn large_content_round_trips() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store should open");
        let big = "x".repeat(10_000);
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: big.clone(),
            })
            .await
            .expect("append big");
        let recent = store.recent(1).await.expect("recent");
        assert_eq!(recent[0].content, big);
        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn unicode_emoji_content_round_trips() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store should open");
        let content = "日本語テスト 🎉🚀 ñ à ü ö";
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: content.to_string(),
            })
            .await
            .expect("append unicode");
        let recent = store.recent(1).await.expect("recent");
        assert_eq!(recent[0].content, content);
        fs::remove_file(db_path).ok();
    }
}
