use crate::command_core::CommandContext;
use agentzero_config::load as load_config;
use agentzero_core::MemoryStore;
use agentzero_memory_sqlite::SqliteMemoryStore;

pub async fn build_memory_store(ctx: &CommandContext) -> anyhow::Result<Box<dyn MemoryStore>> {
    let config = load_config(&ctx.config_path)?;
    let backend = config.memory.backend;

    match backend.as_str() {
        "sqlite" => Ok(Box::new(SqliteMemoryStore::open(
            config.memory.sqlite_path,
        )?)),
        "turso" => build_turso_store().await,
        other => Err(anyhow::anyhow!(
            "unsupported AGENTZERO_MEMORY_BACKEND `{other}`; expected `sqlite` or `turso`"
        )),
    }
}

#[cfg(feature = "memory-turso")]
async fn build_turso_store() -> anyhow::Result<Box<dyn MemoryStore>> {
    let settings = agentzero_memory_turso::TursoSettings::from_env()?;
    let store = agentzero_memory_turso::TursoMemoryStore::connect(settings).await?;
    Ok(Box::new(store))
}

#[cfg(not(feature = "memory-turso"))]
async fn build_turso_store() -> anyhow::Result<Box<dyn MemoryStore>> {
    Err(anyhow::anyhow!(
        "turso backend requested but agentzero-cli was built without `memory-turso` feature"
    ))
}

#[cfg(test)]
mod tests {
    use super::build_memory_store;
    use crate::command_core::CommandContext;
    use agentzero_core::MemoryEntry;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after unix epoch")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("agentzero-memory-cli-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn write_config(path: &PathBuf, memory_backend: &str, sqlite_path: &str) {
        let config = format!(
            "[memory]\nbackend = \"{memory_backend}\"\nsqlite_path = \"{sqlite_path}\"\n\n[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n"
        );
        fs::write(path, config).expect("config should be written");
    }

    #[tokio::test]
    async fn sqlite_backend_stays_usable_with_bad_turso_env() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        let sqlite_path = dir.join("agentzero-test.db");
        write_config(
            &config_path,
            "sqlite",
            sqlite_path.to_str().expect("sqlite path should be utf8"),
        );

        let prev_url = std::env::var("TURSO_DATABASE_URL").ok();
        let prev_token = std::env::var("TURSO_AUTH_TOKEN").ok();
        std::env::set_var("TURSO_DATABASE_URL", "http://not-supported");
        std::env::set_var("TURSO_AUTH_TOKEN", "bad token");

        let ctx = CommandContext {
            workspace_root: dir.clone(),
            config_path: config_path.clone(),
        };
        let store = build_memory_store(&ctx)
            .await
            .expect("sqlite backend should build");
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "hello".to_string(),
            })
            .await
            .expect("sqlite append should work");
        let recent = store.recent(1).await.expect("sqlite recent should work");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].content, "hello");

        match prev_url {
            Some(v) => std::env::set_var("TURSO_DATABASE_URL", v),
            None => std::env::remove_var("TURSO_DATABASE_URL"),
        }
        match prev_token {
            Some(v) => std::env::set_var("TURSO_AUTH_TOKEN", v),
            None => std::env::remove_var("TURSO_AUTH_TOKEN"),
        }
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[cfg(not(feature = "memory-turso"))]
    #[tokio::test]
    async fn turso_backend_reports_unavailable_when_feature_disabled() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        write_config(&config_path, "turso", "./ignored.db");

        let ctx = CommandContext {
            workspace_root: dir.clone(),
            config_path,
        };
        let result = build_memory_store(&ctx).await;
        let err = match result {
            Ok(_) => panic!("turso feature should be unavailable"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("built without `memory-turso` feature"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
