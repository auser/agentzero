use crate::cli::{Cli, Commands};
use crate::command_core::{AgentZeroCommand, CommandContext};
use crate::commands;

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    let ctx = CommandContext::from_current_dir(cli.config.clone(), cli.data_dir.clone())?;

    match cli.command {
        Commands::Onboard {
            interactive,
            force,
            channels_only,
            api_key,
            yes,
            provider,
            base_url,
            model,
            memory,
            memory_path,
            no_totp,
            allowed_root,
            allowed_commands,
        } => {
            commands::onboard::OnboardCommand::run(
                &ctx,
                commands::onboard::OnboardOptions {
                    interactive,
                    force,
                    channels_only,
                    api_key,
                    yes,
                    provider,
                    base_url,
                    model,
                    memory,
                    memory_path,
                    no_totp,
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
        Commands::Daemon { command } => commands::daemon::DaemonCommand::run(&ctx, command).await,
        Commands::Status => commands::status::StatusCommand::run(&ctx, ()).await,
        Commands::Agent {
            message,
            provider,
            model,
            profile,
        } => {
            commands::agent::AgentCommand::run(
                &ctx,
                commands::agent::AgentOptions {
                    message,
                    provider,
                    model,
                    profile,
                },
            )
            .await
        }
        Commands::Auth { command } => commands::auth::AuthCommand::run(&ctx, command).await,
        Commands::Cron { command } => commands::cron::CronCommand::run(&ctx, command).await,
        Commands::Hooks { command } => commands::hooks::HooksCommand::run(&ctx, command).await,
        Commands::Skill { command } => commands::skill::SkillCommand::run(&ctx, command).await,
        Commands::Tunnel { command } => commands::tunnel::TunnelCommand::run(&ctx, command).await,
        Commands::Plugin { command } => commands::plugin::PluginCommand::run(&ctx, command).await,
        Commands::Providers { json, no_color } => {
            commands::providers::ProvidersCommand::run(
                &ctx,
                commands::providers::ProvidersOptions { json, no_color },
            )
            .await
        }
        Commands::Estop {
            level,
            domains,
            tools,
            require_otp,
            command,
        } => {
            commands::estop::EstopCommand::run(
                &ctx,
                commands::estop::EstopOptions {
                    level,
                    domains,
                    tools,
                    require_otp,
                    command,
                },
            )
            .await
        }
        Commands::Channel { command } => {
            commands::channel::ChannelCommand::run(&ctx, command).await
        }
        Commands::Integrations { command } => {
            commands::integrations::IntegrationsCommand::run(&ctx, command).await
        }
        Commands::Local { command } => commands::local::LocalCommand::run(&ctx, command).await,
        Commands::Models { command } => commands::models::ModelsCommand::run(&ctx, command).await,
        Commands::Approval { command } => {
            commands::approval::ApprovalCommand::run(&ctx, command).await
        }
        Commands::Identity { command } => {
            commands::identity::IdentityCommand::run(&ctx, command).await
        }
        Commands::Coordination { command } => {
            commands::coordination::CoordinationCommand::run(&ctx, command).await
        }
        Commands::Cost { command } => commands::cost::CostCommand::run(&ctx, command).await,
        Commands::Goals { command } => commands::goals::GoalsCommand::run(&ctx, command).await,
        Commands::Doctor { command } => commands::doctor::DoctorCommand::run(&ctx, command).await,
        Commands::Service {
            service_init: _service_init,
            command,
        } => commands::service::ServiceCommand::run(&ctx, command).await,
        Commands::Dashboard => commands::dashboard::DashboardCommand::run(&ctx, ()).await,
        Commands::Migrate { command } => commands::update::MigrateCommand::run(&ctx, command).await,
        Commands::Update { check, command } => {
            let resolved = command.unwrap_or_else(|| {
                if check {
                    crate::cli::UpdateCommands::Check {
                        channel: "stable".to_string(),
                        json: false,
                    }
                } else {
                    crate::cli::UpdateCommands::Status { json: false }
                }
            });
            commands::update::UpdateCommand::run(&ctx, resolved).await
        }
        Commands::Completions { shell } => {
            commands::completions::CompletionsCommand::run(&ctx, shell).await
        }
        Commands::Config { command } => commands::config::ConfigCommand::run(&ctx, command).await,
        Commands::Memory { command } => commands::memory::MemoryCommand::run(&ctx, command).await,
        Commands::Rag { command } => commands::rag::RagCommand::run(&ctx, command).await,
        Commands::Hardware { command } => {
            commands::hardware::HardwareCommand::run(&ctx, command).await
        }
        Commands::Peripheral { command } => {
            commands::peripheral::PeripheralCommand::run(&ctx, command).await
        }
        Commands::ProvidersQuota { provider, json } => {
            commands::providers::ProvidersQuotaCommand::run(
                &ctx,
                commands::providers::ProvidersQuotaOptions { provider, json },
            )
            .await
        }
        Commands::Template { command } => {
            commands::template::TemplateCommand::run(&ctx, command).await
        }
    }
}
