use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_config::{load, load_env_var};
use agentzero_providers::find_provider;
use anyhow::Context;
use async_trait::async_trait;
use console::style;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

pub struct DoctorCommand;

#[async_trait]
impl AgentZeroCommand for DoctorCommand {
    type Options = ();

    async fn run(ctx: &CommandContext, _opts: Self::Options) -> anyhow::Result<()> {
        let report = collect_report(ctx, &run_command);
        let mut stdout = io::stdout();
        render_report(&report, &mut stdout)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    Ok,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
struct Check {
    section: &'static str,
    severity: Severity,
    message: String,
}

#[derive(Debug, Default)]
struct DoctorReport {
    checks: Vec<Check>,
}

impl DoctorReport {
    fn ok(&mut self, section: &'static str, message: impl Into<String>) {
        self.checks.push(Check {
            section,
            severity: Severity::Ok,
            message: message.into(),
        });
    }

    fn warn(&mut self, section: &'static str, message: impl Into<String>) {
        self.checks.push(Check {
            section,
            severity: Severity::Warn,
            message: message.into(),
        });
    }

    fn error(&mut self, section: &'static str, message: impl Into<String>) {
        self.checks.push(Check {
            section,
            severity: Severity::Error,
            message: message.into(),
        });
    }

    fn counts(&self) -> (usize, usize, usize) {
        let mut ok = 0usize;
        let mut warnings = 0usize;
        let mut errors = 0usize;
        for check in &self.checks {
            match check.severity {
                Severity::Ok => ok += 1,
                Severity::Warn => warnings += 1,
                Severity::Error => errors += 1,
            }
        }
        (ok, warnings, errors)
    }
}

fn collect_report(
    ctx: &CommandContext,
    run: &impl Fn(&str, &[&str]) -> anyhow::Result<String>,
) -> DoctorReport {
    let mut report = DoctorReport::default();
    collect_config_checks(ctx, &mut report);
    collect_workspace_checks(ctx, run, &mut report);
    collect_daemon_checks(ctx, &mut report);
    collect_environment_checks(run, &mut report);
    collect_cli_tool_checks(run, &mut report);
    report
}

fn collect_config_checks(ctx: &CommandContext, report: &mut DoctorReport) {
    let config_path = &ctx.config_path;
    if config_path.exists() {
        report.ok("config", format!("config file: {}", config_path.display()));
    } else {
        report.error(
            "config",
            format!("config file not found: {}", config_path.display()),
        );
    }

    match load(config_path) {
        Ok(config) => {
            let provider = config.provider.kind.as_str();
            if find_provider(provider).is_some() {
                report.ok("config", format!("provider `{provider}` is valid"));
            } else {
                report.warn(
                    "config",
                    format!("provider `{provider}` is not in known defaults"),
                );
            }

            match load_env_var(config_path, "OPENAI_API_KEY") {
                Ok(Some(_)) => report.ok("config", "OPENAI_API_KEY is set"),
                Ok(None) => report.warn(
                    "config",
                    "OPENAI_API_KEY is not set (may rely on environment-specific defaults)",
                ),
                Err(err) => report.warn("config", format!("failed to load OPENAI_API_KEY: {err}")),
            }

            if config.provider.model.trim().is_empty() {
                report.error("config", "provider.model is empty");
            } else {
                report.ok(
                    "config",
                    format!("default model: {}", config.provider.model.trim()),
                );
            }

            report.ok(
                "config",
                format!(
                    "memory backend: {} ({})",
                    config.memory.backend, config.memory.sqlite_path
                ),
            );
        }
        Err(err) => report.error("config", format!("config load failed: {err}")),
    }
}

fn collect_workspace_checks(
    ctx: &CommandContext,
    run: &impl Fn(&str, &[&str]) -> anyhow::Result<String>,
    report: &mut DoctorReport,
) {
    let workspace = &ctx.workspace_root;
    if workspace.exists() {
        report.ok(
            "workspace",
            format!("directory exists: {}", workspace.display()),
        );
    } else {
        report.error(
            "workspace",
            format!("directory missing: {}", workspace.display()),
        );
    }

    match probe_writable(workspace) {
        Ok(()) => report.ok("workspace", "directory is writable"),
        Err(err) => report.error("workspace", format!("directory is not writable: {err}")),
    }

    let required_files = ["AGENTS.md", "specs/SPRINT.md"];
    for file in required_files {
        if workspace.join(file).exists() {
            report.ok("workspace", format!("{file} present"));
        } else {
            report.warn("workspace", format!("{file} missing"));
        }
    }

    match disk_space_available_mb(workspace, run) {
        Ok(available) => report.ok("workspace", format!("disk space: {available} MB available")),
        Err(err) => report.warn(
            "workspace",
            format!(
                "disk space check unavailable: {}",
                truncate_line(&err.to_string(), 80)
            ),
        ),
    }
}

fn collect_daemon_checks(ctx: &CommandContext, report: &mut DoctorReport) {
    let config_dir = ctx
        .config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let daemon_state = config_dir.join("daemon_state.json");
    if daemon_state.exists() {
        report.ok(
            "daemon",
            format!("state file present: {}", daemon_state.display()),
        );
    } else {
        report.error(
            "daemon",
            format!(
                "state file not found: {} (is the daemon running?)",
                daemon_state.display()
            ),
        );
    }
}

fn collect_environment_checks(
    run: &impl Fn(&str, &[&str]) -> anyhow::Result<String>,
    report: &mut DoctorReport,
) {
    match run("git", &["--version"]) {
        Ok(value) => report.ok("environment", format!("git: {}", truncate_line(&value, 80))),
        Err(err) => report.warn("environment", format!("git unavailable: {err}")),
    }

    if std::env::var("SHELL")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        report.ok("environment", "shell env is set");
    } else {
        report.warn("environment", "SHELL env is not set");
    }

    if std::env::var("HOME")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        report.ok("environment", "home directory env is set");
    } else {
        report.warn("environment", "HOME env is not set");
    }
}

