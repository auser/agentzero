mod app;
mod cli;
mod command_core;
mod commands;

use agentzero_common::init_tracing;
use clap::Parser;
use cli::Cli;
use std::ffi::OsString;

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
    app::run(cli).await
}

pub async fn run() -> anyhow::Result<()> {
    let cli = parse_cli_from(std::env::args_os())?;
    execute(cli).await
}

pub async fn cli() -> anyhow::Result<()> {
    run().await
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
    use crate::cli::{Cli, Commands};
    use clap::{ColorChoice, CommandFactory};

    #[test]
    fn parse_cli_from_parses_status_command() {
        let parsed = parse_cli_from(["agentzero", "status"]).expect("status should parse");
        assert!(matches!(parsed.command, Commands::Status { json: false }));
    }

    #[test]
    fn parse_cli_from_parses_status_json_flag() {
        let parsed =
            parse_cli_from(["agentzero", "status", "--json"]).expect("status --json should parse");
        assert!(matches!(parsed.command, Commands::Status { json: true }));
    }

    #[test]
    fn parse_cli_from_parses_doctor_command() {
        let parsed = parse_cli_from(["agentzero", "doctor"]).expect("doctor should parse");
        assert!(matches!(parsed.command, Commands::Doctor));
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
    fn parse_cli_from_parses_global_verbose_flag() {
        let parsed =
            parse_cli_from(["agentzero", "--verbose", "status"]).expect("--verbose should parse");
        assert_eq!(parsed.verbose, 1);
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
