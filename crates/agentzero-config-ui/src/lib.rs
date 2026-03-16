//! Visual node graph configuration editor for AgentZero.
//!
//! Provides a browser-based UI (React Flow + Axum) for visually composing
//! tools, security policies, agents, model routing, and generating TOML config.

pub mod agents_api;
pub mod api;
pub mod schema;
pub mod server;
pub mod toml_bridge;

use agentzero_orchestrator::agent_store::AgentStore;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Start the config UI server and optionally open the browser.
///
/// # Arguments
/// * `config_path` — Optional path to an existing `agentzero.toml` to pre-load.
/// * `port` — Port to bind the server on.
/// * `open_browser` — Whether to open the default browser automatically.
pub async fn start_config_ui(
    _config_path: Option<PathBuf>,
    port: u16,
    open_browser: bool,
) -> anyhow::Result<()> {
    start_config_ui_with_data_dir(_config_path, port, open_browser, None).await
}

/// Start the config UI server with an optional data directory for persistent
/// agent management.
pub async fn start_config_ui_with_data_dir(
    _config_path: Option<PathBuf>,
    port: u16,
    open_browser: bool,
    data_dir: Option<&Path>,
) -> anyhow::Result<()> {
    let agent_store = match data_dir {
        Some(dir) => Some(Arc::new(AgentStore::persistent(dir)?)),
        None => None,
    };
    let router = server::build_router_with_agents(agent_store);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let url = format!("http://127.0.0.1:{port}");

    println!();
    println!("  ╭─────────────────────────────────────────╮");
    println!("  │  AgentZero Config UI                     │");
    println!("  │                                          │");
    println!("  │  → {:<37}│", url);
    println!("  │                                          │");
    println!("  │  Press Ctrl+C to stop                    │");
    println!("  ╰─────────────────────────────────────────╯");
    println!();

    if open_browser {
        // Best-effort browser open — don't fail if it doesn't work
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&url).spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("cmd")
                .args(["/c", "start", &url])
                .spawn();
        }
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
