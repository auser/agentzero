//! Enhanced TUI dashboard that polls the gateway API for live data.
//!
//! Provides a tab-based interface with Overview, Runs, Agents, and Events views.
//! Uses ratatui + crossterm for rendering and tokio for async HTTP polling.

use anyhow::Context;
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
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs},
    Terminal,
};
use serde::Deserialize;
use std::io::{self, IsTerminal};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Tab enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardTab {
    Overview,
    Runs,
    Agents,
    Events,
}

impl DashboardTab {
    pub const ALL: [DashboardTab; 4] = [
        DashboardTab::Overview,
        DashboardTab::Runs,
        DashboardTab::Agents,
        DashboardTab::Events,
    ];

    pub fn index(self) -> usize {
        match self {
            Self::Overview => 0,
            Self::Runs => 1,
            Self::Agents => 2,
            Self::Events => 3,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Runs => "Runs",
            Self::Agents => "Agents",
            Self::Events => "Events",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Overview => Self::Runs,
            Self::Runs => Self::Agents,
            Self::Agents => Self::Events,
            Self::Events => Self::Overview,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Overview => Self::Events,
            Self::Runs => Self::Overview,
            Self::Agents => Self::Runs,
            Self::Events => Self::Agents,
        }
    }
}

// ---------------------------------------------------------------------------
// API response types (mirror gateway models, but Deserialize)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HealthResponse {
    pub status: String,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)] // Fields used for JSON deserialization
