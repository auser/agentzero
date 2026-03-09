use crate::StorageKey;
use agentzero_core::{MemoryEntry, MemoryStore};
use anyhow::Context;
use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

pub(crate) const MEMORY_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS memory (
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
                migrate_privacy_columns(&conn)?;
                migrate_conversation_column(&conn)?;
                migrate_ttl_column(&conn)?;
                return Ok(Self {
                    conn: Mutex::new(conn),
                });
            }
        }

        conn.execute(MEMORY_SCHEMA, [])
            .context("failed to create memory table")?;
        migrate_privacy_columns(&conn)?;
        migrate_conversation_column(&conn)?;
        migrate_ttl_column(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

/// Run all schema migrations (privacy columns + conversation_id + TTL).
#[cfg(feature = "pool")]
pub(crate) fn run_migrations(conn: &Connection) -> anyhow::Result<()> {
    migrate_privacy_columns(conn)?;
    migrate_conversation_column(conn)?;
    migrate_ttl_column(conn)?;
    Ok(())
}

/// Add privacy_boundary and source_channel columns if they don't exist yet.
/// SQLite doesn't support `ADD COLUMN IF NOT EXISTS`, so we catch the
/// "duplicate column" error and ignore it.
fn migrate_privacy_columns(conn: &Connection) -> anyhow::Result<()> {
    for sql in &[
        "ALTER TABLE memory ADD COLUMN privacy_boundary TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE memory ADD COLUMN source_channel TEXT DEFAULT NULL",
    ] {
        match conn.execute_batch(sql) {
            Ok(()) => {}
            Err(e) if e.to_string().contains("duplicate column") => {}
            Err(e) => return Err(e).context("failed to migrate memory table"),
        }
    }
    Ok(())
}

/// Add conversation_id column if it doesn't exist yet.
fn migrate_conversation_column(conn: &Connection) -> anyhow::Result<()> {
    match conn
        .execute_batch("ALTER TABLE memory ADD COLUMN conversation_id TEXT NOT NULL DEFAULT ''")
    {
        Ok(()) => {}
        Err(e) if e.to_string().contains("duplicate column") => {}
        Err(e) => return Err(e).context("failed to add conversation_id column"),
    }
    Ok(())
}

/// Add expires_at column for message TTL support.
fn migrate_ttl_column(conn: &Connection) -> anyhow::Result<()> {
    match conn.execute_batch("ALTER TABLE memory ADD COLUMN expires_at INTEGER DEFAULT NULL") {
        Ok(()) => {}
        Err(e) if e.to_string().contains("duplicate column") => {}
        Err(e) => return Err(e).context("failed to add expires_at column"),
    }
    Ok(())
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

/// Map a query row (columns 0–6) to a [`MemoryEntry`].
fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryEntry> {
    Ok(MemoryEntry {
        role: row.get(0)?,
        content: row.get(1)?,
        privacy_boundary: row.get::<_, String>(2).unwrap_or_default(),
        source_channel: row.get::<_, Option<String>>(3).unwrap_or_default(),
        conversation_id: row.get::<_, String>(4).unwrap_or_default(),
        created_at: row.get::<_, Option<String>>(5).ok().flatten(),
        expires_at: row.get::<_, Option<i64>>(6).unwrap_or_default(),
    })
}

#[async_trait]
impl MemoryStore for SqliteMemoryStore {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute(
            "INSERT INTO memory(role, content, privacy_boundary, source_channel, conversation_id, expires_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![entry.role, entry.content, entry.privacy_boundary, entry.source_channel, entry.conversation_id, entry.expires_at],
        )?;
        Ok(())
    }

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at
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
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at
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
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at
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
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute(
            "INSERT INTO memory(role, content, privacy_boundary, source_channel, conversation_id, expires_at)
             SELECT role, content, privacy_boundary, source_channel, ?2, expires_at
             FROM memory
             WHERE conversation_id = ?1
               AND (expires_at IS NULL OR expires_at > unixepoch())
             ORDER BY id",
            params![from_id, new_id],
        )?;
        Ok(())
    }

    async fn gc_expired(&self) -> anyhow::Result<u64> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let deleted = conn.execute(
            "DELETE FROM memory WHERE expires_at IS NOT NULL AND expires_at <= unixepoch()",
            [],
        )?;
        Ok(deleted as u64)
    }

    async fn list_conversations(&self) -> anyhow::Result<Vec<String>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
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
                ..Default::default()
            })
            .await
            .expect("first append should succeed");
        store
            .append(MemoryEntry {
                role: "assistant".to_string(),
                content: "second".to_string(),
                ..Default::default()
            })
            .await
            .expect("second append should succeed");
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "third".to_string(),
                ..Default::default()
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
                ..Default::default()
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
                    ..Default::default()
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
                    ..Default::default()
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
                    ..Default::default()
                })
                .await
                .expect("append should succeed");
            store
                .append(MemoryEntry {
                    role: "assistant".to_string(),
                    content: "also before migration".to_string(),
                    ..Default::default()
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
                ..Default::default()
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
                    ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
            })
            .await
            .expect("append unicode");
        let recent = store.recent(1).await.expect("recent");
        assert_eq!(recent[0].content, content);
        fs::remove_file(db_path).ok();
    }

    // --- Privacy boundary tests (Sprint 25, Phase 1) ---

    #[tokio::test]
    async fn append_with_boundary_roundtrips() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "local msg".to_string(),
                privacy_boundary: "local_only".to_string(),
                source_channel: Some("cli".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        let recent = store.recent(1).await.unwrap();
        assert_eq!(recent[0].privacy_boundary, "local_only");
        assert_eq!(recent[0].source_channel.as_deref(), Some("cli"));
        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn boundary_filtering_excludes_other_boundaries() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");

        // Insert local_only and any entries
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "local secret".to_string(),
                privacy_boundary: "local_only".to_string(),
                source_channel: None,
                ..Default::default()
            })
            .await
            .unwrap();
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "public msg".to_string(),
                privacy_boundary: "any".to_string(),
                source_channel: None,
                ..Default::default()
            })
            .await
            .unwrap();

        // Query with "any" boundary should NOT see local_only entries
        let filtered = store.recent_for_boundary(10, "any", None).await.unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].content, "public msg");

        // Query with "local_only" should see both
        let all = store
            .recent_for_boundary(10, "local_only", None)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn empty_boundary_visible_to_all() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "pre-migration entry".to_string(),
                privacy_boundary: String::new(),
                source_channel: None,
                ..Default::default()
            })
            .await
            .unwrap();

        for boundary in &["local_only", "encrypted_only", "any", ""] {
            let result = store.recent_for_boundary(10, boundary, None).await.unwrap();
            assert_eq!(
                result.len(),
                1,
                "empty-boundary entry should be visible to '{boundary}'"
            );
        }
        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn source_channel_filtering() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "from telegram".to_string(),
                privacy_boundary: String::new(),
                source_channel: Some("telegram".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "from cli".to_string(),
                privacy_boundary: String::new(),
                source_channel: Some("cli".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        let tg = store
            .recent_for_boundary(10, "", Some("telegram"))
            .await
            .unwrap();
        assert_eq!(tg.len(), 1);
        assert_eq!(tg[0].content, "from telegram");

        // None source_channel returns all
        let all = store.recent_for_boundary(10, "", None).await.unwrap();
        assert_eq!(all.len(), 2);

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn migration_is_idempotent() {
        let db_path = temp_db_path();
        // Open twice — the second open should re-run migration without error
        let store = SqliteMemoryStore::open(&db_path, None).expect("first open");
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "before re-migration".to_string(),
                privacy_boundary: "encrypted_only".to_string(),
                source_channel: None,
                ..Default::default()
            })
            .await
            .unwrap();
        drop(store);

        let store = SqliteMemoryStore::open(&db_path, None).expect("second open should succeed");
        let recent = store.recent(1).await.unwrap();
        assert_eq!(recent[0].content, "before re-migration");
        assert_eq!(recent[0].privacy_boundary, "encrypted_only");

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn existing_entries_get_empty_boundary_after_migration() {
        let db_path = temp_db_path();
        // Simulate pre-migration: create DB without privacy columns
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute(
                "CREATE TABLE IF NOT EXISTS memory (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch())
                )",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO memory(role, content) VALUES(?1, ?2)",
                rusqlite::params!["user", "old data"],
            )
            .unwrap();
        }

        // Open via SqliteMemoryStore — should auto-migrate and read old data
        let store = SqliteMemoryStore::open(&db_path, None).expect("migration open");
        let recent = store.recent(1).await.unwrap();
        assert_eq!(recent[0].content, "old data");
        assert_eq!(recent[0].privacy_boundary, "");
        assert!(recent[0].source_channel.is_none());

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn encrypted_only_visible_to_local_and_encrypted() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "enc msg".to_string(),
                privacy_boundary: "encrypted_only".to_string(),
                source_channel: None,
                ..Default::default()
            })
            .await
            .unwrap();

        // encrypted_only should see it
        let enc = store
            .recent_for_boundary(10, "encrypted_only", None)
            .await
            .unwrap();
        assert_eq!(enc.len(), 1);

        // local_only should see it (stricter can see less-strict)
        let local = store
            .recent_for_boundary(10, "local_only", None)
            .await
            .unwrap();
        assert_eq!(local.len(), 1);

        // any should NOT see it
        let any = store.recent_for_boundary(10, "any", None).await.unwrap();
        assert_eq!(any.len(), 0);

        fs::remove_file(db_path).ok();
    }

    // --- TTL / expires_at tests ---

    #[tokio::test]
    async fn expired_entries_excluded_from_recent() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");

        // Insert a permanent entry.
        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "permanent".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        // Insert an already-expired entry.
        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "expired".into(),
                expires_at: Some(1), // 1970-01-01 — definitely expired
                ..Default::default()
            })
            .await
            .unwrap();

        let recent = store.recent(10).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].content, "permanent");

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn future_ttl_entry_is_visible() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");

        let far_future = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 86400;

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "not yet expired".into(),
                expires_at: Some(far_future),
                ..Default::default()
            })
            .await
            .unwrap();

        let recent = store.recent(10).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].content, "not yet expired");
        assert_eq!(recent[0].expires_at, Some(far_future));

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn gc_expired_removes_expired_keeps_permanent() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "permanent".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "expired".into(),
                expires_at: Some(1),
                ..Default::default()
            })
            .await
            .unwrap();

        let deleted = store.gc_expired().await.unwrap();
        assert_eq!(deleted, 1);

        // Force a raw query to verify the row is actually gone (not just filtered).
        let conn = store.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM memory", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn expired_entries_excluded_from_conversation_query() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "visible".into(),
                conversation_id: "conv-1".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "gone".into(),
                conversation_id: "conv-1".into(),
                expires_at: Some(1),
                ..Default::default()
            })
            .await
            .unwrap();

        let entries = store.recent_for_conversation("conv-1", 10).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "visible");

        fs::remove_file(db_path).ok();
    }
}
