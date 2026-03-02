use crate::command_core::{AgentZeroCommand, CommandContext};
use crate::commands::memory::build_memory_store;
use anyhow::Context;
use async_trait::async_trait;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::io::{self, IsTerminal};
use std::time::{Duration, Instant};

pub struct DashboardCommand;

#[derive(Debug, Clone)]
struct DashboardSnapshot {
    config_path: String,
    data_dir: String,
    recent_memory_items: usize,
    last_refresh_age_secs: u64,
    // Daemon status
    daemon_running: bool,
    daemon_pid: Option<u32>,
    daemon_uptime_secs: u64,
    daemon_host: Option<String>,
    daemon_port: Option<u16>,
    // Channel catalog
    total_channels: usize,
    // Provider info
    provider_kind: Option<String>,
    provider_model: Option<String>,
}

#[async_trait]
impl AgentZeroCommand for DashboardCommand {
    type Options = ();

    async fn run(ctx: &CommandContext, _opts: Self::Options) -> anyhow::Result<()> {
        if !io::stdout().is_terminal() {
            anyhow::bail!("dashboard requires a TTY terminal");
        }

        enable_raw_mode().context("failed to enable terminal raw mode")?;
        execute!(io::stdout(), EnterAlternateScreen).context("failed to enter alternate screen")?;

        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend).context("failed to initialize dashboard")?;
        let _guard = TerminalCleanupGuard;

        let mut last_refresh = Instant::now();
        let mut snapshot = build_snapshot(ctx).await?;