fn collect_cli_tool_checks(
    run: &impl Fn(&str, &[&str]) -> anyhow::Result<String>,
    report: &mut DoctorReport,
) {
    let tools = [
        ("git", "Version Control", "--version"),
        ("python3", "Language", "--version"),
        ("node", "Language", "--version"),
        ("npm", "Package Manager", "--version"),
        ("cargo", "Build", "--version"),
        ("rustc", "Language", "--version"),
    ];

    let mut discovered = 0usize;
    for (cmd, label, version_flag) in tools {
        match run(cmd, &[version_flag]) {
            Ok(value) => {
                report.ok(
                    "cli-tools",
                    format!("{cmd} ({label}) - {}", truncate_line(&value, 100)),
                );
                discovered += 1;
            }
            Err(err) => {
                report.warn("cli-tools", format!("{cmd} ({label}) missing: {err}"));
            }
        }
    }
    report.ok("cli-tools", format!("{discovered} CLI tools discovered"));
}

fn probe_writable(path: &Path) -> anyhow::Result<()> {
    let probe = path.join(".agentzero-doctor-write-check");
    fs::write(&probe, b"ok").with_context(|| format!("cannot create {}", probe.display()))?;
    fs::remove_file(&probe).with_context(|| format!("cannot remove {}", probe.display()))?;
    Ok(())
}

fn disk_space_available_mb(
    path: &Path,
    run: &impl Fn(&str, &[&str]) -> anyhow::Result<String>,
) -> anyhow::Result<u64> {
    let output = run("df", &["-k", path.to_string_lossy().as_ref()])?;
    let line = output
        .lines()
        .nth(1)
        .context("missing df data line in output")?;
    let columns = line.split_whitespace().collect::<Vec<_>>();
    if columns.len() < 4 {
        anyhow::bail!("unexpected df output format");
    }
    let available_kb = columns[3]
        .parse::<u64>()
        .context("failed to parse available KB from df output")?;
    Ok(available_kb / 1024)
}

