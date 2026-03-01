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
