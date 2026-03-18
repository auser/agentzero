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

        #[cfg(feature = "storage-encrypted")]
        if let Some(k) = key {
            let hex_key = hex_encode_key(k);
            conn.execute_batch(&format!("PRAGMA key = \"x'{hex_key}'\""))?;

            // If the DB was previously plaintext, PRAGMA key will succeed but
            // subsequent reads will fail because SQLCipher tries to decrypt
            // plaintext pages. Detect this and auto-migrate.
            //
            // We distinguish "plaintext DB" from "encrypted with a different
            // key" by checking the file header: an unencrypted SQLite DB
            // starts with "SQLite format 3\0".  An encrypted DB has random
            // bytes in the header, so we should NOT attempt migration for it.
            if path.exists()
                && fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false)
                && is_plaintext_sqlite(path)
                && conn
                    .execute_batch("SELECT count(*) FROM sqlite_master")
                    .is_err()
            {
                drop(conn);
                migrate_plaintext_to_encrypted(path, k)?;
                let conn = Connection::open(path)?;
                conn.execute_batch(&format!("PRAGMA key = \"x'{hex_key}'\""))?;
                conn.execute(MEMORY_SCHEMA, [])
                    .map_err(|e| map_db_open_error(e, path))
                    .context("failed to create memory table after migration")?;
                run_versioned_migrations(&conn)?;
                return Ok(Self {
                    conn: Mutex::new(conn),
                });
            }
        }

        #[cfg(not(feature = "storage-encrypted"))]
        let _ = key; // suppress unused warning for plain SQLite builds

        if let Err(e) = conn.execute(MEMORY_SCHEMA, []) {
            if is_key_mismatch(&e) {
                // The DB was encrypted with a different key. The conversation
                // history is inaccessible, so recreate the file automatically
                // rather than crashing the agent.
                agentzero_core::tracing::warn!(
                    path = %path.display(),
                    "memory database encrypted with a different key; \
                     recreating (conversation history will be lost)"
                );
                drop(conn);
                fs::remove_file(path).context("failed to remove stale memory database")?;
                return SqliteMemoryStore::open(path, key);
            }
            return Err(map_db_open_error(e, path)).context("failed to create memory table");
        }
        run_versioned_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

// ---------------------------------------------------------------------------
// Schema version tracking and migrations
// ---------------------------------------------------------------------------

/// Schema version table. Created automatically on first migration run.
const SCHEMA_VERSION_TABLE: &str = "CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    description TEXT NOT NULL,
    applied_at INTEGER NOT NULL DEFAULT (unixepoch())
)";

/// Migration entry: version number, description, and SQL statements.
struct Migration {
    version: u32,
    description: &'static str,
    statements: &'static [&'static str],
}

/// Ordered list of all migrations. Append-only — never remove or reorder entries.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "add privacy_boundary and source_channel columns",
        statements: &[
            "ALTER TABLE memory ADD COLUMN privacy_boundary TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE memory ADD COLUMN source_channel TEXT DEFAULT NULL",
        ],
    },
    Migration {
        version: 2,
        description: "add conversation_id column",
        statements: &["ALTER TABLE memory ADD COLUMN conversation_id TEXT NOT NULL DEFAULT ''"],
    },
    Migration {
        version: 3,
        description: "add expires_at column for message TTL",
        statements: &["ALTER TABLE memory ADD COLUMN expires_at INTEGER DEFAULT NULL"],
    },
    Migration {
        version: 4,
        description: "add org_id column for multi-tenancy isolation",
        statements: &["ALTER TABLE memory ADD COLUMN org_id TEXT NOT NULL DEFAULT ''"],
    },
    Migration {
        version: 5,
        description: "add agent_id column for per-agent memory isolation",
        statements: &["ALTER TABLE memory ADD COLUMN agent_id TEXT NOT NULL DEFAULT ''"],
    },
];

/// Run all pending migrations against the connection. Creates the version table
/// if it doesn't exist, then applies each migration that hasn't been recorded yet.
///
/// Backward-compatible: on databases that already have the columns (from the old
/// idempotent approach), the "duplicate column" error is silently ignored and the
/// version is recorded as applied.
pub(crate) fn run_versioned_migrations(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(SCHEMA_VERSION_TABLE)
        .context("failed to create schema_version table")?;

    let current_version: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for migration in MIGRATIONS {
        if migration.version <= current_version {
            continue;
        }
        for sql in migration.statements {
            match conn.execute_batch(sql) {
                Ok(()) => {}
                // Backward-compat: column already exists from pre-versioned migration.
                Err(e) if e.to_string().contains("duplicate column") => {}
                Err(e) => {
                    return Err(e).context(format!(
                        "migration v{} failed: {}",
                        migration.version, migration.description
                    ))
                }
            }
        }
        conn.execute(
            "INSERT INTO schema_version (version, description) VALUES (?1, ?2)",
            params![migration.version, migration.description],
        )
        .with_context(|| {
            format!(
                "failed to record migration v{}: {}",
                migration.version, migration.description
            )
        })?;
    }

    Ok(())
}

