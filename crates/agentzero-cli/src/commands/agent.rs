use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_runtime::{run_agent_once, RunAgentRequest};
use async_trait::async_trait;

pub struct AgentOptions {
    /// Message to send to agent
    pub message: String,
}

pub struct AgentCommand;

#[async_trait]
impl AgentZeroCommand for AgentCommand {
    type Options = AgentOptions;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let output = run_agent_once(RunAgentRequest {
            workspace_root: ctx.workspace_root.clone(),
            config_path: ctx.config_path.clone(),
            message: opts.message,
        })
        .await?;

        println!("{}", output.response_text);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentCommand, AgentOptions};
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
        let dir = std::env::temp_dir().join(format!("agentzero-cli-agent-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn agent_command_fails_when_api_key_missing() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(
            &config_path,
            "[provider]\nkind=\"openai\"\nbase_url=\"https://api.openai.com\"\nmodel=\"gpt-4o-mini\"\n\n[memory]\nbackend=\"sqlite\"\nsqlite_path=\"./agentzero.db\"\n",
        )
        .expect("config should be written");
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path,
        };

        let err = AgentCommand::run(
            &ctx,
            AgentOptions {
                message: "hello".to_string(),
            },
        )
        .await
        .expect_err("missing api key should fail");
        assert!(err.to_string().contains("missing OPENAI_API_KEY"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
