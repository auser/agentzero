use agentzero_config::{load, load_env_var};
use agentzero_health::{assess_freshness, HealthSeverity};
use agentzero_heartbeat::HeartbeatStore;
use agentzero_providers::find_provider;
use anyhow::Context;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct DoctorContext {
    pub workspace_root: PathBuf,
    pub data_dir: PathBuf,
    pub config_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Ok,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub section: String,
    pub severity: Severity,
    pub message: String,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DoctorReport {
    pub checks: Vec<Check>,
}

impl DoctorReport {
    pub fn ok(&mut self, section: impl Into<String>, message: impl Into<String>) {
        self.checks.push(Check {
            section: section.into(),
            severity: Severity::Ok,
            message: message.into(),
            hint: None,
        });
    }

    pub fn warn(&mut self, section: impl Into<String>, message: impl Into<String>) {
        self.checks.push(Check {
            section: section.into(),
            severity: Severity::Warn,
            message: message.into(),
            hint: None,
        });
    }

    pub fn error(&mut self, section: impl Into<String>, message: impl Into<String>) {
        self.checks.push(Check {
            section: section.into(),
            severity: Severity::Error,
            message: message.into(),
            hint: None,
        });
    }

    pub fn warn_with_hint(
        &mut self,
        section: impl Into<String>,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) {
        self.checks.push(Check {
            section: section.into(),
            severity: Severity::Warn,
            message: message.into(),
            hint: Some(hint.into()),
        });
    }

    pub fn error_with_hint(
        &mut self,
        section: impl Into<String>,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) {
        self.checks.push(Check {
            section: section.into(),
            severity: Severity::Error,
            message: message.into(),
            hint: Some(hint.into()),
        });
    }

    pub fn counts(&self) -> (usize, usize, usize) {
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

pub fn collect_report(
    ctx: &DoctorContext,
    run: &impl Fn(&str, &[&str]) -> anyhow::Result<String>,
    now_epoch_seconds: u64,
) -> DoctorReport {
    let mut report = DoctorReport::default();
    collect_config_checks(ctx, &mut report);
    collect_workspace_checks(ctx, run, &mut report);
    collect_daemon_checks(ctx, &mut report);
    collect_freshness_checks(ctx, &mut report, now_epoch_seconds);
    collect_environment_checks(run, &mut report);
    collect_cli_tool_checks(run, &mut report);
    report
}

pub fn run_command(cmd: &str, args: &[&str]) -> anyhow::Result<String> {
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

fn collect_config_checks(ctx: &DoctorContext, report: &mut DoctorReport) {
    let config_path = &ctx.config_path;
    if config_path.exists() {
        report.ok("config", format!("config file: {}", config_path.display()));
    } else {
        report.error_with_hint(
            "config",
            format!("config file not found: {}", config_path.display()),
            "Run `agentzero onboard` or pass `--config <path>`.",
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
                Ok(None) => report.warn_with_hint(
                    "config",
                    "OPENAI_API_KEY is not set (may rely on environment-specific defaults)",
                    "Set OPENAI_API_KEY in environment or .env for provider-backed commands.",
                ),
                Err(err) => report.warn("config", format!("failed to load OPENAI_API_KEY: {err}")),
            }

            if config.provider.model.trim().is_empty() {
                report.error_with_hint(
                    "config",
                    "provider.model is empty",
                    "Set `[provider].model` in config or rerun `agentzero onboard`.",
                );
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
        Err(err) => report.error_with_hint(
            "config",
            format!("config load failed: {err}"),
            "Fix the config file syntax/values and rerun `agentzero doctor`.",
        ),
    }
}

fn collect_workspace_checks(
    ctx: &DoctorContext,
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
        Err(err) => report.error_with_hint(
            "workspace",
            format!("directory is not writable: {err}"),
            "Use a writable workspace or fix directory permissions.",
        ),
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

fn collect_daemon_checks(ctx: &DoctorContext, report: &mut DoctorReport) {
    let daemon_state = ctx.data_dir.join("daemon_state.json");
    if daemon_state.exists() {
        report.ok(
            "daemon",
            format!("state file present: {}", daemon_state.display()),
        );
    } else {
        report.error_with_hint(
            "daemon",
            format!(
                "state file not found: {} (is the daemon running?)",
                daemon_state.display()
            ),
            "Run `agentzero daemon start` (or `agentzero service start`) to initialize runtime state.",
        );
    }
}

fn collect_freshness_checks(
    ctx: &DoctorContext,
    report: &mut DoctorReport,
    now_epoch_seconds: u64,
) {
    let store = HeartbeatStore::new(&ctx.data_dir);
    let components = [
        ("daemon", 120_u64),
        ("channels", 300_u64),
        ("scheduler", 300_u64),
    ];

    for (component, stale_after_seconds) in components {
        let last_seen = store
            .get(component)
            .ok()
            .flatten()
            .map(|record| record.last_seen_epoch_seconds);
        let assessment =
            assess_freshness(component, last_seen, stale_after_seconds, now_epoch_seconds);
        match assessment.severity {
            HealthSeverity::Ok => report.ok("freshness", assessment.message),
            HealthSeverity::Warn => {
                if let Some(hint) = assessment.hint {
                    report.warn_with_hint("freshness", assessment.message, hint);
                } else {
                    report.warn("freshness", assessment.message);
                }
            }
            HealthSeverity::Error => {
                if let Some(hint) = assessment.hint {
                    report.error_with_hint("freshness", assessment.message, hint);
                } else {
                    report.error("freshness", assessment.message);
                }
            }
        }
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

fn truncate_line(value: &str, max_len: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_len {
        return trimmed.to_string();
    }
    let mut out = trimmed.chars().take(max_len).collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::{collect_report, DoctorContext, Severity};
    use agentzero_heartbeat::HeartbeatStore;
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
        let dir = std::env::temp_dir().join(format!("agentzero-doctor-crate-{nanos}-{seq}"));
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
    fn report_marks_stale_heartbeat_as_error_negative_path() {
        let workspace = temp_dir();
        fs::write(workspace.join("AGENTS.md"), "rules").expect("should write AGENTS.md");
        fs::create_dir_all(workspace.join("specs")).expect("should create specs");
        fs::write(workspace.join("specs/SPRINT.md"), "sprint").expect("should write sprint");
        fs::write(workspace.join("agentzero.toml"), "[provider]\nkind=\"openai\"\nbase_url=\"https://api.openai.com\"\nmodel=\"gpt-4o-mini\"\n\n[memory]\nbackend=\"sqlite\"\nsqlite_path=\"./agentzero.db\"\n").expect("should write config");

        let heartbeats = HeartbeatStore::new(&workspace);
        heartbeats.touch("daemon", 100).expect("touch should work");

        let ctx = DoctorContext {
            workspace_root: workspace.clone(),
            data_dir: workspace.clone(),
            config_path: workspace.join("agentzero.toml"),
        };

        let report = collect_report(&ctx, &fake_run_command, 1000);
        assert!(report.checks.iter().any(|check| {
            check.section == "freshness"
                && check.severity == Severity::Error
                && check.message.contains("daemon heartbeat is stale")
        }));

        fs::remove_dir_all(workspace).expect("temp dir should be removed");
    }

    #[test]
    fn report_marks_recent_heartbeat_as_ok_success_path() {
        let workspace = temp_dir();
        fs::write(workspace.join("AGENTS.md"), "rules").expect("should write AGENTS.md");
        fs::create_dir_all(workspace.join("specs")).expect("should create specs");
        fs::write(workspace.join("specs/SPRINT.md"), "sprint").expect("should write sprint");
        fs::write(workspace.join("agentzero.toml"), "[provider]\nkind=\"openai\"\nbase_url=\"https://api.openai.com\"\nmodel=\"gpt-4o-mini\"\n\n[memory]\nbackend=\"sqlite\"\nsqlite_path=\"./agentzero.db\"\n").expect("should write config");
        fs::write(workspace.join("daemon_state.json"), "{}").expect("should write daemon state");

        let heartbeats = HeartbeatStore::new(&workspace);
        heartbeats.touch("daemon", 995).expect("touch should work");

        let ctx = DoctorContext {
            workspace_root: workspace.clone(),
            data_dir: workspace.clone(),
            config_path: workspace.join("agentzero.toml"),
        };

        let report = collect_report(&ctx, &fake_run_command, 1000);
        assert!(report.checks.iter().any(|check| {
            check.section == "freshness"
                && check.severity == Severity::Ok
                && check.message.contains("daemon heartbeat is fresh")
        }));

        fs::remove_dir_all(workspace).expect("temp dir should be removed");
    }
}
