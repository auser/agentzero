use agentzero_core::{MemoryEntry, MemoryStore};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use std::fmt::{Debug, Formatter};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct SecretToken(String);

impl SecretToken {
    pub fn new(value: String) -> anyhow::Result<Self> {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            return Err(anyhow!("TURSO_AUTH_TOKEN cannot be empty"));
        }
        if trimmed.chars().any(char::is_whitespace) {
            return Err(anyhow!("TURSO_AUTH_TOKEN must not contain whitespace"));
        }
        Ok(Self(trimmed))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Debug for SecretToken {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

pub struct TursoSettings {
    pub database_url: String,
    pub auth_token: SecretToken,
}

impl TursoSettings {
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = std::env::var("TURSO_DATABASE_URL")
            .context("missing TURSO_DATABASE_URL for Turso memory backend")?;
        let auth_token_raw = std::env::var("TURSO_AUTH_TOKEN")
            .context("missing TURSO_AUTH_TOKEN for Turso memory backend")?;
        let auth_token = SecretToken::new(auth_token_raw)?;

        if !(database_url.starts_with("libsql://") || database_url.starts_with("https://")) {
            return Err(anyhow!(
                "invalid TURSO_DATABASE_URL: must start with libsql:// or https://"
            ));
        }
        if database_url.starts_with("libsql://") && database_url.contains("?tls=0") {
            return Err(anyhow!(
                "invalid TURSO_DATABASE_URL: TLS must remain enabled for libsql:// connections"
            ));
        }

        Ok(Self {
            database_url,
            auth_token,
        })
    }

    pub fn sanitized_connection_metadata(&self) -> String {
        let without_query = self.database_url.split('?').next().unwrap_or("");
        without_query.to_string()
    }
}

// ---------------------------------------------------------------------------
// Migration framework for Turso (async variant of SQLite migration system)
// ---------------------------------------------------------------------------

struct TursoMigration {
    version: u32,
    description: &'static str,
    statements: &'static [&'static str],
}

/// Ordered list of all Turso migrations. Append-only — never remove or reorder.
const TURSO_MIGRATIONS: &[TursoMigration] = &[
    TursoMigration {
        version: 1,
        description: "add privacy_boundary and source_channel columns",
        statements: &[
            "ALTER TABLE memory ADD COLUMN privacy_boundary TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE memory ADD COLUMN source_channel TEXT DEFAULT NULL",
        ],
    },
    TursoMigration {
        version: 2,
        description: "add conversation_id column",
        statements: &["ALTER TABLE memory ADD COLUMN conversation_id TEXT NOT NULL DEFAULT ''"],
    },
    TursoMigration {
        version: 3,
        description: "add expires_at column for message TTL",
        statements: &["ALTER TABLE memory ADD COLUMN expires_at INTEGER DEFAULT NULL"],
    },
    TursoMigration {
        version: 4,
        description: "add org_id column for multi-tenancy isolation",
        statements: &["ALTER TABLE memory ADD COLUMN org_id TEXT NOT NULL DEFAULT ''"],
    },
];

