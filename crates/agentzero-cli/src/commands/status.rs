use crate::command_core::{AgentZeroCommand, CommandContext};
use crate::commands::memory::build_memory_store;
use async_trait::async_trait;
use serde_json::json;

pub struct StatusOptions {
    pub json: bool,
}

pub struct StatusCommand;

#[async_trait]
impl AgentZeroCommand for StatusCommand {
    type Options = StatusOptions;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let memory = build_memory_store(ctx).await?;
        let items = memory.recent(5).await?;
        if opts.json {
            println!(
                "{}",
                json!({
                    "name": "agentzero",
                    "recent_memory_items": items.len(),
                })
            );
        } else {
            println!("AgentZero status");
            println!("recent memory items: {}", items.len());
        }
        Ok(())
    }
}
