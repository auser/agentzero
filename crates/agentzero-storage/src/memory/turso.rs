use agentzero_core::{MemoryEntry, MemoryStore};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use std::fmt::{Debug, Formatter};

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

        Ok(Self {
            _db: db,
            conn: tokio::sync::Mutex::new(conn),
        })
    }
}

#[async_trait]
impl MemoryStore for TursoMemoryStore {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO memory(role, content) VALUES(?1, ?2)",
            libsql::params![entry.role, entry.content],
        )
        .await
        .context("failed to append memory entry in Turso")?;
        Ok(())
    }

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT role, content FROM memory ORDER BY id DESC LIMIT ?1",
                libsql::params![limit as i64],
            )
            .await
            .context("failed to query memory entries from Turso")?;

        let mut out = Vec::new();
        while let Some(row) = rows.next().await.context("failed to read Turso row")? {
            out.push(MemoryEntry {
                role: row.get::<String>(0).context("invalid role column type")?,
                content: row
                    .get::<String>(1)
                    .context("invalid content column type")?,
                ..Default::default()
            });
        }
        out.reverse();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{SecretToken, TursoMemoryStore, TursoSettings};
    use agentzero_core::MemoryStore;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn turso_settings_reject_invalid_url_scheme() {
        let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
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
        let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
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
        let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
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
        let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
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
