mod app;
mod cli;
mod command_core;
mod commands;

use agentzero_common::init_tracing;
use clap::Parser;
use cli::Cli;
use gag::BufferRedirect;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::io::Read;

pub fn parse_cli_from<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let raw = args.into_iter().map(Into::into).collect::<Vec<OsString>>();
    let normalized = normalize_verbose_args(raw)?;
    Cli::try_parse_from(normalized)
}

pub async fn execute(cli: Cli) -> anyhow::Result<()> {
    init_tracing(cli.verbose);
    if cli.json {
        run_with_global_json(cli).await
    } else {
        app::run(cli).await
    }
}

pub async fn run() -> anyhow::Result<()> {
    let cli = parse_cli_from(std::env::args_os())?;
    execute(cli).await
}

pub async fn cli() -> anyhow::Result<()> {
    run().await
}

async fn run_with_global_json(cli: Cli) -> anyhow::Result<()> {
    let command = command_label(&cli.command);
    let mut redirect = BufferRedirect::stdout()?;
    let result = app::run(cli).await;

    let mut captured = String::new();
    redirect.read_to_string(&mut captured)?;
    drop(redirect);

    let payload = match result.as_ref() {
        Ok(()) => json!({
            "ok": true,
            "command": command,
            "result": parse_captured_output(&captured),
        }),
        Err(err) => json!({
            "ok": false,
            "command": command,
            "result": parse_captured_output(&captured),
            "error": err.to_string(),
        }),
    };
    println!("{}", serde_json::to_string_pretty(&payload)?);

    result
}

fn parse_captured_output(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return json!({});
    }

    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(map)) => Value::Object(map),
        Ok(other) => json!({ "data": other }),
        Err(_) => json!({ "text": trimmed }),
    }
}

fn command_label(command: &crate::cli::Commands) -> &'static str {
    use crate::cli::Commands;

    match command {
        Commands::Onboard { .. } => "onboard",
        Commands::Gateway { .. } => "gateway",
        Commands::Daemon { .. } => "daemon",
        Commands::Agent { .. } => "agent",
        Commands::Auth { .. } => "auth",
        Commands::Cron { .. } => "cron",
        Commands::Hooks { .. } => "hooks",
        Commands::Skill { .. } => "skill",
        Commands::Tunnel { .. } => "tunnel",
        Commands::Plugin { .. } => "plugin",
        Commands::Providers { .. } => "providers",
        Commands::Estop { .. } => "estop",
        Commands::Channel { .. } => "channel",
        Commands::Integrations { .. } => "integrations",
        Commands::Local { .. } => "local",
        Commands::Models { .. } => "models",
        Commands::Approval { .. } => "approval",
        Commands::Identity { .. } => "identity",
        Commands::Coordination { .. } => "coordination",
        Commands::Cost { .. } => "cost",
        Commands::Goals { .. } => "goals",
        Commands::Status => "status",
        Commands::Doctor { .. } => "doctor",
        Commands::Service { .. } => "service",
        Commands::Dashboard => "dashboard",
        Commands::Migrate { .. } => "migrate",
        Commands::Update { .. } => "update",
        Commands::Completions { .. } => "completions",
        Commands::Config { .. } => "config",
        Commands::Memory { .. } => "memory",
        Commands::Rag { .. } => "rag",
        Commands::Hardware { .. } => "hardware",
        Commands::Peripheral { .. } => "peripheral",
        Commands::ProvidersQuota { .. } => "providers-quota",
    }
}

