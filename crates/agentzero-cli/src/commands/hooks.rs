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
