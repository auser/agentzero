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