/// Run all schema migrations. Public for pool feature.
#[cfg(feature = "pool")]
pub(crate) fn run_migrations(conn: &Connection) -> anyhow::Result<()> {
    run_versioned_migrations(conn)
}

/// Current schema version (max version in MIGRATIONS).
#[cfg(test)]
fn current_schema_version() -> u32 {
    MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
}

#[cfg(feature = "storage-encrypted")]
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

/// Map a rusqlite error to a human-friendly message when the database is
/// encrypted with a different key (SQLITE_NOTADB, code 26).
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

/// Returns `true` if the file at `path` starts with the SQLite magic header,
/// indicating it is an unencrypted (plaintext) SQLite database.
/// Encrypted (SQLCipher) databases have random bytes in the header.
#[cfg(feature = "storage-encrypted")]
fn is_plaintext_sqlite(path: &Path) -> bool {
    const SQLITE_MAGIC: &[u8] = b"SQLite format 3\0";
    let Ok(mut f) = fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 16];
    use std::io::Read;
    if f.read_exact(&mut buf).is_err() {
        return false;
    }
    buf == *SQLITE_MAGIC
}

/// Migrate an existing plaintext SQLite database to SQLCipher-encrypted format.
///
/// Opens the plaintext DB (no PRAGMA key), attaches a new encrypted DB,
/// exports all data via `sqlcipher_export`, then swaps the files.
///
/// If the export fails (e.g. `sqlcipher_export` is not available or the DB is
/// corrupt), falls back to deleting the plaintext file when it contains no
/// conversation data worth preserving.
#[cfg(feature = "storage-encrypted")]
fn migrate_plaintext_to_encrypted(path: &Path, key: &StorageKey) -> anyhow::Result<()> {
    let hex_key = hex_encode_key(key);
    let tmp = path.with_extension("db.encrypting");

    let conn = Connection::open(path).context("failed to open plaintext DB for migration")?;
    let export_result = conn.execute_batch(&format!(
        "ATTACH DATABASE '{}' AS encrypted KEY \"x'{hex_key}'\"; \
         SELECT sqlcipher_export('encrypted'); \
         DETACH DATABASE encrypted;",
        tmp.display()
    ));

    match export_result {
        Ok(()) => {
            drop(conn);
            fs::rename(&tmp, path).context("failed to swap encrypted DB into place")?;
        }
        Err(export_err) => {
            // Check if the DB has any rows worth preserving.
            let row_count: i64 = conn
                .query_row("SELECT count(*) FROM memory", [], |r| r.get(0))
                .unwrap_or(0);
            drop(conn);
            let _ = fs::remove_file(&tmp); // clean up partial temp file

            if row_count == 0 {
                // Empty DB — just delete it so a fresh encrypted one is created.
                fs::remove_file(path)
                    .context("failed to remove empty plaintext DB for re-creation")?;
            } else {
                return Err(export_err)
                    .context("failed to export plaintext DB to encrypted format");
            }
        }
    }

    Ok(())
}

/// Map a query row (columns 0–8) to a [`MemoryEntry`].
fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryEntry> {
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
    })
}

