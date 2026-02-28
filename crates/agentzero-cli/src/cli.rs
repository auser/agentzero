use clap::{ArgAction, ColorChoice, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "agentzero")]
#[command(about = "Learning-focused lightweight clone")]
#[command(color = ColorChoice::Always)]
pub struct Cli {
    /// Directory for config and persisted state (overrides AGENTZERO_DATA_DIR and config file setting).
    #[arg(long, visible_alias = "config-dir", global = true)]
    pub data_dir: Option<PathBuf>,

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
        /// Clear all paired gateway tokens and generate a fresh pairing code.
        #[arg(long)]
        new_pairing: bool,
    },
    /// Send a single message through the agent loop.
    Agent {
        /// User message text to send.
        #[arg(short, long)]
        message: String,
    },
    /// Manage provider subscription authentication profiles.
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },
    /// List supported AI providers.
    Providers {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
        /// Disable ANSI color in table output.
        #[arg(long)]
        no_color: bool,
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

#[derive(Debug, Subcommand)]
pub enum AuthCommands {
    /// Login with OAuth (OpenAI Codex or Gemini).
    Login {
        /// Provider (`openai-codex` or `gemini`).
        #[arg(long)]
        provider: String,
        /// Profile name (default: default).
        #[arg(long, default_value = "default")]
        profile: String,
        /// Use OAuth device-code flow (planned).
        #[arg(long)]
        device_code: bool,
    },
    /// Complete OAuth by pasting redirect URL or auth code.
    PasteRedirect {
        /// Provider (`openai-codex` or `gemini`).
        #[arg(long)]
        provider: String,
        /// Profile name (default: default).
        #[arg(long, default_value = "default")]
        profile: String,
        /// Full redirect URL or raw OAuth code.
        #[arg(long)]
        input: Option<String>,
    },
    /// Paste setup token / auth token (for Anthropic subscription auth).
    PasteToken {
        /// Profile name.
        #[arg(long, default_value = "default")]
        profile: String,
        /// Provider id this token belongs to.
        #[arg(long)]
        provider: String,
        /// Token value.
        #[arg(long)]
        token: String,
        /// Set profile as active after saving.
        #[arg(long, default_value_t = true)]
        activate: bool,
    },
    /// Alias for `paste-token` (interactive by default).
    SetupToken {
        /// Profile name.
        #[arg(long, default_value = "default")]
        profile: String,
        /// Provider id this token belongs to.
        #[arg(long)]
        provider: String,
        /// Token value. If omitted, reads from stdin prompt.
        #[arg(long)]
        token: Option<String>,
        /// Set profile as active after saving.
        #[arg(long, default_value_t = true)]
        activate: bool,
    },
    /// Refresh OpenAI Codex access token using refresh token.
    Refresh {
        /// Provider (`openai-codex` or `gemini`).
        #[arg(long)]
        provider: String,
        /// Profile name or profile id.
        #[arg(long)]
        profile: Option<String>,
    },
    /// Remove auth profile.
    Logout {
        /// Provider id (`openai-codex`, `gemini`, `anthropic`, etc.).
        #[arg(long)]
        provider: String,
        /// Profile name (default: default).
        #[arg(long)]
        profile: Option<String>,
    },
    /// Set active profile for a provider.
    Use {
        /// Profile name to activate.
        profile: String,
    },
    /// List auth profiles.
    List {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Show auth status with active profile and token expiry info.
    Status {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
}
