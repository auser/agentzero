use crate::cli::GoalCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_goals::Goal;
use agentzero_storage::EncryptedJsonStore;
use async_trait::async_trait;
use std::collections::BTreeMap;

const GOALS_FILE: &str = "goals.json";

pub struct GoalsCommand;

#[async_trait]
impl AgentZeroCommand for GoalsCommand {
    type Options = GoalCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let store = EncryptedJsonStore::in_config_dir(&ctx.data_dir, GOALS_FILE)?;
        let mut goals = store.load_or_default::<BTreeMap<String, Goal>>()?;

        match opts {
            GoalCommands::List { json } => {
                let list = goals.values().cloned().collect::<Vec<_>>();
                if json {
                    println!("{}", serde_json::to_string_pretty(&list)?);
                } else if list.is_empty() {
                    println!("No goals");
                } else {
                    println!("Goals ({})", list.len());
                    for goal in list {
                        println!(
                            "- {} [{}] {}",
                            goal.id,
                            if goal.completed {
                                "completed"
                            } else {
                                "incomplete"
                            },
                            goal.title
                        );
                    }
                }
            }
            GoalCommands::Add { id, title } => {
                let goal = Goal {
                    id: id.clone(),
                    title,
                    completed: false,
                };
                goals.insert(id.clone(), goal);
                store.save(&goals)?;
                println!("Upserted goal `{id}`");
            }
            GoalCommands::Complete { id } => {
                let goal = goals
                    .get_mut(&id)
                    .ok_or_else(|| anyhow::anyhow!("goal `{id}` not found"))?;
                goal.complete();
                store.save(&goals)?;
                println!("Marked goal `{id}` complete");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::GoalsCommand;
    use crate::cli::GoalCommands;
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
        let dir = std::env::temp_dir().join(format!("agentzero-goals-cmd-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn goals_add_complete_list_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        GoalsCommand::run(
            &ctx,
            GoalCommands::Add {
                id: "g1".to_string(),
                title: "Ship feature".to_string(),
            },
        )
        .await
        .expect("add should succeed");

        GoalsCommand::run(
            &ctx,
            GoalCommands::Complete {
                id: "g1".to_string(),
            },
        )
        .await
        .expect("complete should succeed");

        GoalsCommand::run(&ctx, GoalCommands::List { json: true })
            .await
            .expect("list should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn goals_complete_missing_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = GoalsCommand::run(
            &ctx,
            GoalCommands::Complete {
                id: "missing".to_string(),
            },
        )
        .await
        .expect_err("missing goal should fail");
        assert!(err.to_string().contains("not found"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