#[async_trait]
impl MemoryStore for SqliteMemoryStore {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute(
            "INSERT INTO memory(role, content, privacy_boundary, source_channel, conversation_id, expires_at, org_id, agent_id) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![entry.role, entry.content, entry.privacy_boundary, entry.source_channel, entry.conversation_id, entry.expires_at, entry.org_id, entry.agent_id],
        )?;
        Ok(())
    }

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id, agent_id
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
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id
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
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id
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
            "INSERT INTO memory(role, content, privacy_boundary, source_channel, conversation_id, expires_at, org_id, agent_id)
             SELECT role, content, privacy_boundary, source_channel, ?2, expires_at, org_id, agent_id
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

    async fn recent_for_org(&self, org_id: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id
             FROM memory
             WHERE org_id = ?1
               AND (expires_at IS NULL OR expires_at > unixepoch())
             ORDER BY id DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![org_id, limit as i64], row_to_entry)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out.reverse();
        Ok(out)
    }

    async fn recent_for_org_conversation(
        &self,
        org_id: &str,
        conversation_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id
             FROM memory
             WHERE org_id = ?1 AND conversation_id = ?2
               AND (expires_at IS NULL OR expires_at > unixepoch())
             ORDER BY id DESC
             LIMIT ?3",
        )?;
        let oid = org_id.to_string();
        let cid = conversation_id.to_string();
        let rows = stmt.query_map(params![oid, cid, limit as i64], row_to_entry)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out.reverse();
        Ok(out)
    }

    async fn list_conversations_for_org(&self, org_id: &str) -> anyhow::Result<Vec<String>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT DISTINCT conversation_id FROM memory
             WHERE conversation_id != '' AND org_id = ?1
             ORDER BY conversation_id",
        )?;
        let rows = stmt.query_map(params![org_id], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
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

    async fn recent_for_agent(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id
             FROM memory
             WHERE agent_id = ?1
               AND (expires_at IS NULL OR expires_at > unixepoch())
             ORDER BY id DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![agent_id, limit as i64], row_to_entry)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out.reverse();
        Ok(out)
    }

    async fn recent_for_agent_conversation(
        &self,
        agent_id: &str,
        conversation_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                    datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id
             FROM memory
             WHERE agent_id = ?1 AND conversation_id = ?2
               AND (expires_at IS NULL OR expires_at > unixepoch())
             ORDER BY id DESC
             LIMIT ?3",
        )?;
        let aid = agent_id.to_string();
        let cid = conversation_id.to_string();
        let rows = stmt.query_map(params![aid, cid, limit as i64], row_to_entry)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out.reverse();
        Ok(out)
    }

    async fn list_conversations_for_agent(&self, agent_id: &str) -> anyhow::Result<Vec<String>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT DISTINCT conversation_id FROM memory
             WHERE conversation_id != '' AND agent_id = ?1
             ORDER BY conversation_id",
        )?;
        let rows = stmt.query_map(params![agent_id], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{current_schema_version, SqliteMemoryStore, MEMORY_SCHEMA};
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
    #[cfg(feature = "storage-encrypted")]
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
    #[cfg(feature = "storage-encrypted")]
    async fn sqlite_encrypted_wrong_key_recreates_db() {
        use crate::crypto::StorageKey;

        let db_path = temp_db_path();
        let key_a = StorageKey::from_bytes([0xAA; 32]);
        let key_b = StorageKey::from_bytes([0xBB; 32]);

        // Create encrypted DB with key A and store data
        {
            let store = SqliteMemoryStore::open(&db_path, Some(&key_a))
                .expect("store should open with key A");
            store
                .append(MemoryEntry {
                    role: "user".to_string(),
                    content: "secret data".to_string(),
                    ..Default::default()
                })
                .await
                .expect("append should succeed");
        }

        // Open with different key B — DB auto-recreated (production behavior)
        let store_b = SqliteMemoryStore::open(&db_path, Some(&key_b))
            .expect("open with wrong key should auto-recreate");

        // Data from key A should be gone
        let entries = store_b
            .recent(100)
            .await
            .expect("recent should succeed on recreated DB");
        assert!(
            entries.is_empty(),
            "data encrypted with key A should not be accessible after recreate"
        );

        fs::remove_file(db_path).expect("test db should be removed");
    }

    #[tokio::test]
    #[cfg(feature = "storage-encrypted")]
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
    async fn schema_version_table_created_on_open() {
        let db_path = temp_db_path();
        let _store = SqliteMemoryStore::open(&db_path, None).expect("open");

        // Verify the schema_version table exists and has entries.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let version: u32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, current_schema_version());

        // Verify all migration versions are recorded.
        let count: u32 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, current_schema_version());

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn schema_version_survives_reopen() {
        let db_path = temp_db_path();
        {
            let _store = SqliteMemoryStore::open(&db_path, None).expect("first open");
        }
        // Second open should not fail and should not duplicate version entries.
        {
            let _store = SqliteMemoryStore::open(&db_path, None).expect("second open");
        }

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let count: u32 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .unwrap();
        // Still exactly 3 migrations (no duplicates).
        assert_eq!(count, current_schema_version());

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn pre_versioned_db_gets_version_table_on_upgrade() {
        let db_path = temp_db_path();
        // Simulate a pre-versioned DB: has all columns but no schema_version table.
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(MEMORY_SCHEMA).unwrap();
            conn.execute_batch(
                "ALTER TABLE memory ADD COLUMN privacy_boundary TEXT NOT NULL DEFAULT ''",
            )
            .unwrap();
            conn.execute_batch("ALTER TABLE memory ADD COLUMN source_channel TEXT DEFAULT NULL")
                .unwrap();
            conn.execute_batch(
                "ALTER TABLE memory ADD COLUMN conversation_id TEXT NOT NULL DEFAULT ''",
            )
            .unwrap();
            conn.execute_batch("ALTER TABLE memory ADD COLUMN expires_at INTEGER DEFAULT NULL")
                .unwrap();
        }

        // Open via SqliteMemoryStore — should create version table and record all as applied.
        let _store = SqliteMemoryStore::open(&db_path, None).expect("upgrade open");

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let version: u32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, current_schema_version());

        fs::remove_file(db_path).ok();
    }

    #[test]
    fn current_schema_version_equals_migration_count() {
        assert_eq!(current_schema_version(), 5);
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

    // --- Org isolation tests (Phase F) ---

    #[tokio::test]
    async fn org_scoped_recent_filters_by_org() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "org-a msg".into(),
                org_id: "org-a".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "org-b msg".into(),
                org_id: "org-b".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        let org_a = store.recent_for_org("org-a", 10).await.unwrap();
        assert_eq!(org_a.len(), 1);
        assert_eq!(org_a[0].content, "org-a msg");

        let org_b = store.recent_for_org("org-b", 10).await.unwrap();
        assert_eq!(org_b.len(), 1);
        assert_eq!(org_b[0].content, "org-b msg");

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn org_scoped_conversation_isolates_transcripts() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "org-a conv".into(),
                org_id: "org-a".into(),
                conversation_id: "conv-1".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "org-b conv".into(),
                org_id: "org-b".into(),
                conversation_id: "conv-1".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        // Same conversation_id but different org — isolated
        let org_a = store
            .recent_for_org_conversation("org-a", "conv-1", 10)
            .await
            .unwrap();
        assert_eq!(org_a.len(), 1);
        assert_eq!(org_a[0].content, "org-a conv");

        let org_b = store
            .recent_for_org_conversation("org-b", "conv-1", 10)
            .await
            .unwrap();
        assert_eq!(org_b.len(), 1);
        assert_eq!(org_b[0].content, "org-b conv");

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn list_conversations_for_org_filters_correctly() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "a".into(),
                org_id: "org-a".into(),
                conversation_id: "conv-1".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "b".into(),
                org_id: "org-b".into(),
                conversation_id: "conv-2".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        let a_convs = store.list_conversations_for_org("org-a").await.unwrap();
        assert_eq!(a_convs, vec!["conv-1"]);

        let b_convs = store.list_conversations_for_org("org-b").await.unwrap();
        assert_eq!(b_convs, vec!["conv-2"]);

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn org_id_persists_through_roundtrip() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "test".into(),
                org_id: "acme-corp".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        let recent = store.recent(1).await.unwrap();
        assert_eq!(recent[0].org_id, "acme-corp");

        fs::remove_file(db_path).ok();
    }

    // --- Per-agent memory isolation tests (Sprint 43, Phase F) ---

    #[tokio::test]
    async fn agent_scoped_recent_filters_by_agent() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "agent-a msg".into(),
                agent_id: "agent-a".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "agent-b msg".into(),
                agent_id: "agent-b".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        let agent_a = store.recent_for_agent("agent-a", 10).await.unwrap();
        assert_eq!(agent_a.len(), 1);
        assert_eq!(agent_a[0].content, "agent-a msg");

        let agent_b = store.recent_for_agent("agent-b", 10).await.unwrap();
        assert_eq!(agent_b.len(), 1);
        assert_eq!(agent_b[0].content, "agent-b msg");

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn agent_scoped_conversation_isolates_transcripts() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "agent-a conv".into(),
                agent_id: "agent-a".into(),
                conversation_id: "conv-1".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "agent-b conv".into(),
                agent_id: "agent-b".into(),
                conversation_id: "conv-1".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        // Same conversation_id but different agent — isolated
        let agent_a = store
            .recent_for_agent_conversation("agent-a", "conv-1", 10)
            .await
            .unwrap();
        assert_eq!(agent_a.len(), 1);
        assert_eq!(agent_a[0].content, "agent-a conv");

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn agent_id_persists_through_roundtrip() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "test".into(),
                agent_id: "agent-researcher".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        let recent = store.recent(1).await.unwrap();
        assert_eq!(recent[0].agent_id, "agent-researcher");

        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn list_conversations_for_agent_filters_correctly() {
        let db_path = temp_db_path();
        let store = SqliteMemoryStore::open(&db_path, None).expect("store");

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "a".into(),
                agent_id: "agent-a".into(),
                conversation_id: "conv-1".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        store
            .append(MemoryEntry {
                role: "user".into(),
                content: "b".into(),
                agent_id: "agent-b".into(),
                conversation_id: "conv-2".into(),
                ..Default::default()
            })
            .await
            .unwrap();

        let a_convs = store.list_conversations_for_agent("agent-a").await.unwrap();
        assert_eq!(a_convs, vec!["conv-1"]);

        let b_convs = store.list_conversations_for_agent("agent-b").await.unwrap();
        assert_eq!(b_convs, vec!["conv-2"]);

        fs::remove_file(db_path).ok();
    }
}
