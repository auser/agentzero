use crate::command_core::{AgentZeroCommand, CommandContext};
use crate::commands::memory::build_memory_store;
use async_trait::async_trait;

pub struct StatusCommand;

#[async_trait]
impl AgentZeroCommand for StatusCommand {
    type Options = ();

    async fn run(ctx: &CommandContext, _opts: Self::Options) -> anyhow::Result<()> {
        let memory = build_memory_store(ctx).await?;
        let items = memory.recent(5).await?;
        println!("AgentZero status");
        println!("recent memory items: {}", items.len());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::StatusCommand;
    use crate::command_core::{AgentZeroCommand, CommandContext};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("agentzero-cli-status-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn status_command_with_valid_config_success_path() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        let db_path = dir.join("status-test.db");
        let config = format!(
            "[memory]\nbackend = \"sqlite\"\nsqlite_path = \"{}\"\n\n[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n",
            db_path.to_str().expect("path should be utf8")
        );
        fs::write(&config_path, config).expect("config should be written");

        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path,
        };

        StatusCommand::run(&ctx, ())
            .await
            .expect("status with valid config should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn status_command_with_invalid_backend_fails_negative_path() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(
            &config_path,
            "[memory]\nbackend = \"redis\"\nsqlite_path = \"./test.db\"\n\n[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n",
        )
        .expect("config should be written");

        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path,
        };

        let err = StatusCommand::run(&ctx, ())
            .await
            .expect_err("unsupported backend should fail");
        assert!(err.to_string().contains("unsupported"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
