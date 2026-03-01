use crate::cli::CoordinationCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_coordination::CoordinationStatus;
use agentzero_storage::EncryptedJsonStore;
use async_trait::async_trait;

const COORDINATION_STATUS_FILE: &str = "coordination-status.json";

pub struct CoordinationCommand;

#[async_trait]
impl AgentZeroCommand for CoordinationCommand {
    type Options = CoordinationCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let store = EncryptedJsonStore::in_config_dir(&ctx.data_dir, COORDINATION_STATUS_FILE)?;
        let mut status = store.load_or_default::<CoordinationStatus>()?;

        match opts {
            CoordinationCommands::Status { json } => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&status)?);
                } else {
                    println!(
                        "Coordination: workers={} queued_tasks={} idle={}",
                        status.active_workers,
                        status.queued_tasks,
                        status.is_idle()
                    );
                }
            }
            CoordinationCommands::Set {
                active_workers,
                queued_tasks,
            } => {
                status.active_workers = active_workers;
                status.queued_tasks = queued_tasks;
                store.save(&status)?;
                println!(
                    "Updated coordination status: workers={} queued_tasks={}",
                    status.active_workers, status.queued_tasks
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::CoordinationCommand;
    use crate::cli::CoordinationCommands;
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
        let dir =
            std::env::temp_dir().join(format!("agentzero-coordination-cmd-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn coordination_set_and_status_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        CoordinationCommand::run(
            &ctx,
            CoordinationCommands::Set {
                active_workers: 2,
                queued_tasks: 5,
            },
        )
        .await
        .expect("set should succeed");

        CoordinationCommand::run(&ctx, CoordinationCommands::Status { json: true })
            .await
            .expect("status should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