/// Run all pending migrations against a Turso connection.
async fn run_turso_migrations(conn: &libsql::Connection) -> anyhow::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at INTEGER NOT NULL DEFAULT (unixepoch())
        )",
        (),
    )
    .await
    .context("failed to create schema_version table in Turso")?;

    let current_version: u32 = {
        let mut rows = conn
            .query("SELECT COALESCE(MAX(version), 0) FROM schema_version", ())
            .await
            .context("failed to query schema_version in Turso")?;
        match rows
            .next()
            .await
            .context("failed to read schema_version row")?
        {
            Some(row) => row.get::<u32>(0).unwrap_or(0),
            None => 0,
        }
    };

    for migration in TURSO_MIGRATIONS {
        if migration.version <= current_version {
            continue;
        }
        for sql in migration.statements {
            match conn.execute(sql, ()).await {
                Ok(_) => {}
                // Backward-compat: column already exists from pre-versioned setup.
                Err(e) if e.to_string().contains("duplicate column") => {}
                Err(e) => {
                    return Err(anyhow!(
                        "Turso migration v{} failed ({}): {e}",
                        migration.version,
                        migration.description
                    ));
                }
            }
        }
        conn.execute(
            "INSERT INTO schema_version (version, description) VALUES (?1, ?2)",
            libsql::params![migration.version, migration.description],
        )
        .await
        .with_context(|| {
            format!(
                "failed to record Turso migration v{}: {}",
                migration.version, migration.description
            )
        })?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// TursoMemoryStore
// ---------------------------------------------------------------------------

pub struct TursoMemoryStore {
    _db: libsql::Database,
    conn: tokio::sync::Mutex<libsql::Connection>,
}

impl TursoMemoryStore {
    pub async fn connect(settings: TursoSettings) -> anyhow::Result<Self> {
        let metadata = settings.sanitized_connection_metadata();
        let db = libsql::Builder::new_remote(
            settings.database_url,
            settings.auth_token.expose().to_string(),
        )
        .build()
        .await
        .with_context(|| format!("failed to initialize Turso database (url={metadata})"))?;

        let conn = db
            .connect()
            .with_context(|| format!("failed to connect to Turso (url={metadata})"))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
            (),
        )
        .await
        .context("failed to ensure memory schema in Turso")?;

        run_turso_migrations(&conn)
            .await
            .context("failed to run Turso migrations")?;

        Ok(Self {
            _db: db,
            conn: tokio::sync::Mutex::new(conn),
        })
    }
}

/// Helper: map a Turso row to a [`MemoryEntry`] (columns 0–7).
fn turso_row_to_entry(row: &libsql::Row) -> anyhow::Result<MemoryEntry> {
    Ok(MemoryEntry {
        role: row.get::<String>(0).context("invalid role column")?,
        content: row.get::<String>(1).context("invalid content column")?,
        privacy_boundary: row.get::<String>(2).unwrap_or_default(),
        source_channel: row.get::<Option<String>>(3).unwrap_or_default(),
        conversation_id: row.get::<String>(4).unwrap_or_default(),
        created_at: row.get::<Option<String>>(5).ok().flatten(),
        expires_at: row.get::<Option<i64>>(6).unwrap_or_default(),
        org_id: row.get::<String>(7).unwrap_or_default(),
        agent_id: row.get::<String>(8).unwrap_or_default(),
    })
}

