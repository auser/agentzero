use crate::cli::HookCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_hooks::HookStore;
use async_trait::async_trait;

pub struct HooksCommand;

#[async_trait]
impl AgentZeroCommand for HooksCommand {
    type Options = HookCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let store = HookStore::new(&ctx.data_dir)?;
        match opts {
            HookCommands::List { json } => {
                let hooks = store.list()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&hooks)?);
                } else {
                    println!("Hooks ({})", hooks.len());
                    for hook in hooks {
                        println!(
                            "- {} [{}]",
                            hook.name,
                            if hook.enabled { "enabled" } else { "disabled" }
                        );
                    }
                }
            }
            HookCommands::Enable { name } => {
                let hook = store.enable(&name)?;
                println!("Enabled hook `{}`", hook.name);
            }
            HookCommands::Disable { name } => {
                let hook = store.disable(&name)?;
                println!("Disabled hook `{}`", hook.name);
            }
            HookCommands::Test { name } => {
                let result = store.test(&name)?;
                println!("{result}");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::HooksCommand;
    use crate::cli::HookCommands;
    use crate::command_core::{AgentZeroCommand, CommandContext};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("agentzero-hooks-cmd-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn hooks_list_empty_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        HooksCommand::run(&ctx, HookCommands::List { json: true })
            .await
            .expect("list should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn hooks_enable_missing_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = HooksCommand::run(
            &ctx,
            HookCommands::Enable {
                name: "nonexistent".to_string(),
            },
        )
        .await
        .expect_err("enabling missing hook should fail");
        assert!(err.to_string().contains("unknown hook"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn hooks_disable_missing_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = HooksCommand::run(
            &ctx,
            HookCommands::Disable {
                name: "nonexistent".to_string(),
            },
        )
        .await
        .expect_err("disabling missing hook should fail");
        assert!(err.to_string().contains("unknown hook"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn hooks_test_missing_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = HooksCommand::run(
            &ctx,
            HookCommands::Test {
                name: "nonexistent".to_string(),
            },
        )
        .await
        .expect_err("testing missing hook should fail");
        assert!(err.to_string().contains("unknown hook"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
