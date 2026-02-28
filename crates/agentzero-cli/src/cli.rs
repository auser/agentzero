use clap::{ArgAction, ColorChoice, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "agentzero")]
#[command(about = "Learning-focused lightweight clone")]
#[command(color = ColorChoice::Always)]
pub struct Cli {
    /// Path to config file (overrides AGENTZERO_CONFIG and default lookup).
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    /// Increase verbosity: -v=error, -vv=info, -vvv=debug, -vvvv=trace.
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Create a starter agentzero.toml in the current directory.
    Onboard {
        /// Skip prompts and auto-accept defaults/non-interactive behavior.
        #[arg(long)]
        yes: bool,
        /// Provider name (openai, openrouter, anthropic).
        #[arg(long)]
        provider: Option<String>,
        /// Provider base URL.
        #[arg(long)]
        base_url: Option<String>,
        /// Provider model ID.
        #[arg(long)]
        model: Option<String>,
        /// Memory database path.
        #[arg(long)]
        memory_path: Option<String>,
        /// Allowed root path for scoped filesystem access.
        #[arg(long)]
        allowed_root: Option<String>,
        /// Allowed shell commands (repeat or pass comma-separated values).
        #[arg(long, value_delimiter = ',')]
        allowed_commands: Vec<String>,
    },
    /// Start the HTTP gateway server.
    Gateway {
        /// Host interface to bind (default: 127.0.0.1).
        #[arg(long)]
        host: Option<String>,
        /// Port to bind (default: 8080).
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// Send a single message through the agent loop.
    Agent {
        /// User message text to send.
        #[arg(short, long)]
        message: String,
    },
    /// Show a minimal status summary and recent memory count.
    Status {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Run diagnostics for daemon/scheduler/channel freshness.
    Doctor,
}