        loop {
            snapshot.last_refresh_age_secs = last_refresh.elapsed().as_secs();
            terminal
                .draw(|frame| draw_dashboard(frame, &snapshot))
                .context("failed to render dashboard")?;

            if event::poll(Duration::from_millis(200)).context("failed to poll terminal events")? {
                if let Event::Key(key) = event::read().context("failed to read terminal event")? {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('r') => {
                            snapshot = build_snapshot(ctx).await?;
                            last_refresh = Instant::now();
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }
}

struct TerminalCleanupGuard;

impl Drop for TerminalCleanupGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

async fn build_snapshot(ctx: &CommandContext) -> anyhow::Result<DashboardSnapshot> {
    let memory = build_memory_store(ctx).await?;
    let items = memory.recent(20).await?;

    // Daemon status (best-effort).
    let (daemon_running, daemon_pid, daemon_uptime_secs, daemon_host, daemon_port) =
        match agentzero_daemon::DaemonManager::new(&ctx.data_dir) {
            Ok(manager) => match manager.status() {
                Ok(status) => (
                    status.running,
                    status.pid,
                    status.uptime_secs(),
                    status.host.clone(),
                    status.port,
                ),
                Err(_) => (false, None, 0, None, None),
            },
            Err(_) => (false, None, 0, None, None),
        };

    // Channel catalog count.
    let total_channels = agentzero_channels::channel_catalog().len();

    // Provider info from config (best-effort).
    let (provider_kind, provider_model) = load_provider_info(ctx);

    Ok(DashboardSnapshot {
        config_path: ctx.config_path.display().to_string(),
        data_dir: ctx.data_dir.display().to_string(),
        recent_memory_items: items.len(),
        last_refresh_age_secs: 0,
        daemon_running,
        daemon_pid,
        daemon_uptime_secs,
        daemon_host,
        daemon_port,
        total_channels,
        provider_kind,
        provider_model,
    })
}

fn load_provider_info(ctx: &CommandContext) -> (Option<String>, Option<String>) {
    // Try to read the config file for provider info.
    let content = match std::fs::read_to_string(&ctx.config_path) {
        Ok(c) => c,
        Err(_) => return (None, None),
    };
    // Simple TOML key extraction (avoid full config dependency).
    let mut kind = None;
    let mut model = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("kind") {
            if let Some(val) = extract_toml_string_value(trimmed) {
                kind = Some(val);
            }
        }
        if trimmed.starts_with("model") {
            if let Some(val) = extract_toml_string_value(trimmed) {
                model = Some(val);
            }
        }
    }
    (kind, model)
}

fn extract_toml_string_value(line: &str) -> Option<String> {
    let (_, val) = line.split_once('=')?;
    let val = val.trim().trim_matches('"').trim_matches('\'');
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

fn draw_dashboard(frame: &mut ratatui::Frame<'_>, snapshot: &DashboardSnapshot) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Length(5), // daemon status
            Constraint::Length(5), // provider + channels
            Constraint::Min(4),    // memory + config
            Constraint::Length(3), // footer
        ])
        .split(frame.area());

    // Header
    let header = Paragraph::new(Line::from(vec![Span::styled(
        " AgentZero Dashboard ",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(header, chunks[0]);

    // Daemon status panel
    let status_color = if snapshot.daemon_running {
        Color::Green
    } else {
        Color::Red
    };
    let status_text = if snapshot.daemon_running {
        "running"
    } else {
        "stopped"
    };
    let mut daemon_lines = vec![Line::from(vec![
        Span::raw("  Status: "),
        Span::styled(status_text, Style::default().fg(status_color)),
    ])];
    if let Some(pid) = snapshot.daemon_pid {
        daemon_lines.push(Line::from(format!("  PID: {pid}")));
    }
    if snapshot.daemon_running {
        let uptime = format_uptime(snapshot.daemon_uptime_secs);
        let addr = format!(
            "{}:{}",
            snapshot.daemon_host.as_deref().unwrap_or("?"),
            snapshot
                .daemon_port
                .map(|p| p.to_string())
                .unwrap_or_else(|| "?".to_string())
        );
        daemon_lines.push(Line::from(format!("  Address: {addr}  Uptime: {uptime}")));
    }
    let daemon_panel =
        Paragraph::new(daemon_lines).block(Block::default().title("Daemon").borders(Borders::ALL));
    frame.render_widget(daemon_panel, chunks[1]);

    // Provider + channels panel
    let provider_display = match (&snapshot.provider_kind, &snapshot.provider_model) {
        (Some(k), Some(m)) => format!("{k} / {m}"),
        (Some(k), None) => k.clone(),
        _ => "not configured".to_string(),
    };
    let provider_lines = vec![
        Line::from(format!("  Provider: {}", provider_display)),
        Line::from(format!(
            "  Channels: {} registered",
            snapshot.total_channels
        )),
    ];
    let provider_panel = Paragraph::new(provider_lines).block(
        Block::default()
            .title("Provider & Channels")
            .borders(Borders::ALL),
    );
    frame.render_widget(provider_panel, chunks[2]);

    // Runtime info panel
    let body = Paragraph::new(vec![
        Line::from(format!("  Config: {}", snapshot.config_path)),
        Line::from(format!("  Data Dir: {}", snapshot.data_dir)),
        Line::from(format!(
            "  Memory Items: {}  (refresh age: {}s)",
            snapshot.recent_memory_items, snapshot.last_refresh_age_secs
        )),
    ])
    .block(Block::default().title("Runtime").borders(Borders::ALL));
    frame.render_widget(body, chunks[3]);

    // Footer
    let footer = Paragraph::new(Line::from("  [r] refresh  [q] quit"))
        .block(Block::default().title("Controls").borders(Borders::ALL));
    frame.render_widget(footer, chunks[4]);
}

fn format_uptime(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_toml_string_value, format_uptime, DashboardCommand, DashboardSnapshot};
    use crate::command_core::{AgentZeroCommand, CommandContext};
    use std::fs;
    use std::io::IsTerminal;
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
        let dir = std::env::temp_dir().join(format!(
            "agentzero-cli-dashboard-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn snapshot_fields_reflect_render_data_success_path() {
        let snapshot = DashboardSnapshot {
            config_path: "/tmp/config.toml".to_string(),
            data_dir: "/tmp/data".to_string(),
            recent_memory_items: 3,
            last_refresh_age_secs: 0,
            daemon_running: false,
            daemon_pid: None,
            daemon_uptime_secs: 0,
            daemon_host: None,
            daemon_port: None,
            total_channels: 27,
            provider_kind: Some("openai".to_string()),
            provider_model: Some("gpt-4".to_string()),
        };
        assert_eq!(snapshot.recent_memory_items, 3);
        assert!(snapshot.config_path.contains("config.toml"));
        assert!(snapshot.data_dir.contains("/tmp/data"));
        assert_eq!(snapshot.total_channels, 27);
    }

    #[tokio::test]
    async fn dashboard_command_requires_tty_negative_path() {
        // This test validates non-TTY behavior; skip when stdout is a real terminal
        // (e.g. `cargo test` from an interactive shell / pre-commit hook).
        if std::io::stdout().is_terminal() {
            return;
        }
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };
        let err = DashboardCommand::run(&ctx, ())
            .await
            .expect_err("dashboard should fail in non-tty test env");
        assert!(err.to_string().contains("requires a TTY"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn format_uptime_formatting() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(45), "45s");
        assert_eq!(format_uptime(120), "2m 0s");
        assert_eq!(format_uptime(3661), "1h 1m");
    }

    #[test]
    fn extract_toml_value_parses_quoted_strings() {
        assert_eq!(
            extract_toml_string_value(r#"kind = "openai""#),
            Some("openai".to_string())
        );
        assert_eq!(
            extract_toml_string_value(r#"model = "gpt-4""#),
            Some("gpt-4".to_string())
        );
        assert_eq!(extract_toml_string_value(r#"key = """#), None);
    }
}