pub struct RunListItem {
    pub run_id: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub tokens_used: u64,
    #[serde(default)]
    pub cost_microdollars: u64,
    #[serde(default)]
    pub created_at_epoch_ms: u64,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AgentListItem {
    pub agent_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub provider: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct EventItem {
    #[serde(rename = "type", default)]
    pub event_type: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Wrapper for list-style API responses.
#[derive(Debug, Clone, Deserialize)]
struct ListResponse<T> {
    #[serde(default)]
    data: Vec<T>,
}

/// Wrapper for event list responses (events field rather than data).
#[derive(Debug, Clone, Deserialize)]
struct EventListResponse {
    #[serde(default)]
    events: Vec<EventItem>,
}

// ---------------------------------------------------------------------------
// Dashboard state
// ---------------------------------------------------------------------------

pub struct DashboardState {
    pub gateway_url: String,
    pub client: reqwest::Client,
    pub health: Option<HealthResponse>,
    pub runs: Vec<RunListItem>,
    pub agents: Vec<AgentListItem>,
    pub events: Vec<EventItem>,
    pub active_tab: DashboardTab,
    pub last_error: Option<String>,
    pub scroll_offset: usize,
}

impl DashboardState {
    pub fn new(gateway_url: String) -> Self {
        Self {
            gateway_url,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("reqwest client should build with default TLS"),
            health: None,
            runs: Vec::new(),
            agents: Vec::new(),
            events: Vec::new(),
            active_tab: DashboardTab::Overview,
            last_error: None,
            scroll_offset: 0,
        }
    }

    pub async fn refresh(&mut self) {
        self.last_error = None;

        // Health
        match self
            .client
            .get(format!("{}/health", self.gateway_url))
            .send()
            .await
        {
            Ok(resp) => match resp.json::<HealthResponse>().await {
                Ok(h) => self.health = Some(h),
                Err(e) => self.last_error = Some(format!("health parse: {e}")),
            },
            Err(e) => {
                self.health = None;
                self.last_error = Some(format!("health fetch: {e}"));
            }
        }

        // Runs
        match self
            .client
            .get(format!("{}/v1/runs", self.gateway_url))
            .send()
            .await
        {
            Ok(resp) => match resp.json::<ListResponse<RunListItem>>().await {
                Ok(list) => self.runs = list.data,
                Err(e) => {
                    if self.last_error.is_none() {
                        self.last_error = Some(format!("runs parse: {e}"));
                    }
                }
            },
            Err(e) => {
                if self.last_error.is_none() {
                    self.last_error = Some(format!("runs fetch: {e}"));
                }
            }
        }

        // Agents
        match self
            .client
            .get(format!("{}/v1/agents", self.gateway_url))
            .send()
            .await
        {
            Ok(resp) => match resp.json::<ListResponse<AgentListItem>>().await {
                Ok(list) => self.agents = list.data,
                Err(e) => {
                    if self.last_error.is_none() {
                        self.last_error = Some(format!("agents parse: {e}"));
                    }
                }
            },
            Err(e) => {
                if self.last_error.is_none() {
                    self.last_error = Some(format!("agents fetch: {e}"));
                }
            }
        }

        // Events — poll from the first run that has events, or collect from all runs.
        // The gateway exposes per-run events at /v1/runs/:run_id/events.
        // We aggregate from recent runs.
        let mut all_events: Vec<EventItem> = Vec::new();
        for run in self.runs.iter().take(10) {
            match self
                .client
                .get(format!(
                    "{}/v1/runs/{}/events",
                    self.gateway_url, run.run_id
                ))
                .send()
                .await
            {
                Ok(resp) => {
                    if let Ok(ev_resp) = resp.json::<EventListResponse>().await {
                        all_events.extend(ev_resp.events);
                    }
                }
                Err(_) => { /* skip silently for event aggregation */ }
            }
        }
        self.events = all_events;
    }
}

// ---------------------------------------------------------------------------
// Color mapping for events
// ---------------------------------------------------------------------------

pub fn event_topic_color(event_type: &str) -> Color {
    if event_type.starts_with("tool") {
        Color::Blue
    } else if event_type.starts_with("job") {
        Color::Green
    } else if event_type.starts_with("error") || event_type.starts_with("fail") {
        Color::Red
    } else if event_type.starts_with("agent") {
        Color::Yellow
    } else if event_type.starts_with("presence") {
        Color::Magenta
    } else {
        Color::Gray
    }
}

/// Map a run status string to a display color.
fn status_color(status: &str) -> Color {
    match status {
        "completed" => Color::Green,
        "running" => Color::Yellow,
        "failed" => Color::Red,
        "cancelled" => Color::DarkGray,
        "pending" => Color::Cyan,
        _ => Color::White,
    }
}

// ---------------------------------------------------------------------------
// Health display formatting
// ---------------------------------------------------------------------------

pub fn format_health_display(health: &Option<HealthResponse>) -> String {
    match health {
        Some(h) => format!(
            "status={} service={} version={}",
            h.status, h.service, h.version
        ),
        None => "unavailable".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn draw_tui(frame: &mut ratatui::Frame<'_>, state: &DashboardState) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // tab bar
            Constraint::Min(8),    // main content
            Constraint::Length(3), // footer / key hints
        ])
        .split(frame.area());

    // -- Tab bar --
    let titles: Vec<Line<'_>> = DashboardTab::ALL
        .iter()
        .map(|t| Line::from(t.title()))
        .collect();
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .title(" AgentZero Dashboard ")
                .borders(Borders::ALL),
        )
        .select(state.active_tab.index())
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, outer[0]);

    // -- Main content area --
    match state.active_tab {
        DashboardTab::Overview => draw_overview(frame, state, outer[1]),
        DashboardTab::Runs => draw_runs(frame, state, outer[1]),
        DashboardTab::Agents => draw_agents(frame, state, outer[1]),
        DashboardTab::Events => draw_events(frame, state, outer[1]),
    }

    // -- Footer --
    let error_hint = state
        .last_error
        .as_deref()
        .map(|e| format!("  [!] {e}"))
        .unwrap_or_default();
    let footer_text = format!(
        "  [Tab/Shift-Tab] switch tab  [r] refresh  [q] quit{}",
        error_hint
    );
    let footer = Paragraph::new(Line::from(footer_text))
        .block(Block::default().title("Controls").borders(Borders::ALL));
    frame.render_widget(footer, outer[2]);
}

fn draw_overview(
    frame: &mut ratatui::Frame<'_>,
    state: &DashboardState,
    area: ratatui::layout::Rect,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // health
            Constraint::Min(4),    // counts
        ])
        .split(area);

    // Health section
    let health_text = format_health_display(&state.health);
    let health_color = if state.health.as_ref().is_some_and(|h| h.status == "ok") {
        Color::Green
    } else {
        Color::Red
    };
    let health_lines = vec![
        Line::from(vec![
            Span::raw("  Health: "),
            Span::styled(health_text, Style::default().fg(health_color)),
        ]),
        Line::from(format!("  Active agents: {}", state.agents.len())),
    ];
    let health_panel =
        Paragraph::new(health_lines).block(Block::default().title("Health").borders(Borders::ALL));
    frame.render_widget(health_panel, chunks[0]);

    // Run counts
    let running = state.runs.iter().filter(|r| r.status == "running").count();
    let completed = state
        .runs
        .iter()
        .filter(|r| r.status == "completed")
        .count();
    let failed = state.runs.iter().filter(|r| r.status == "failed").count();
    let pending = state.runs.iter().filter(|r| r.status == "pending").count();
    let cancelled = state
        .runs
        .iter()
        .filter(|r| r.status == "cancelled")
        .count();

    let run_lines = vec![
        Line::from(vec![
            Span::raw("  Running: "),
            Span::styled(running.to_string(), Style::default().fg(Color::Yellow)),
            Span::raw("  Completed: "),
            Span::styled(completed.to_string(), Style::default().fg(Color::Green)),
            Span::raw("  Failed: "),
            Span::styled(failed.to_string(), Style::default().fg(Color::Red)),
        ]),
        Line::from(format!(
            "  Pending: {}  Cancelled: {}  Total: {}",
            pending,
            cancelled,
            state.runs.len()
        )),
        Line::from(format!("  Recent events: {}", state.events.len())),
    ];
    let runs_panel = Paragraph::new(run_lines)
        .block(Block::default().title("Run Summary").borders(Borders::ALL));
    frame.render_widget(runs_panel, chunks[1]);
}

