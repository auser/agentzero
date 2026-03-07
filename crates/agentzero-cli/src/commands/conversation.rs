use crate::cli::ConversationCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use crate::commands::memory::build_memory_store;
use async_trait::async_trait;

pub struct ConversationCommand;

#[async_trait]
impl AgentZeroCommand for ConversationCommand {
    type Options = ConversationCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            ConversationCommands::List { json } => {
                let store = build_memory_store(ctx).await?;
                let conversations = store.list_conversations().await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&conversations)?);
                } else if conversations.is_empty() {
                    println!("No named conversations");
                } else {
                    println!("Conversations ({}):", conversations.len());
                    for cid in &conversations {
                        println!("  - {cid}");
                    }
                }
            }
            ConversationCommands::Fork { from, to, json } => {
                let store = build_memory_store(ctx).await?;
                store.fork_conversation(&from, &to).await?;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({"status": "ok", "from": from, "to": to})
                    );
                } else {
                    println!("Forked conversation '{from}' -> '{to}'");
                }
            }
            ConversationCommands::Switch { id } => {
                let state_path = ctx.data_dir.join("active_conversation");
                if id.is_empty() {
                    if state_path.exists() {
                        std::fs::remove_file(&state_path)?;
                    }
                    println!("Switched to global conversation (no scope)");
                } else {
                    std::fs::write(&state_path, &id)?;
                    println!("Switched to conversation '{id}'");
                }
            }
        }
        Ok(())
    }
}

/// Read the active conversation ID from state file, if any.
pub fn read_active_conversation(ctx: &CommandContext) -> Option<String> {
    let path = ctx.data_dir.join("active_conversation");
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
