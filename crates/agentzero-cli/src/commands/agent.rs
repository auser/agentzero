use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_infra::runtime::{
    build_runtime_execution, run_agent_once, run_agent_streaming, RunAgentRequest,
};
use agentzero_orchestrator::agent_store::AgentStore;
use async_trait::async_trait;
use std::io::Write;
use std::sync::Arc;

pub struct AgentOptions {
    /// Message to send to agent
    pub message: String,
    /// Override the provider kind (e.g. openrouter, openai, ollama)
    pub provider: Option<String>,
    /// Override the model name
    pub model: Option<String>,
    /// Use a specific auth profile by name (from `auth list`)
    pub profile: Option<String>,
    /// Enable streaming output
    pub stream: bool,
}

pub struct AgentCommand;

#[async_trait]
impl AgentZeroCommand for AgentCommand {
    type Options = AgentOptions;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        if opts.stream {
            return run_streaming(ctx, opts).await;
        }

        let agent_store = build_agent_store(ctx);
        let output = run_agent_once(RunAgentRequest {
            workspace_root: ctx.workspace_root.clone(),
            config_path: ctx.config_path.clone(),
            message: opts.message,
            provider_override: opts.provider,
            model_override: opts.model,
            profile_override: opts.profile,
            extra_tools: vec![],
            conversation_id: super::conversation::read_active_conversation(ctx),
            agent_store,
        })
        .await?;

        println!("{}", output.response_text);
        Ok(())
    }
}

/// Build an `AgentStore` from the CLI data directory (best-effort).
fn build_agent_store(
    ctx: &CommandContext,
) -> Option<Arc<dyn agentzero_core::agent_store::AgentStoreApi>> {
    match AgentStore::persistent(&ctx.data_dir) {
        Ok(store) => Some(Arc::new(store)),
        Err(e) => {
            tracing::debug!(error = %e, "could not open agent store; agent_manage tool will be unavailable");
            None
        }
    }
}

async fn run_streaming(ctx: &CommandContext, opts: AgentOptions) -> anyhow::Result<()> {
    let message = opts.message.clone();
    let agent_store = build_agent_store(ctx);
    let execution = build_runtime_execution(RunAgentRequest {
        workspace_root: ctx.workspace_root.clone(),
        config_path: ctx.config_path.clone(),
        message: message.clone(),
        provider_override: opts.provider,
        model_override: opts.model,
        profile_override: opts.profile,
        extra_tools: vec![],
        conversation_id: super::conversation::read_active_conversation(ctx),
        agent_store,
    })
    .await?;

    let (mut rx, handle) = run_agent_streaming(execution, ctx.workspace_root.clone(), message);

    while let Some(chunk) = rx.recv().await {
        if !chunk.delta.is_empty() {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            write!(out, "{}", chunk.delta)?;
            out.flush()?;
        }
    }
    println!();

    handle.await??;
    Ok(())
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
        let dir = std::env::temp_dir().join(format!(
            "agentzero-cli-agent-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    /// Verify that running the agent command without a live provider fails
    /// rather than hanging or panicking.  The exact error depends on the host
    /// environment (missing API key, network error, memory-store error, etc.)
    /// so we only assert that the result is `Err`.
    #[tokio::test]
    async fn agent_command_fails_without_live_provider() {
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

        AgentCommand::run(
            &ctx,
            AgentOptions {
                message: "hello".to_string(),
                provider: None,
                model: None,
                profile: None,
                stream: false,
            },
        )
        .await
        .expect_err("should fail without a reachable provider");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn agent_command_fails_without_config_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("nonexistent.toml"),
        };

        AgentCommand::run(
            &ctx,
            AgentOptions {
                message: "hello".to_string(),
                provider: None,
                model: None,
                profile: None,
                stream: false,
            },
        )
        .await
        .expect_err("missing config should fail");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn agent_options_struct_construction_success_path() {
        let opts = AgentOptions {
            message: "test message".to_string(),
            provider: None,
            model: None,
            profile: None,
            stream: false,
        };
        assert_eq!(opts.message, "test message");
        assert!(!opts.stream);

        let empty_opts = AgentOptions {
            message: String::new(),
            provider: Some("ollama".to_string()),
            model: Some("llama3".to_string()),
            profile: Some("myprofile".to_string()),
            stream: false,
        };
        assert!(empty_opts.message.is_empty());
        assert_eq!(empty_opts.provider.as_deref(), Some("ollama"));
        assert_eq!(empty_opts.model.as_deref(), Some("llama3"));
        assert_eq!(empty_opts.profile.as_deref(), Some("myprofile"));
    }

    #[test]
    fn stream_flag_defaults_false() {
        let opts = AgentOptions {
            message: "hi".to_string(),
            provider: None,
            model: None,
            profile: None,
            stream: false,
        };
        assert!(!opts.stream);
    }

    /// Same as above but with streaming mode enabled.
    #[tokio::test]
    async fn stream_error_propagation() {
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

        AgentCommand::run(
            &ctx,
            AgentOptions {
                message: "hello".to_string(),
                provider: None,
                model: None,
                profile: None,
                stream: true,
            },
        )
        .await
        .expect_err("should fail without a reachable provider (streaming)");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
