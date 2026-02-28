use crate::cli::{Cli, Commands};
use crate::command_core::{AgentZeroCommand, CommandContext};
use crate::commands;

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    let ctx = CommandContext::from_current_dir(cli.config.clone())?;

    match cli.command {
        Commands::Onboard {
            yes,
            provider,
            base_url,
            model,
            memory_path,
            allowed_root,
            allowed_commands,
        } => {
            commands::onboard::OnboardCommand::run(
                &ctx,
                commands::onboard::OnboardOptions {
                    yes,
                    provider,
                    base_url,
                    model,
                    memory_path,
                    allowed_root,
                    allowed_commands,
                },
            )
            .await
        }
        Commands::Gateway { host, port } => {
            commands::gateway::GatewayCommand::run(
                &ctx,
                commands::gateway::GatewayOptions { host, port },
            )
            .await
        }
        Commands::Status { json } => {
            commands::status::StatusCommand::run(&ctx, commands::status::StatusOptions { json })
                .await
        }
        Commands::Agent { message } => {
            commands::agent::AgentCommand::run(&ctx, commands::agent::AgentOptions { message })
                .await
        }
        Commands::Doctor => commands::doctor::DoctorCommand::run(&ctx, ()).await,
    }
}
