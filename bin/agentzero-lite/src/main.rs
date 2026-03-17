use std::process::ExitCode;

use clap::Parser;
use tracing_subscriber::EnvFilter;

/// Lightweight AgentZero gateway for resource-constrained devices.
///
/// Starts the HTTP/WebSocket gateway server with minimal dependencies:
/// no tool execution, no channel integrations, no TUI, no WASM plugins.
#[derive(Parser)]
#[command(name = "agentzero-lite")]
#[command(about = "Lightweight AgentZero gateway for resource-constrained devices")]
struct Args {
    /// Host to bind
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to bind
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// Config file path (agentzero.toml)
    #[arg(long)]
    config: Option<std::path::PathBuf>,

    /// Workspace root directory
    #[arg(long)]
    workspace: Option<std::path::PathBuf>,

    /// Data directory for persistent state
    #[arg(long)]
    data_dir: Option<std::path::PathBuf>,

    /// Generate a new pairing code (clears existing paired tokens)
    #[arg(long)]
    new_pairing: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    // Install the redacting panic hook so secrets are never leaked in backtraces.
    agentzero_core::security::redaction::install_redacting_panic_hook();
    let _ = agentzero_core::security::policy::baseline_version();

    // Initialize tracing from RUST_LOG / AGENTZERO_LOG env vars.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("AGENTZERO_LOG")
                .or_else(|_| EnvFilter::try_from_env("RUST_LOG"))
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    let options = agentzero_gateway::GatewayRunOptions {
        token_store_path: args
            .data_dir
            .as_ref()
            .map(|d| d.join("gateway_tokens.json")),
        new_pairing: args.new_pairing,
        config_path: args.config,
        workspace_root: args.workspace,
        data_dir: args.data_dir,
        ..Default::default()
    };

    match agentzero_gateway::run(&args.host, args.port, options).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "error: {}",
                agentzero_core::security::redaction::redact_error_chain(err.as_ref())
            );
            ExitCode::from(1)
        }
    }
}