fn run_command(cmd: &str, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute `{cmd}`"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "`{cmd}` exited with status {}: {}",
            output.status,
            truncate_line(stderr.trim(), 80)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return Ok(stdout);
    }

    Ok(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

fn truncate_line(value: &str, max_len: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_len {
        return trimmed.to_string();
    }
    let mut out = trimmed.chars().take(max_len).collect::<String>();
    out.push_str("...");
    out
}

fn render_report(report: &DoctorReport, writer: &mut dyn Write) -> anyhow::Result<()> {
    writeln!(writer, "AgentZero Doctor (enhanced)").context("failed to write output")?;

    let order = ["config", "workspace", "daemon", "environment", "cli-tools"];
    let sections = report
        .checks
        .iter()
        .map(|check| check.section)
        .collect::<BTreeSet<_>>();

    for section in order {
        if !sections.contains(section) {
            continue;
        }
        writeln!(writer, "\n[{section}]").context("failed to write output")?;
        for check in report
            .checks
            .iter()
            .filter(|check| check.section == section)
        {
            let (symbol, message) = match check.severity {
                Severity::Ok => (style("✅").green().bold(), style(&check.message).white()),
                Severity::Warn => (style("⚠️").yellow().bold(), style(&check.message).yellow()),
                Severity::Error => (style("❌").red().bold(), style(&check.message).red()),
            };
            writeln!(writer, "{} {}", symbol, message).context("failed to write output")?;
        }
    }

    let (ok, warnings, errors) = report.counts();
    writeln!(
        writer,
        "\nSummary: {ok} ok, {warnings} warnings, {errors} errors"
    )
    .context("failed to write output")?;
    if errors > 0 {
        writeln!(
            writer,
            "{}",
            style("Fix the errors above, then run `agentzero doctor` again.").yellow()
        )
        .context("failed to write output")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::collect_report;
    use crate::command_core::CommandContext;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("agentzero-doctor-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn fake_run_command(cmd: &str, _args: &[&str]) -> anyhow::Result<String> {
        if cmd == "df" {
            return Ok(
                "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/mock 2048 1024 1024 50% /"
                    .to_string(),
            );
        }
        Ok(format!("{cmd} version 1.0.0"))
    }

    #[test]
    fn collect_report_flags_missing_config_as_error() {
        let workspace = temp_dir();
        fs::write(workspace.join("AGENTS.md"), "rules").expect("should write AGENTS.md");
        fs::create_dir_all(workspace.join("specs")).expect("should create specs");
        fs::write(workspace.join("specs/SPRINT.md"), "sprint").expect("should write sprint");

        let ctx = CommandContext {
            workspace_root: workspace.clone(),
            data_dir: workspace.clone(),
            config_path: workspace.join("missing.toml"),
        };

        let report = collect_report(&ctx, &fake_run_command);
        assert!(report
            .checks
            .iter()
            .any(|check| check.section == "config"
                && check.message.contains("config file not found")));
        assert!(report.checks.iter().any(
            |check| check.section == "daemon" && check.message.contains("state file not found")
        ));

        fs::remove_dir_all(workspace).expect("temp dir should be removed");
    }

    #[test]
    fn collect_report_emits_workspace_and_tool_successes_when_config_is_valid() {
        let workspace = temp_dir();
        fs::write(workspace.join("AGENTS.md"), "rules").expect("should write AGENTS.md");
        fs::create_dir_all(workspace.join("specs")).expect("should create specs");
        fs::write(workspace.join("specs/SPRINT.md"), "sprint").expect("should write sprint");
        fs::write(workspace.join(".env"), "OPENAI_API_KEY=sk-test\n").expect("should write .env");
        fs::write(
            workspace.join("agentzero.toml"),
            "[provider]\nkind=\"openai\"\nbase_url=\"https://api.openai.com\"\nmodel=\"gpt-4o-mini\"\n\n[memory]\nbackend=\"sqlite\"\nsqlite_path=\"./agentzero.db\"\n",
        )
        .expect("should write config");
        fs::write(workspace.join("daemon_state.json"), "{}").expect("should write daemon state");

        let ctx = CommandContext {
            workspace_root: workspace.clone(),
            data_dir: workspace.clone(),
            config_path: workspace.join("agentzero.toml"),
        };

        let report = collect_report(&ctx, &fake_run_command);
        assert!(report.checks.iter().any(|check| check.section == "config"
            && check.message.contains("provider `openai` is valid")));
        assert!(report.checks.iter().any(
            |check| check.section == "workspace" && check.message.contains("directory exists")
        ));
        assert!(report
            .checks
            .iter()
            .any(|check| check.section == "cli-tools"
                && check.message.contains("CLI tools discovered")));

        fs::remove_dir_all(workspace).expect("temp dir should be removed");
    }
}
