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
    text::Line,
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
        let mut recent_memory_items = current_memory_count(ctx).await?;

        loop {
            let snapshot = DashboardSnapshot {
                config_path: ctx.config_path.display().to_string(),
                data_dir: ctx.data_dir.display().to_string(),
                recent_memory_items,
                last_refresh_age_secs: last_refresh.elapsed().as_secs(),
            };
            terminal
                .draw(|frame| draw_dashboard(frame, &snapshot))
                .context("failed to render dashboard")?;

            if event::poll(Duration::from_millis(200)).context("failed to poll terminal events")? {
                if let Event::Key(key) = event::read().context("failed to read terminal event")? {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('r') => {
                            recent_memory_items = current_memory_count(ctx).await?;
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

async fn current_memory_count(ctx: &CommandContext) -> anyhow::Result<usize> {
    let memory = build_memory_store(ctx).await?;
    let items = memory.recent(20).await?;
    Ok(items.len())
}

fn draw_dashboard(frame: &mut ratatui::Frame<'_>, snapshot: &DashboardSnapshot) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let header = Paragraph::new(Line::from("AgentZero Dashboard"))
        .block(Block::default().title("Overview").borders(Borders::ALL));
    frame.render_widget(header, chunks[0]);

    let body = Paragraph::new(vec![
        Line::from(format!("Config: {}", snapshot.config_path)),
        Line::from(format!("Data Dir: {}", snapshot.data_dir)),
        Line::from(format!(
            "Recent Memory Items: {}",
            snapshot.recent_memory_items
        )),
        Line::from(format!(
            "Last Refresh Age: {}s",
            snapshot.last_refresh_age_secs
        )),
    ])
    .block(Block::default().title("Runtime").borders(Borders::ALL));
    frame.render_widget(body, chunks[1]);

    let footer = Paragraph::new(Line::from("Controls: [r] refresh  [q] quit"))
        .block(Block::default().title("Controls").borders(Borders::ALL));
    frame.render_widget(footer, chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::{DashboardCommand, DashboardSnapshot};
    use crate::command_core::{AgentZeroCommand, CommandContext};
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
        let dir = std::env::temp_dir().join(format!("agentzero-cli-dashboard-{nanos}-{seq}"));
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
        };
        assert_eq!(snapshot.recent_memory_items, 3);
        assert!(snapshot.config_path.contains("config.toml"));
        assert!(snapshot.data_dir.contains("/tmp/data"));
    }

    #[tokio::test]
    async fn dashboard_command_requires_tty_negative_path() {
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
}
