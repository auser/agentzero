//! Placeholder for tier CLI commands (Sprint 74).
//! Full implementation lives on `feat/self-evolution-engine`.

use crate::cli::TierCommands;
use crate::command_core::CommandContext;

pub struct TierCommand;

impl TierCommand {
    pub async fn run(_ctx: &CommandContext, _command: TierCommands) -> anyhow::Result<()> {
        anyhow::bail!("tier commands are not yet available — coming in Sprint 74")
    }
}