fn draw_runs(frame: &mut ratatui::Frame<'_>, state: &DashboardState, area: ratatui::layout::Rect) {
    let header_cells = ["ID", "Agent", "Status", "Tokens", "Cost ($)"]
        .iter()
        .map(|h| {
            Cell::from(*h).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        });
    let header = Row::new(header_cells).height(1);

    let rows: Vec<Row<'_>> = state
        .runs
        .iter()
        .map(|r| {
            let cost_dollars = r.cost_microdollars as f64 / 1_000_000.0;
            let id_short = if r.run_id.len() > 12 {
                &r.run_id[..12]
            } else {
                &r.run_id
            };
            Row::new(vec![
                Cell::from(id_short.to_string()),
                Cell::from(r.agent_id.clone()),
                Cell::from(r.status.clone()).style(Style::default().fg(status_color(&r.status))),
                Cell::from(r.tokens_used.to_string()),
                Cell::from(format!("{cost_dollars:.4}")),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(20),
            Constraint::Percentage(25),
            Constraint::Percentage(15),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(format!("Runs ({})", state.runs.len()))
            .borders(Borders::ALL),
    );

    frame.render_widget(table, area);
}

fn draw_agents(
    frame: &mut ratatui::Frame<'_>,
    state: &DashboardState,
    area: ratatui::layout::Rect,
) {
    let header_cells = ["Agent ID", "Name", "Status", "Provider", "Model"]
        .iter()
        .map(|h| {
            Cell::from(*h).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        });
    let header = Row::new(header_cells).height(1);

    let rows: Vec<Row<'_>> = state
        .agents
        .iter()
        .map(|a| {
            // Count active runs for this agent.
            let active = state
                .runs
                .iter()
                .filter(|r| r.agent_id == a.agent_id && r.status == "running")
                .count();
            let name_display = if active > 0 {
                format!("{} ({active} active)", a.name)
            } else {
                a.name.clone()
            };
            let status_clr = if a.status == "active" {
                Color::Green
            } else {
                Color::DarkGray
            };
            Row::new(vec![
                Cell::from(a.agent_id.clone()),
                Cell::from(name_display),
                Cell::from(a.status.clone()).style(Style::default().fg(status_clr)),
                Cell::from(a.provider.clone()),
                Cell::from(a.model.clone()),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(20),
            Constraint::Percentage(25),
            Constraint::Percentage(15),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(format!("Agents ({})", state.agents.len()))
            .borders(Borders::ALL),
    );

    frame.render_widget(table, area);
}

fn draw_events(
    frame: &mut ratatui::Frame<'_>,
    state: &DashboardState,
    area: ratatui::layout::Rect,
) {
    let visible_height = area.height.saturating_sub(2) as usize; // borders
    let total = state.events.len();
    let offset = state
        .scroll_offset
        .min(total.saturating_sub(visible_height));

    let lines: Vec<Line<'_>> = state
        .events
        .iter()
        .skip(offset)
        .take(visible_height)
        .map(|ev| {
            let color = event_topic_color(&ev.event_type);
            let tool_str = ev.tool.as_deref().unwrap_or("-");
            let detail = ev.result.as_deref().or(ev.error.as_deref()).unwrap_or("");
            let detail_truncated = if detail.len() > 60 {
                &detail[..60]
            } else {
                detail
            };
            Line::from(vec![
                Span::styled(
                    format!(" [{:>12}] ", ev.event_type),
                    Style::default().fg(color),
                ),
                Span::raw(format!("run={} ", &ev.run_id)),
                Span::styled(
                    format!("tool={tool_str} "),
                    Style::default().fg(Color::White),
                ),
                Span::raw(detail_truncated.to_string()),
            ])
        })
        .collect();

    let title = if total > visible_height {
        format!(
            "Events ({total}) [{}-{}]",
            offset + 1,
            (offset + visible_height).min(total)
        )
    } else {
        format!("Events ({total})")
    };

    let events_panel =
        Paragraph::new(lines).block(Block::default().title(title).borders(Borders::ALL));
    frame.render_widget(events_panel, area);
}

// ---------------------------------------------------------------------------
// Terminal cleanup guard
// ---------------------------------------------------------------------------

struct TerminalCleanupGuard;

impl Drop for TerminalCleanupGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub async fn run_dashboard_tui(host: &str, port: u16) -> anyhow::Result<()> {
    if !io::stdout().is_terminal() {
        anyhow::bail!("dashboard requires a TTY terminal");
    }

    let gateway_url = format!("http://{host}:{port}");
    let mut state = DashboardState::new(gateway_url);

    // Initial fetch
    state.refresh().await;

    enable_raw_mode().context("failed to enable terminal raw mode")?;
    execute!(io::stdout(), EnterAlternateScreen).context("failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("failed to initialize terminal")?;
    let _guard = TerminalCleanupGuard;

    let mut last_poll = Instant::now();
    let poll_interval = Duration::from_secs(3);

    loop {
        // Auto-refresh
        if last_poll.elapsed() >= poll_interval {
            state.refresh().await;
            last_poll = Instant::now();
        }

        terminal
            .draw(|frame| draw_tui(frame, &state))
            .context("failed to render dashboard")?;

        // Poll for input with a short timeout so we can auto-refresh
        if event::poll(Duration::from_millis(200)).context("failed to poll terminal events")? {
            if let Event::Key(key) = event::read().context("failed to read terminal event")? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('r') => {
                        state.refresh().await;
                        last_poll = Instant::now();
                    }
                    KeyCode::Tab => {
                        state.active_tab = state.active_tab.next();
                        state.scroll_offset = 0;
                    }
                    KeyCode::BackTab => {
                        state.active_tab = state.active_tab.prev();
                        state.scroll_offset = 0;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if state.active_tab == DashboardTab::Events {
                            state.scroll_offset = state.scroll_offset.saturating_add(1);
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if state.active_tab == DashboardTab::Events {
                            state.scroll_offset = state.scroll_offset.saturating_sub(1);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_cycles_forward_and_back() {
        let tab = DashboardTab::Overview;
        assert_eq!(tab.next(), DashboardTab::Runs);
        assert_eq!(tab.next().next(), DashboardTab::Agents);
        assert_eq!(tab.next().next().next(), DashboardTab::Events);
        // Full cycle back to Overview
        assert_eq!(tab.next().next().next().next(), DashboardTab::Overview);

        // Reverse cycle
        assert_eq!(DashboardTab::Overview.prev(), DashboardTab::Events);
        assert_eq!(DashboardTab::Events.prev(), DashboardTab::Agents);
        assert_eq!(DashboardTab::Agents.prev(), DashboardTab::Runs);
        assert_eq!(DashboardTab::Runs.prev(), DashboardTab::Overview);
    }

    #[test]
    fn dashboard_state_creates_with_defaults() {
        let state = DashboardState::new("http://localhost:8080".to_string());
        assert_eq!(state.gateway_url, "http://localhost:8080");
        assert!(state.health.is_none());
        assert!(state.runs.is_empty());
        assert!(state.agents.is_empty());
        assert!(state.events.is_empty());
        assert_eq!(state.active_tab, DashboardTab::Overview);
        assert!(state.last_error.is_none());
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn event_color_mapping_returns_expected_colors() {
        assert_eq!(event_topic_color("tool.exec"), Color::Blue);
        assert_eq!(event_topic_color("tool_call"), Color::Blue);
        assert_eq!(event_topic_color("job.started"), Color::Green);
        assert_eq!(event_topic_color("job.completed"), Color::Green);
        assert_eq!(event_topic_color("error.timeout"), Color::Red);
        assert_eq!(event_topic_color("failed.run"), Color::Red);
        assert_eq!(event_topic_color("agent.spawn"), Color::Yellow);
        assert_eq!(event_topic_color("presence.heartbeat"), Color::Magenta);
        assert_eq!(event_topic_color("unknown_type"), Color::Gray);
    }

    #[test]
    fn health_display_formatting() {
        // None case
        assert_eq!(format_health_display(&None), "unavailable");

        // Some case
        let health = Some(HealthResponse {
            status: "ok".to_string(),
            service: "agentzero".to_string(),
            version: "0.1.0".to_string(),
        });
        let display = format_health_display(&health);
        assert!(display.contains("status=ok"));
        assert!(display.contains("service=agentzero"));
        assert!(display.contains("version=0.1.0"));
    }

    #[test]
    fn tab_index_and_title() {
        assert_eq!(DashboardTab::Overview.index(), 0);
        assert_eq!(DashboardTab::Runs.index(), 1);
        assert_eq!(DashboardTab::Agents.index(), 2);
        assert_eq!(DashboardTab::Events.index(), 3);

        assert_eq!(DashboardTab::Overview.title(), "Overview");
        assert_eq!(DashboardTab::Runs.title(), "Runs");
        assert_eq!(DashboardTab::Agents.title(), "Agents");
        assert_eq!(DashboardTab::Events.title(), "Events");
    }

    #[test]
    fn status_color_mapping() {
        assert_eq!(status_color("completed"), Color::Green);
        assert_eq!(status_color("running"), Color::Yellow);
        assert_eq!(status_color("failed"), Color::Red);
        assert_eq!(status_color("cancelled"), Color::DarkGray);
        assert_eq!(status_color("pending"), Color::Cyan);
        assert_eq!(status_color("other"), Color::White);
    }
}