fn now_epoch_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[async_trait]
impl MemoryStore for TursoMemoryStore {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO memory(role, content, privacy_boundary, source_channel, conversation_id, expires_at, org_id, agent_id)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            libsql::params![
                entry.role,
                entry.content,
                entry.privacy_boundary,
                entry.source_channel,
                entry.conversation_id,
                entry.expires_at,
                entry.org_id,
                entry.agent_id
            ],
        )
        .await
        .context("failed to append memory entry in Turso")?;
        Ok(())
    }

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().await;
        let now = now_epoch_secs();
        let mut rows = conn
            .query(
                "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                        datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id
                 FROM memory
                 WHERE expires_at IS NULL OR expires_at > ?1
                 ORDER BY id DESC LIMIT ?2",
                libsql::params![now, limit as i64],
            )
            .await
            .context("failed to query memory entries from Turso")?;

        let mut out = Vec::new();
        while let Some(row) = rows.next().await.context("failed to read Turso row")? {
            out.push(turso_row_to_entry(&row)?);
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
        let conn = self.conn.lock().await;
        let now = now_epoch_secs();
        let boundary_owned = boundary.to_string();
        let source_owned = source_channel.map(|s| s.to_string());
        let mut rows = conn
            .query(
                "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                        datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id
                 FROM memory
                 WHERE (expires_at IS NULL OR expires_at > ?1)
                   AND (privacy_boundary = '' OR privacy_boundary = ?2
                        OR privacy_boundary IN ('any', 'inherit')
                        OR (?2 = 'local_only' AND privacy_boundary = 'encrypted_only'))
                   AND (?3 IS NULL OR source_channel IS NULL OR source_channel = ?3)
                 ORDER BY id DESC LIMIT ?4",
                libsql::params![now, boundary_owned, source_owned, limit as i64],
            )
            .await
            .context("failed to query boundary-filtered entries from Turso")?;

        let mut out = Vec::new();
        while let Some(row) = rows.next().await.context("failed to read Turso row")? {
            out.push(turso_row_to_entry(&row)?);
        }
        out.reverse();
        Ok(out)
    }

    async fn recent_for_conversation(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().await;
        let now = now_epoch_secs();
        let cid = conversation_id.to_string();
        let mut rows = conn
            .query(
                "SELECT role, content, privacy_boundary, source_channel, conversation_id,
                        datetime(created_at, 'unixepoch') as created_at_iso, expires_at, org_id, agent_id
                 FROM memory
                 WHERE conversation_id = ?1
                   AND (expires_at IS NULL OR expires_at > ?2)
                 ORDER BY id DESC LIMIT ?3",
                libsql::params![cid, now, limit as i64],
            )
            .await
            .context("failed to query conversation entries from Turso")?;

        let mut out = Vec::new();
        while let Some(row) = rows.next().await.context("failed to read Turso row")? {
            out.push(turso_row_to_entry(&row)?);
        }
        out.reverse();
        Ok(out)
    }

    async fn fork_conversation(&self, from_id: &str, new_id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        let now = now_epoch_secs();
        conn.execute(
            "INSERT INTO memory(role, content, privacy_boundary, source_channel, conversation_id, expires_at, org_id, agent_id)
             SELECT role, content, privacy_boundary, source_channel, ?1, expires_at, org_id, agent_id
             FROM memory
             WHERE conversation_id = ?2
               AND (expires_at IS NULL OR expires_at > ?3)
             ORDER BY id",
            libsql::params![new_id, from_id, now],
        )
        .await
        .context("failed to fork conversation in Turso")?;
        Ok(())
    }

    async fn list_conversations(&self) -> anyhow::Result<Vec<String>> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT DISTINCT conversation_id FROM memory
                 WHERE conversation_id != ''
                 ORDER BY conversation_id",
                (),
            )
            .await
            .context("failed to list conversations in Turso")?;

        let mut out = Vec::new();
        while let Some(row) = rows.next().await.context("failed to read Turso row")? {
            out.push(row.get::<String>(0).context("invalid conversation_id")?);
        }
        Ok(out)
    }

    async fn gc_expired(&self) -> anyhow::Result<u64> {
        let conn = self.conn.lock().await;
        let now = now_epoch_secs();
        let deleted = conn
            .execute(
                "DELETE FROM memory WHERE expires_at IS NOT NULL AND expires_at <= ?1",
                libsql::params![now],
            )
            .await
            .context("failed to gc expired entries in Turso")?;
        Ok(deleted as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::MemoryStore;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Current max migration version for assertions.
    fn current_turso_schema_version() -> u32 {
        TURSO_MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
    }

    #[test]
    fn turso_migration_count_matches_sqlite() {
        // Keep Turso and SQLite migration counts in sync.
        assert_eq!(current_turso_schema_version(), 4);
    }

    #[test]
    fn turso_settings_reject_invalid_url_scheme() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous_url = std::env::var("TURSO_DATABASE_URL").ok();
        let previous_token = std::env::var("TURSO_AUTH_TOKEN").ok();

        std::env::set_var("TURSO_DATABASE_URL", "http://not-supported");
        std::env::set_var("TURSO_AUTH_TOKEN", "token");

        let result = TursoSettings::from_env();
        assert!(result.is_err());

        match previous_url {
            Some(v) => std::env::set_var("TURSO_DATABASE_URL", v),
            None => std::env::remove_var("TURSO_DATABASE_URL"),
        }
        match previous_token {
            Some(v) => std::env::set_var("TURSO_AUTH_TOKEN", v),
            None => std::env::remove_var("TURSO_AUTH_TOKEN"),
        }
    }

    #[test]
    fn secret_token_debug_is_redacted() {
        let token = SecretToken::new("token-123".to_string()).expect("token should parse");
        assert_eq!(format!("{token:?}"), "[REDACTED]");
    }

    #[test]
    fn turso_settings_reject_whitespace_token() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous_url = std::env::var("TURSO_DATABASE_URL").ok();
        let previous_token = std::env::var("TURSO_AUTH_TOKEN").ok();

        std::env::set_var("TURSO_DATABASE_URL", "libsql://example.turso.io");
        std::env::set_var("TURSO_AUTH_TOKEN", "bad token");

        let result = TursoSettings::from_env();
        assert!(result.is_err());
        let err = result.err().expect("whitespace token should fail");
        assert!(err.to_string().contains("must not contain whitespace"));

        match previous_url {
            Some(v) => std::env::set_var("TURSO_DATABASE_URL", v),
            None => std::env::remove_var("TURSO_DATABASE_URL"),
        }
        match previous_token {
            Some(v) => std::env::set_var("TURSO_AUTH_TOKEN", v),
            None => std::env::remove_var("TURSO_AUTH_TOKEN"),
        }
    }

    #[test]
    fn turso_settings_reject_tls_disable_marker() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous_url = std::env::var("TURSO_DATABASE_URL").ok();
        let previous_token = std::env::var("TURSO_AUTH_TOKEN").ok();

        std::env::set_var("TURSO_DATABASE_URL", "libsql://example.turso.io?tls=0");
        std::env::set_var("TURSO_AUTH_TOKEN", "token");

        let result = TursoSettings::from_env();
        assert!(result.is_err());
        let err = result.err().expect("tls disable marker should fail");
        assert!(err.to_string().contains("TLS must remain enabled"));

        match previous_url {
            Some(v) => std::env::set_var("TURSO_DATABASE_URL", v),
            None => std::env::remove_var("TURSO_DATABASE_URL"),
        }
        match previous_token {
            Some(v) => std::env::set_var("TURSO_AUTH_TOKEN", v),
            None => std::env::remove_var("TURSO_AUTH_TOKEN"),
        }
    }

    #[test]
    fn turso_settings_reject_missing_auth_token() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous_url = std::env::var("TURSO_DATABASE_URL").ok();
        let previous_token = std::env::var("TURSO_AUTH_TOKEN").ok();

        std::env::set_var("TURSO_DATABASE_URL", "libsql://example.turso.io");
        std::env::remove_var("TURSO_AUTH_TOKEN");

        let result = TursoSettings::from_env();
        assert!(result.is_err());
        let err = result.err().expect("missing auth token should fail");
        assert!(err.to_string().contains("missing TURSO_AUTH_TOKEN"));

        match previous_url {
            Some(v) => std::env::set_var("TURSO_DATABASE_URL", v),
            None => std::env::remove_var("TURSO_DATABASE_URL"),
        }
        match previous_token {
            Some(v) => std::env::set_var("TURSO_AUTH_TOKEN", v),
            None => std::env::remove_var("TURSO_AUTH_TOKEN"),
        }
    }

    #[test]
    fn sanitized_connection_metadata_strips_query_parameters() {
        let settings = TursoSettings {
            database_url: "libsql://example.turso.io?auth_token=secret&tls=1".to_string(),
            auth_token: SecretToken::new("token".to_string()).expect("token should parse"),
        };
        assert_eq!(
            settings.sanitized_connection_metadata(),
            "libsql://example.turso.io"
        );
    }

    #[tokio::test]
    async fn turso_roundtrip_runs_when_env_is_configured() {
        let database_url = match std::env::var("TURSO_TEST_DATABASE_URL") {
            Ok(value) if !value.trim().is_empty() => value,
            _ => return,
        };
        let auth_token = match std::env::var("TURSO_TEST_AUTH_TOKEN") {
            Ok(value) if !value.trim().is_empty() => value,
            _ => return,
        };

        let store = TursoMemoryStore::connect(TursoSettings {
            database_url,
            auth_token: SecretToken::new(auth_token).expect("test token should parse"),
        })
        .await
        .expect("turso test connection should succeed");

        store
            .append(agentzero_core::MemoryEntry {
                role: "user".to_string(),
                content: "integration-roundtrip".to_string(),
                ..Default::default()
            })
            .await
            .expect("append should succeed");
        let recent = store.recent(1).await.expect("recent should succeed");
        assert!(!recent.is_empty());
    }
}
