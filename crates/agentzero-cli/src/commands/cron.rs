use crate::cli::CronCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_cron::CronStore;
use async_trait::async_trait;

pub struct CronCommand;

#[async_trait]
impl AgentZeroCommand for CronCommand {
    type Options = CronCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let store = CronStore::new(&ctx.data_dir)?;
        match opts {
            CronCommands::List { json: emit_json } => {
                let tasks = store.list()?;
                if emit_json {
                    println!("{}", serde_json::to_string_pretty(&tasks)?);
                } else {
                    println!("Scheduled tasks ({})", tasks.len());
                    for task in tasks {
                        println!(
                            "- {} [{}] {} :: {}",
                            task.id,
                            if task.enabled { "enabled" } else { "paused" },
                            task.schedule,
                            task.command
                        );
                    }
                }
            }
            CronCommands::Add {
                id,
                schedule,
                command,
            }
            | CronCommands::AddAt {
                id,
                schedule,
                command,
            }
            | CronCommands::AddEvery {
                id,
                schedule,
                command,
            }
            | CronCommands::Once {
                id,
                schedule,
                command,
            } => {
                let task = store.add(&id, &schedule, &command)?;
                println!("Added cron task `{}`", task.id);
            }
            CronCommands::Update {
                id,
                schedule,
                command,
            } => {
                let task = store.update(&id, schedule.as_deref(), command.as_deref())?;
                println!(
                    "Updated cron task `{}`: schedule={}, command={}",
                    task.id, task.schedule, task.command
                );
            }
            CronCommands::Pause { id } => {
                let task = store.pause(&id)?;
                println!("Paused cron task `{}`", task.id);
            }
            CronCommands::Resume { id } => {
                let task = store.resume(&id)?;
                println!("Resumed cron task `{}`", task.id);
            }
            CronCommands::Remove { id } => {
                store.remove(&id)?;
                println!("Removed cron task `{}`", id);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::CronCommand;
    use crate::cli::CronCommands;
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
        let dir = std::env::temp_dir().join(format!("agentzero-cron-cmd-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn cron_add_list_remove_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        CronCommand::run(
            &ctx,
            CronCommands::Add {
                id: "task1".to_string(),
                schedule: "0 * * * *".to_string(),
                command: "echo hello".to_string(),
            },
        )
        .await
        .expect("add should succeed");

        CronCommand::run(&ctx, CronCommands::List { json: true })
            .await
            .expect("list should succeed");

        CronCommand::run(
            &ctx,
            CronCommands::Remove {
                id: "task1".to_string(),
            },
        )
        .await
        .expect("remove should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn cron_pause_resume_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        CronCommand::run(
            &ctx,
            CronCommands::Add {
                id: "t2".to_string(),
                schedule: "daily".to_string(),
                command: "backup".to_string(),
            },
        )
        .await
        .expect("add should succeed");

        CronCommand::run(
            &ctx,
            CronCommands::Pause {
                id: "t2".to_string(),
            },
        )
        .await
        .expect("pause should succeed");

        CronCommand::run(
            &ctx,
            CronCommands::Resume {
                id: "t2".to_string(),
            },
        )
        .await
        .expect("resume should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn cron_remove_missing_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = CronCommand::run(
            &ctx,
            CronCommands::Remove {
                id: "nonexistent".to_string(),
            },
        )
        .await
        .expect_err("removing missing task should fail");
        assert!(err.to_string().contains("not found"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
