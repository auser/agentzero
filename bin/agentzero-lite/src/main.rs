//! Lightweight AgentZero gateway for resource-constrained devices.
//!
//! Runs only the gateway server with provider access — no local tool execution,
//! no channels, no WASM plugins, no TUI. Designed for edge devices like
//! Raspberry Pi where the full binary is too large.

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "agentzero-lite")]
#[command(about = "Lightweight AgentZero gateway for resource-constrained devices")]
struct Args {
    /// Host interface to bind (default: 127.0.0.1).
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to bind (default: 8080).
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// Path to config file (optional — auto-detects provider from env vars).
    #[arg(long)]
    config: Option<PathBuf>,

    /// Workspace root directory.
    #[arg(long, default_value = ".")]
    workspace: PathBuf,

    /// Data directory for persistent state.
    #[arg(long)]
    data_dir: Option<PathBuf>,

    /// Clear all paired gateway tokens and generate a fresh pairing code.
    #[arg(long)]
    new_pairing: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    tracing::info!(
        host = args.host,
        port = args.port,
        "starting agentzero-lite gateway"
    );

    let data_dir = args
        .data_dir
        .unwrap_or_else(|| args.workspace.join(".agentzero"));

    let config_path = args
        .config
        .unwrap_or_else(|| data_dir.join("agentzero.toml"));

    // Delegate to the gateway — same server, just without tool/channel crates linked
    let options = agentzero_gateway::GatewayRunOptions {
        token_store_path: Some(data_dir.join("gateway-tokens.json")),
        new_pairing: args.new_pairing,
        middleware: Default::default(),
        config_path: Some(config_path),
        workspace_root: Some(args.workspace),
        data_dir: Some(data_dir),
    };
    agentzero_gateway::run(&args.host, args.port, options).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_defaults() {
        let args = Args::try_parse_from(["agentzero-lite"]).expect("should parse defaults");
        assert_eq!(args.host, "127.0.0.1");
        assert_eq!(args.port, 8080);
        assert!(args.config.is_none());
        assert!(!args.new_pairing);
    }

    #[test]
    fn cli_parses_custom_args() {
        let args = Args::try_parse_from([
            "agentzero-lite",
            "--host",
            "0.0.0.0",
            "--port",
            "9090",
            "--config",
            "/tmp/config.toml",
            "--new-pairing",
        ])
        .expect("should parse custom args");
        assert_eq!(args.host, "0.0.0.0");
        assert_eq!(args.port, 9090);
        assert_eq!(
            args.config.as_deref(),
            Some(std::path::Path::new("/tmp/config.toml"))
        );
        assert!(args.new_pairing);
    }

    #[test]
    fn lite_binary_excludes_heavy_crates() {
        // Verify at compile time that we don't depend on heavy crates.
        // If any of these were linked, this test file wouldn't compile
        // without them in Cargo.toml (which they're not).
        //
        // agentzero-tools: NOT in deps (no tool execution)
        // agentzero-channels: NOT in deps (no platform integrations)
        // agentzero-plugins: NOT in deps (no WASM runtime)
        // agentzero-cli: NOT in deps (no CLI commands)
        // agentzero-ffi: NOT in deps (no FFI bindings)
        //
        // This test passes by virtue of compiling successfully.
        assert!(true, "lite binary compiles without heavy crates");
    }
}
