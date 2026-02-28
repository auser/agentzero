use crate::cli::{Cli, Commands};
use crate::command_core::{AgentZeroCommand, CommandContext};
use crate::commands;

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    let ctx = CommandContext::from_current_dir(cli.config.clone(), cli.data_dir.clone())?;

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
        Commands::Gateway {
            host,
            port,
            new_pairing,
        } => {
            commands::gateway::GatewayCommand::run(
                &ctx,
                commands::gateway::GatewayOptions {
                    host,
                    port,
                    new_pairing,
                },
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
        Commands::Auth { command } => commands::auth::AuthCommand::run(&ctx, command).await,
        Commands::Providers { json, no_color } => {
            commands::providers::ProvidersCommand::run(
                &ctx,
                commands::providers::ProvidersOptions { json, no_color },
            )
            .await
        }
        Commands::Doctor => commands::doctor::DoctorCommand::run(&ctx, ()).await,
    }
}