fn normalize_verbose_args(args: Vec<OsString>) -> Result<Vec<OsString>, clap::Error> {
    let mut normalized = Vec::with_capacity(args.len());
    let mut idx = 0usize;

    while idx < args.len() {
        let current = &args[idx];
        if current == "--verbose" {
            if let Some(next) = args.get(idx + 1).and_then(|value| value.to_str()) {
                if next.chars().all(|ch| ch.is_ascii_digit()) {
                    let level = next.parse::<usize>().map_err(|_| {
                        clap::Error::raw(
                            clap::error::ErrorKind::ValueValidation,
                            "--verbose numeric level must be between 1 and 4",
                        )
                    })?;

                    if !(1..=4).contains(&level) {
                        return Err(clap::Error::raw(
                            clap::error::ErrorKind::ValueValidation,
                            "--verbose numeric level must be between 1 and 4",
                        ));
                    }

                    for _ in 0..level {
                        normalized.push(OsString::from("-v"));
                    }
                    idx += 2;
                    continue;
                }
            }
        }

        normalized.push(current.clone());
        idx += 1;
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::parse_cli_from;
    use crate::cli::{
        ApprovalCommands, ApprovalRisk, AuthCommands, ChannelCommands, Cli, Commands,
        CompletionShell, ConfigCommands, CoordinationCommands, CostCommands, CronCommands,
        DoctorCommands, EstopCommands, EstopLevel, GoalCommands, HardwareCommands, HookCommands,
        IdentityCommands, IdentityKind, IntegrationsCommands, MemoryCommands, MigrateCommands,
        ModelCommands, PeripheralCommands, PluginCommands, RagCommands, ServiceCommands,
        ServiceInit, SkillCommands, TunnelCommands, UpdateCommands,
    };
    use clap::{ColorChoice, CommandFactory};

    #[test]
    fn parse_cli_from_parses_status_command() {
        let parsed = parse_cli_from(["agentzero", "status"]).expect("status should parse");
        assert!(matches!(parsed.command, Commands::Status));
    }

    #[test]
    fn parse_cli_from_parses_onboard_parity_flags_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "onboard",
            "--interactive",
            "--force",
            "--channels-only",
            "--api-key",
            "sk-test",
            "--provider",
            "openrouter",
            "--model",
            "openai/gpt-4o-mini",
            "--memory",
            "sqlite",
            "--no-totp",
        ])
        .expect("onboard parity flags should parse");
        assert!(matches!(parsed.command, Commands::Onboard { .. }));
    }

    #[test]
    fn parse_cli_from_parses_approval_evaluate_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "approval",
            "evaluate",
            "--actor",
            "operator-1",
            "--action",
            "wipe_data",
            "--risk",
            "high",
        ])
        .expect("approval evaluate should parse");
        assert!(matches!(
            parsed.command,
            Commands::Approval {
                command: ApprovalCommands::Evaluate {
                    risk: ApprovalRisk::High,
                    ..
                }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_approval_without_subcommand() {
        let err = parse_cli_from(["agentzero", "approval"])
            .expect_err("approval without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_identity_upsert_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "identity",
            "upsert",
            "--id",
            "operator-1",
            "--name",
            "Operator",
            "--kind",
            "human",
        ])
        .expect("identity upsert should parse");
        assert!(matches!(
            parsed.command,
            Commands::Identity {
                command: IdentityCommands::Upsert {
                    kind: IdentityKind::Human,
                    ..
                }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_identity_without_subcommand() {
        let err = parse_cli_from(["agentzero", "identity"])
            .expect_err("identity without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_coordination_set_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "coordination",
            "set",
            "--active-workers",
            "2",
            "--queued-tasks",
            "5",
        ])
        .expect("coordination set should parse");
        assert!(matches!(
            parsed.command,
            Commands::Coordination {
                command: CoordinationCommands::Set { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_coordination_without_subcommand() {
        let err = parse_cli_from(["agentzero", "coordination"])
            .expect_err("coordination without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_cost_record_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "cost",
            "record",
            "--tokens",
            "200",
            "--usd",
            "0.04",
        ])
        .expect("cost record should parse");
        assert!(matches!(
            parsed.command,
            Commands::Cost {
                command: CostCommands::Record { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_cost_without_subcommand() {
        let err =
            parse_cli_from(["agentzero", "cost"]).expect_err("cost without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_goals_add_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "goals",
            "add",
            "--id",
            "g1",
            "--title",
            "Ship feature",
        ])
        .expect("goals add should parse");
        assert!(matches!(
            parsed.command,
            Commands::Goals {
                command: GoalCommands::Add { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_goals_without_subcommand() {
        let err = parse_cli_from(["agentzero", "goals"])
            .expect_err("goals without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_doctor_models_command() {
        let parsed =
            parse_cli_from(["agentzero", "doctor", "models"]).expect("doctor models should parse");
        assert!(matches!(
            parsed.command,
            Commands::Doctor {
                command: DoctorCommands::Models { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_doctor_traces_command() {
        let parsed = parse_cli_from(["agentzero", "doctor", "traces", "--limit", "10"])
            .expect("doctor traces should parse");
        assert!(matches!(
            parsed.command,
            Commands::Doctor {
                command: DoctorCommands::Traces { limit: 10, .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_doctor_without_subcommand() {
        let err = parse_cli_from(["agentzero", "doctor"])
            .expect_err("doctor without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_service_install_command() {
        let parsed = parse_cli_from(["agentzero", "service", "install"])
            .expect("service install should parse");
        assert!(matches!(
            parsed.command,
            Commands::Service {
                service_init: ServiceInit::Auto,
                command: ServiceCommands::Install
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_service_restart_with_service_init_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "service",
            "--service-init",
            "systemd",
            "restart",
        ])
        .expect("service restart should parse");
        assert!(matches!(
            parsed.command,
            Commands::Service {
                service_init: ServiceInit::Systemd,
                command: ServiceCommands::Restart
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_service_uninstall_command() {
        let parsed = parse_cli_from(["agentzero", "service", "uninstall"])
            .expect("service uninstall should parse");
        assert!(matches!(
            parsed.command,
            Commands::Service {
                service_init: ServiceInit::Auto,
                command: ServiceCommands::Uninstall
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_service_without_subcommand() {
        let err = parse_cli_from(["agentzero", "service"])
            .expect_err("service without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_dashboard_command() {
        let parsed = parse_cli_from(["agentzero", "dashboard"]).expect("dashboard should parse");
        assert!(matches!(parsed.command, Commands::Dashboard));
    }

    #[test]
    fn parse_cli_from_parses_daemon_command() {
        let parsed = parse_cli_from(["agentzero", "daemon", "--port", "42617"])
            .expect("daemon should parse");
        assert!(matches!(parsed.command, Commands::Daemon { .. }));
    }

    #[test]
    fn parse_cli_from_parses_daemon_with_defaults() {
        let parsed = parse_cli_from(["agentzero", "daemon"]).expect("daemon should parse");
        assert!(matches!(parsed.command, Commands::Daemon { .. }));
    }

    #[test]
    fn parse_cli_from_parses_cron_add_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "cron",
            "add",
            "--id",
            "backup",
            "--schedule",
            "0 * * * *",
            "--command",
            "agentzero status",
        ])
        .expect("cron add should parse");
        assert!(matches!(
            parsed.command,
            Commands::Cron {
                command: CronCommands::Add { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_cron_add_at_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "cron",
            "add-at",
            "--id",
            "backup-at",
            "--schedule",
            "0 * * * *",
            "--command",
            "agentzero status",
        ])
        .expect("cron add-at should parse");
        assert!(matches!(
            parsed.command,
            Commands::Cron {
                command: CronCommands::AddAt { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_cron_add_every_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "cron",
            "add-every",
            "--id",
            "backup-every",
            "--schedule",
            "*/5 * * * *",
            "--command",
            "agentzero status",
        ])
        .expect("cron add-every should parse");
        assert!(matches!(
            parsed.command,
            Commands::Cron {
                command: CronCommands::AddEvery { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_cron_once_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "cron",
            "once",
            "--id",
            "backup-once",
            "--schedule",
            "@once",
            "--command",
            "agentzero status",
        ])
        .expect("cron once should parse");
        assert!(matches!(
            parsed.command,
            Commands::Cron {
                command: CronCommands::Once { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_cron_without_subcommand() {
        let err =
            parse_cli_from(["agentzero", "cron"]).expect_err("cron without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_hooks_test_command() {
        let parsed = parse_cli_from(["agentzero", "hooks", "test", "--name", "before_run"])
            .expect("hooks test should parse");
        assert!(matches!(
            parsed.command,
            Commands::Hooks {
                command: HookCommands::Test { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_hooks_without_subcommand() {
        let err = parse_cli_from(["agentzero", "hooks"])
            .expect_err("hooks without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_estop_engage_via_top_level_flags_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "estop",
            "--level",
            "network-kill",
            "--domain",
            "*.example.com",
            "--tool",
            "shell",
        ])
        .expect("estop engage should parse");
        assert!(matches!(
            parsed.command,
            Commands::Estop {
                level: Some(EstopLevel::NetworkKill),
                command: None,
                ..
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_estop_resume_subcommand() {
        let parsed = parse_cli_from([
            "agentzero",
            "estop",
            "resume",
            "--network",
            "--domain",
            "*.example.com",
            "--tool",
            "shell",
        ])
        .expect("estop resume should parse");
        assert!(matches!(
            parsed.command,
            Commands::Estop {
                command: Some(EstopCommands::Resume { .. }),
                ..
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_channel_list_command() {
        let parsed =
            parse_cli_from(["agentzero", "channel", "list"]).expect("channel list should parse");
        assert!(matches!(
            parsed.command,
            Commands::Channel {
                command: ChannelCommands::List
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_channel_bind_telegram_command() {
        let parsed = parse_cli_from(["agentzero", "channel", "bind-telegram"])
            .expect("channel bind-telegram should parse");
        assert!(matches!(
            parsed.command,
            Commands::Channel {
                command: ChannelCommands::BindTelegram
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_channel_without_subcommand() {
        let err = parse_cli_from(["agentzero", "channel"])
            .expect_err("channel without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_integrations_search_command() {
        let parsed = parse_cli_from(["agentzero", "integrations", "search", "--query", "discord"])
            .expect("integrations search should parse");
        assert!(matches!(
            parsed.command,
            Commands::Integrations {
                command: IntegrationsCommands::Search { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_integrations_without_subcommand() {
        let err = parse_cli_from(["agentzero", "integrations"])
            .expect_err("integrations without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_skill_install_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "skill",
            "install",
            "--name",
            "my_skill",
            "--source",
            "local",
        ])
        .expect("skill install should parse");
        assert!(matches!(
            parsed.command,
            Commands::Skill {
                command: SkillCommands::Install { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_skill_without_subcommand() {
        let err = parse_cli_from(["agentzero", "skill"])
            .expect_err("skill without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_tunnel_start_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "tunnel",
            "start",
            "--name",
            "default",
            "--protocol",
            "https",
            "--remote",
            "example.com:443",
            "--local-port",
            "9422",
        ])
        .expect("tunnel start should parse");
        assert!(matches!(
            parsed.command,
            Commands::Tunnel {
                command: TunnelCommands::Start { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_tunnel_without_subcommand() {
        let err = parse_cli_from(["agentzero", "tunnel"])
            .expect_err("tunnel without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_plugin_package_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "plugin",
            "package",
            "--manifest",
            "manifest.json",
            "--wasm",
            "plugin.wasm",
            "--out",
            "plugin.tar",
        ])
        .expect("plugin package should parse");
        assert!(matches!(
            parsed.command,
            Commands::Plugin {
                command: PluginCommands::Package { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_plugin_dev_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "plugin",
            "dev",
            "--manifest",
            "manifest.json",
            "--wasm",
            "plugin.wasm",
            "--iterations",
            "3",
        ])
        .expect("plugin dev should parse");
        assert!(matches!(
            parsed.command,
            Commands::Plugin {
                command: PluginCommands::Dev { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_plugin_list_command() {
        let parsed = parse_cli_from(["agentzero", "plugin", "list", "--json"])
            .expect("plugin list should parse");
        assert!(matches!(
            parsed.command,
            Commands::Plugin {
                command: PluginCommands::List { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_plugin_without_subcommand() {
        let err = parse_cli_from(["agentzero", "plugin"])
            .expect_err("plugin without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_migrate_openclaw_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "migrate",
            "openclaw",
            "--source",
            "/tmp/source",
            "--dry-run",
        ])
        .expect("migrate openclaw should parse");
        assert!(matches!(
            parsed.command,
            Commands::Migrate {
                command: MigrateCommands::Openclaw { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_migrate_without_subcommand() {
        let err = parse_cli_from(["agentzero", "migrate"])
            .expect_err("migrate without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_update_apply_command() {
        let parsed = parse_cli_from(["agentzero", "update", "apply", "--version", "0.2.0"])
            .expect("update apply should parse");
        assert!(matches!(
            parsed.command,
            Commands::Update {
                command: Some(UpdateCommands::Apply { .. }),
                ..
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_update_without_subcommand() {
        let parsed = parse_cli_from(["agentzero", "update"]).expect("update should parse");
        assert!(matches!(
            parsed.command,
            Commands::Update { command: None, .. }
        ));
    }

    #[test]
    fn parse_cli_from_parses_update_check_flag() {
        let parsed = parse_cli_from(["agentzero", "update", "--check"])
            .expect("update --check should parse");
        assert!(matches!(
            parsed.command,
            Commands::Update { check: true, .. }
        ));
    }

    #[test]
    fn parse_cli_from_parses_rag_ingest_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "rag",
            "ingest",
            "--id",
            "doc-1",
            "--text",
            "hello",
        ])
        .expect("rag ingest should parse");
        assert!(matches!(
            parsed.command,
            Commands::Rag {
                command: RagCommands::Ingest { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_rag_without_subcommand() {
        let err =
            parse_cli_from(["agentzero", "rag"]).expect_err("rag without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_hardware_discover_command() {
        let parsed = parse_cli_from(["agentzero", "hardware", "discover"])
            .expect("hardware discover should parse");
        assert!(matches!(
            parsed.command,
            Commands::Hardware {
                command: HardwareCommands::Discover
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_hardware_info_default_chip_command() {
        let parsed =
            parse_cli_from(["agentzero", "hardware", "info"]).expect("hardware info should parse");
        assert!(matches!(
            parsed.command,
            Commands::Hardware {
                command: HardwareCommands::Info { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_hardware_introspect_command() {
        let parsed = parse_cli_from(["agentzero", "hardware", "introspect"])
            .expect("hardware introspect should parse");
        assert!(matches!(
            parsed.command,
            Commands::Hardware {
                command: HardwareCommands::Introspect
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_hardware_without_subcommand() {
        let err = parse_cli_from(["agentzero", "hardware"])
            .expect_err("hardware without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_peripheral_add_command() {
        let parsed = parse_cli_from(["agentzero", "peripheral", "add"])
            .expect("peripheral add should parse");
        assert!(matches!(
            parsed.command,
            Commands::Peripheral {
                command: PeripheralCommands::Add { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_peripheral_flash_command() {
        let parsed = parse_cli_from(["agentzero", "peripheral", "flash"])
            .expect("peripheral flash should parse");
        assert!(matches!(
            parsed.command,
            Commands::Peripheral {
                command: PeripheralCommands::Flash { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_peripheral_flash_nucleo_command() {
        let parsed = parse_cli_from(["agentzero", "peripheral", "flash-nucleo"])
            .expect("peripheral flash-nucleo should parse");
        assert!(matches!(
            parsed.command,
            Commands::Peripheral {
                command: PeripheralCommands::FlashNucleo { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_peripheral_setup_uno_q_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "peripheral",
            "setup-uno-q",
            "--host",
            "192.168.0.48",
        ])
        .expect("peripheral setup-uno-q should parse");
        assert!(matches!(
            parsed.command,
            Commands::Peripheral {
                command: PeripheralCommands::SetupUnoQ { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_peripheral_without_subcommand() {
        let err = parse_cli_from(["agentzero", "peripheral"])
            .expect_err("peripheral without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_completions_command() {
        let parsed = parse_cli_from(["agentzero", "completions", "--shell", "bash"])
            .expect("completions should parse");
        assert!(matches!(
            parsed.command,
            Commands::Completions {
                shell: CompletionShell::Bash
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_config_schema_command() {
        let parsed = parse_cli_from(["agentzero", "config", "schema", "--json"])
            .expect("config schema should parse");
        assert!(matches!(
            parsed.command,
            Commands::Config {
                command: ConfigCommands::Schema { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_config_without_subcommand() {
        let err =
            parse_cli_from(["agentzero", "config"]).expect_err("config without subcommand fails");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_memory_list_command() {
        let parsed = parse_cli_from(["agentzero", "memory", "list", "--limit", "10"])
            .expect("memory list should parse");
        assert!(matches!(
            parsed.command,
            Commands::Memory {
                command: MemoryCommands::List { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_memory_get_without_key_command() {
        let parsed = parse_cli_from(["agentzero", "memory", "get"])
            .expect("memory get without key should parse");
        assert!(matches!(
            parsed.command,
            Commands::Memory {
                command: MemoryCommands::Get { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_memory_clear_with_key_and_json_command() {
        let parsed = parse_cli_from(["agentzero", "memory", "clear", "--key", "user", "--json"])
            .expect("memory clear should parse");
        assert!(matches!(
            parsed.command,
            Commands::Memory {
                command: MemoryCommands::Clear { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_memory_without_subcommand() {
        let err =
            parse_cli_from(["agentzero", "memory"]).expect_err("memory without subcommand fails");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_gateway_with_new_pairing_flag() {
        let parsed = parse_cli_from(["agentzero", "gateway", "--new-pairing"])
            .expect("gateway --new-pairing should parse");
        assert!(matches!(
            parsed.command,
            Commands::Gateway {
                new_pairing: true,
                ..
            }
        ));
    }

    #[test]
    fn parse_cli_from_gateway_defaults_new_pairing_to_false() {
        let parsed = parse_cli_from(["agentzero", "gateway"]).expect("gateway should parse");
        assert!(matches!(
            parsed.command,
            Commands::Gateway {
                new_pairing: false,
                ..
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_auth_login_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "auth",
            "login",
            "--provider",
            "openai-codex",
            "--profile",
            "default",
        ])
        .expect("auth login should parse");
        assert!(matches!(
            parsed.command,
            Commands::Auth {
                command: AuthCommands::Login { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_auth_login_without_token() {
        let err = parse_cli_from([
            "agentzero",
            "auth",
            "login",
            "--profile",
            "default",
            "--provider",
            "openai-codex",
        ])
        .expect("auth login should parse without token");
        assert!(matches!(
            err.command,
            Commands::Auth {
                command: AuthCommands::Login { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_auth_login_without_provider() {
        let err = parse_cli_from(["agentzero", "auth", "login", "--profile", "default"])
            .expect_err("auth login missing provider should fail");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn parse_cli_from_parses_auth_paste_token_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "auth",
            "paste-token",
            "--profile",
            "anthropic-sub",
            "--provider",
            "anthropic",
            "--token",
            "tok",
        ])
        .expect("auth paste-token should parse");
        assert!(matches!(
            parsed.command,
            Commands::Auth {
                command: AuthCommands::PasteToken { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_auth_paste_token_without_token_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "auth",
            "paste-token",
            "--profile",
            "anthropic-sub",
            "--provider",
            "anthropic",
            "--auth-kind",
            "api-key",
        ])
        .expect("auth paste-token without token should parse");
        assert!(matches!(
            parsed.command,
            Commands::Auth {
                command: AuthCommands::PasteToken { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_auth_refresh_command() {
        let parsed = parse_cli_from([
            "agentzero",
            "auth",
            "refresh",
            "--provider",
            "openai-codex",
            "--profile",
            "default",
        ])
        .expect("auth refresh should parse");
        assert!(matches!(
            parsed.command,
            Commands::Auth {
                command: AuthCommands::Refresh { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_auth_refresh_without_provider() {
        let err = parse_cli_from(["agentzero", "auth", "refresh"])
            .expect_err("auth refresh missing provider should fail");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn parse_cli_from_rejects_auth_paste_redirect_without_redirect() {
        let err = parse_cli_from([
            "agentzero",
            "auth",
            "paste-redirect",
            "--profile",
            "default",
            "--provider",
            "openai-codex",
        ])
        .expect("paste-redirect should parse without --input (interactive fallback)");
        assert!(matches!(
            err.command,
            Commands::Auth {
                command: AuthCommands::PasteRedirect { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_auth_logout_with_provider() {
        let parsed = parse_cli_from(["agentzero", "auth", "logout", "--provider", "openai-codex"])
            .expect("auth logout should parse");
        assert!(matches!(
            parsed.command,
            Commands::Auth {
                command: AuthCommands::Logout { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_auth_use_with_provider_and_profile() {
        let parsed = parse_cli_from([
            "agentzero",
            "auth",
            "use",
            "--provider",
            "openai-codex",
            "--profile",
            "default",
        ])
        .expect("auth use should parse");
        assert!(matches!(
            parsed.command,
            Commands::Auth {
                command: AuthCommands::Use { .. }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_auth_logout_without_provider() {
        let err = parse_cli_from(["agentzero", "auth", "logout"])
            .expect_err("auth logout missing provider should fail");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn parse_cli_from_parses_providers_command() {
        let parsed = parse_cli_from(["agentzero", "providers"]).expect("providers should parse");
        assert!(matches!(
            parsed.command,
            Commands::Providers {
                json: false,
                no_color: false
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_models_list_command() {
        let parsed =
            parse_cli_from(["agentzero", "models", "list"]).expect("models list should parse");
        assert!(matches!(
            parsed.command,
            Commands::Models {
                command: ModelCommands::List { provider: None }
            }
        ));
    }

    #[test]
    fn parse_cli_from_parses_models_refresh_with_flags() {
        let parsed = parse_cli_from([
            "agentzero",
            "models",
            "refresh",
            "--provider",
            "openai",
            "--force",
        ])
        .expect("models refresh flags should parse");
        assert!(matches!(
            parsed.command,
            Commands::Models {
                command: ModelCommands::Refresh {
                    provider: Some(_),
                    all: false,
                    force: true
                }
            }
        ));
    }

    #[test]
    fn parse_cli_from_rejects_models_without_subcommand() {
        let err = parse_cli_from(["agentzero", "models"])
            .expect_err("models without subcommand should fail");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parse_cli_from_parses_providers_json_and_no_color_flags() {
        let parsed = parse_cli_from(["agentzero", "providers", "--json", "--no-color"])
            .expect("providers flags should parse");
        assert!(matches!(
            parsed.command,
            Commands::Providers {
                json: true,
                no_color: true
            }
        ));
    }

    #[test]
    fn parse_cli_from_doctor_help_includes_expected_description() {
        let err = parse_cli_from(["agentzero", "doctor", "--help"])
            .expect_err("doctor --help should return clap display help");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
        assert!(err
            .to_string()
            .contains("Run diagnostics for daemon/scheduler/channel freshness"));
    }

    #[test]
    fn parse_cli_from_parses_global_config_flag() {
        let parsed = parse_cli_from(["agentzero", "--config", "/tmp/cfg.toml", "status"])
            .expect("--config should parse");
        assert_eq!(
            parsed
                .config
                .as_ref()
                .expect("config should be present")
                .to_string_lossy(),
            "/tmp/cfg.toml"
        );
    }

    #[test]
    fn parse_cli_from_parses_global_data_dir_flag() {
        let parsed = parse_cli_from(["agentzero", "--data-dir", "/tmp/agentzero", "status"])
            .expect("--data-dir should parse");
        assert_eq!(
            parsed
                .data_dir
                .as_ref()
                .expect("data_dir should be present")
                .to_string_lossy(),
            "/tmp/agentzero"
        );
    }

    #[test]
    fn parse_cli_from_parses_global_config_dir_alias() {
        let parsed = parse_cli_from(["agentzero", "--config-dir", "/tmp/agentzero", "status"])
            .expect("--config-dir alias should parse");
        assert_eq!(
            parsed
                .data_dir
                .as_ref()
                .expect("data_dir should be present")
                .to_string_lossy(),
            "/tmp/agentzero"
        );
    }

    #[test]
    fn parse_cli_from_parses_global_verbose_flag() {
        let parsed =
            parse_cli_from(["agentzero", "--verbose", "status"]).expect("--verbose should parse");
        assert_eq!(parsed.verbose, 1);
    }

    #[test]
    fn parse_cli_from_parses_global_json_flag() {
        let parsed =
            parse_cli_from(["agentzero", "--json", "status"]).expect("--json should parse");
        assert!(parsed.json);
        assert!(matches!(parsed.command, Commands::Status));
    }

    #[test]
    fn parse_cli_from_parses_verbose_count() {
        let parsed = parse_cli_from(["agentzero", "-vvv", "status"]).expect("-vvv should parse");
        assert_eq!(parsed.verbose, 3);
    }

    #[test]
    fn parse_cli_from_parses_numeric_verbose_level() {
        let parsed = parse_cli_from(["agentzero", "--verbose", "4", "status"])
            .expect("--verbose 4 should parse");
        assert_eq!(parsed.verbose, 4);
    }

    #[test]
    fn parse_cli_from_rejects_out_of_range_numeric_verbose_level() {
        let err = parse_cli_from(["agentzero", "--verbose", "0", "status"])
            .expect_err("--verbose 0 should fail");
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn parse_cli_from_rejects_agent_without_message() {
        let err = parse_cli_from(["agentzero", "agent"])
            .expect_err("missing required message should fail");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn cli_command_enforces_colorized_output() {
        let cmd = Cli::command();
        assert_eq!(cmd.get_color(), ColorChoice::Always);
    }
}
