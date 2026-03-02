use clap::{ArgAction, ColorChoice, Parser, Subcommand, ValueEnum};
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

    /// Emit structured JSON object output for any command.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Create a starter agentzero.toml in the current directory.
    Onboard {
        /// Run the full interactive wizard (default is quick setup).
        #[arg(long)]
        interactive: bool,
        /// Overwrite existing config without confirmation.
        #[arg(long)]
        force: bool,
        /// Reconfigure channels only (fast repair flow).
        #[arg(long)]
        channels_only: bool,
        /// API key (used in quick mode, ignored with --interactive).
        #[arg(long)]
        api_key: Option<String>,
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
        /// Memory backend (sqlite, lucid, markdown, none) used in quick mode.
        #[arg(long)]
        memory: Option<String>,
        /// Memory database path.
        #[arg(long)]
        memory_path: Option<String>,
        /// Disable OTP in quick setup (not recommended).
        #[arg(long)]
        no_totp: bool,
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
    /// Manage the background daemon process.
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },
    /// Send a single message through the agent loop.
    Agent {
        /// User message text to send.
        #[arg(short, long)]
        message: String,
        /// Override the provider (e.g. openrouter, openai, ollama).
        #[arg(short, long)]
        provider: Option<String>,
        /// Override the model name.
        #[arg(long)]
        model: Option<String>,
        /// Use a specific auth profile by name (from `auth list`).
        #[arg(long)]
        profile: Option<String>,
    },
    /// Manage provider subscription authentication profiles.
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },
    /// Configure and manage scheduled tasks.
    Cron {
        #[command(subcommand)]
        command: CronCommands,
    },
    /// Manage lifecycle hooks and diagnostics.
    Hooks {
        #[command(subcommand)]
        command: HookCommands,
    },
    /// Manage skills (list/install/test/remove).
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },
    /// Manage secure tunnel lifecycle.
    Tunnel {
        #[command(subcommand)]
        command: TunnelCommands,
    },
    /// Plugin developer lifecycle commands.
    Plugin {
        #[command(subcommand)]
        command: PluginCommands,
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
    /// Engage, inspect, and resume emergency-stop states.
    Estop {
        /// Level used when engaging estop from `agentzero estop`.
        #[arg(long, value_enum)]
        level: Option<EstopLevel>,
        /// Domain pattern(s) for `domain-block` (repeatable).
        #[arg(long = "domain")]
        domains: Vec<String>,
        /// Tool name(s) for `tool-freeze` (repeatable).
        #[arg(long = "tool")]
        tools: Vec<String>,
        /// Require OTP (TOTP) to resume from this emergency stop.
        #[arg(long)]
        require_otp: bool,
        #[command(subcommand)]
        command: Option<EstopCommands>,
    },
    /// Manage channels.
    Channel {
        #[command(subcommand)]
        command: ChannelCommands,
    },
    /// Browse and validate integrations.
    Integrations {
        #[command(subcommand)]
        command: IntegrationsCommands,
    },
    /// Discover and manage local AI model services.
    Local {
        #[command(subcommand)]
        command: LocalCommands,
    },
    /// Manage provider model catalogs.
    Models {
        #[command(subcommand)]
        command: ModelCommands,
    },
    /// Evaluate approval requirements for high-risk actions.
    Approval {
        #[command(subcommand)]
        command: ApprovalCommands,
    },
    /// Manage actor identities and roles.
    Identity {
        #[command(subcommand)]
        command: IdentityCommands,
    },
    /// Inspect and update coordination runtime status.
    Coordination {
        #[command(subcommand)]
        command: CoordinationCommands,
    },
    /// Inspect and update accumulated runtime cost summary.
    Cost {
        #[command(subcommand)]
        command: CostCommands,
    },
    /// Manage runtime goals.
    Goals {
        #[command(subcommand)]
        command: GoalCommands,
    },
    /// Show a minimal status summary and recent memory count.
    Status,
    /// Run diagnostics for daemon/scheduler/channel freshness.
    Doctor {
        #[command(subcommand)]
        command: DoctorCommands,
    },
    /// Manage OS service lifecycle.
    Service {
        /// Init system to use: auto (detect), systemd, or openrc.
        #[arg(long, value_enum, default_value_t = ServiceInit::Auto)]
        service_init: ServiceInit,
        #[command(subcommand)]
        command: ServiceCommands,
    },
    /// Launch interactive terminal dashboard.
    Dashboard,
    /// Migrate data from external runtimes.
    Migrate {
        #[command(subcommand)]
        command: MigrateCommands,
    },
    /// Self-update operations.
    Update {
        /// Check for updates without installing.
        #[arg(long)]
        check: bool,
        #[command(subcommand)]
        command: Option<UpdateCommands>,
    },
    /// Emit shell completion script to stdout.
    Completions {
        /// Shell type to generate completions for.
        #[arg(long, value_enum)]
        shell: CompletionShell,
    },
    /// Configuration inspection commands.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Memory store inspection and maintenance commands.
    Memory {
        #[command(subcommand)]
        command: MemoryCommands,
    },
    /// Retrieval-augmented generation index operations.
    Rag {
        #[command(subcommand)]
        command: RagCommands,
    },
    /// Hardware discovery and inspection commands (feature-gated runtime).
    Hardware {
        #[command(subcommand)]
        command: HardwareCommands,
    },
    /// Peripheral registry commands (feature-gated runtime).
    Peripheral {
        #[command(subcommand)]
        command: PeripheralCommands,
    },
    /// Inspect provider quotas, rate limits, and circuit breaker state.
    ProvidersQuota {
        /// Provider name (defaults to configured default provider).
        #[arg(long)]
        provider: Option<String>,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Template management — discover, scaffold, and validate template files.
    Template {
        #[command(subcommand)]
        command: TemplateCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum ModelCommands {
    /// Refresh and cache provider models.
    Refresh {
        /// Provider name (defaults to configured default provider).
        #[arg(long)]
        provider: Option<String>,
        /// Refresh all providers that support live model discovery.
        #[arg(long)]
        all: bool,
        /// Force live refresh and ignore fresh cache.
        #[arg(long)]
        force: bool,
    },
    /// List cached models for a provider.
    List {
        /// Provider name (defaults to configured default provider).
        #[arg(long)]
        provider: Option<String>,
    },
    /// Set the default model in config.
    Set {
        /// Model name to set as default.
        model: String,
    },
    /// Show current model configuration and cache status.
    Status,
    /// Pull a model from a local provider (currently Ollama only).
    Pull {
        /// Model name to pull (e.g., llama3.1:8b).
        model: String,
        /// Provider to pull from (defaults to configured provider or ollama).
        #[arg(long)]
        provider: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum LocalCommands {
    /// Scan default ports for running local AI services.
    Discover {
        /// Probe timeout in milliseconds.
        #[arg(long, default_value_t = 2000)]
        timeout_ms: u64,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Show status of the configured local provider.
    Status {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Run health check on a specific local provider.
    Health {
        /// Provider name (ollama, llamacpp, lmstudio, vllm, sglang).
        provider: String,
        /// Custom base URL (overrides default).
        #[arg(long)]
        url: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum DoctorCommands {
    /// Probe model catalogs across providers and report availability.
    Models {
        /// Probe a specific provider only (default: all known providers).
        #[arg(long)]
        provider: Option<String>,
        /// Prefer cached catalogs when available (skip forced refresh behavior).
        #[arg(long)]
        use_cache: bool,
    },
    /// Query runtime trace events (tool diagnostics and model replies).
    Traces {
        /// Show a specific trace event by id.
        #[arg(long)]
        id: Option<String>,
        /// Filter list output by event type.
        #[arg(long)]
        event: Option<String>,
        /// Case-insensitive text match across message/payload.
        #[arg(long)]
        contains: Option<String>,
        /// Maximum number of events to display.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

#[derive(Debug, Subcommand)]
pub enum ServiceCommands {
    /// Install service metadata and local state.
    Install,
    /// Restart a running service.
    Restart,
    /// Mark service as running.
    Start,
    /// Mark service as stopped.
    Stop,
    /// Uninstall service metadata and local state.
    Uninstall,
    /// Show service status.
    Status,
}

#[derive(Debug, Subcommand)]
pub enum DaemonCommands {
    /// Start the daemon in the background.
    Start {
        /// Host interface to bind (default: 127.0.0.1).
        #[arg(long)]
        host: Option<String>,
        /// Port to bind (default: 8080).
        #[arg(short, long)]
        port: Option<u16>,
        /// Run in the foreground instead of daemonizing (useful for debugging).
        #[arg(long)]
        foreground: bool,
    },
    /// Stop the running daemon.
    Stop,
    /// Show daemon status.
    Status {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum CronCommands {
    /// List scheduled tasks.
    List {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Add a scheduled task.
    Add {
        #[arg(long)]
        id: String,
        #[arg(long)]
        schedule: String,
        #[arg(long)]
        command: String,
    },
    /// Add a scheduled task (at specific time).
    AddAt {
        #[arg(long)]
        id: String,
        #[arg(long)]
        schedule: String,
        #[arg(long)]
        command: String,
    },
    /// Add a scheduled task (recurring cadence).
    AddEvery {
        #[arg(long)]
        id: String,
        #[arg(long)]
        schedule: String,
        #[arg(long)]
        command: String,
    },
    /// Add a one-time scheduled task.
    Once {
        #[arg(long)]
        id: String,
        #[arg(long)]
        schedule: String,
        #[arg(long)]
        command: String,
    },
    /// Update a scheduled task.
    Update {
        #[arg(long)]
        id: String,
        #[arg(long)]
        schedule: Option<String>,
        #[arg(long)]
        command: Option<String>,
    },
    /// Pause a scheduled task.
    Pause {
        #[arg(long)]
        id: String,
    },
    /// Resume a scheduled task.
    Resume {
        #[arg(long)]
        id: String,
    },
    /// Remove a scheduled task.
    Remove {
        #[arg(long)]
        id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum HookCommands {
    /// List hook states.
    List {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Enable a hook.
    Enable {
        #[arg(long)]
        name: String,
    },
    /// Disable a hook.
    Disable {
        #[arg(long)]
        name: String,
    },
    /// Run a hook test.
    Test {
        #[arg(long)]
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum SkillCommands {
    /// List installed skills.
    List {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Install a skill.
    Install {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "local")]
        source: String,
    },
    /// Run a simple skill validation test.
    Test {
        #[arg(long)]
        name: String,
    },
    /// Remove an installed skill.
    Remove {
        #[arg(long)]
        name: String,
    },
    /// Scaffold a new skill project.
    New {
        /// Name for the new skill.
        name: String,
        /// Scaffold template language (typescript, rust, go, python).
        #[arg(long, default_value = "typescript")]
        template: String,
        /// Target directory (defaults to current directory).
        #[arg(long)]
        dir: Option<String>,
    },
    /// Audit an installed skill for security and compatibility.
    Audit {
        /// Name of the skill to audit.
        #[arg(long)]
        name: String,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// List available skill scaffold templates.
    Templates,
}

#[derive(Debug, Subcommand)]
pub enum TunnelCommands {
    /// Start or replace a tunnel session.
    Start {
        #[arg(long, default_value = "default")]
        name: String,
        /// Tunnel protocol (`http`, `https`, `ssh`).
        #[arg(long)]
        protocol: String,
        /// Remote target (`host:port`).
        #[arg(long)]
        remote: String,
        /// Local bind/listen port.
        #[arg(long)]
        local_port: u16,
    },
    /// Stop an active tunnel session.
    Stop {
        #[arg(long, default_value = "default")]
        name: String,
    },
    /// Show tunnel status.
    Status {
        #[arg(long, default_value = "default")]
        name: String,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum PluginCommands {
    /// Scaffold a plugin manifest template.
    New {
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "0.1.0")]
        version: String,
        #[arg(long, default_value = "run")]
        entrypoint: String,
        #[arg(long, default_value = "plugin.wasm")]
        wasm_file: String,
        #[arg(long)]
        out_dir: Option<String>,
        /// Overwrite existing manifest file if present.
        #[arg(long)]
        force: bool,
    },
    /// Validate a plugin manifest.
    Validate {
        #[arg(long)]
        manifest: String,
    },
    /// Run plugin runtime preflight and optional execution.
    Test {
        #[arg(long)]
        manifest: String,
        #[arg(long)]
        wasm: String,
        /// Execute the entrypoint after preflight.
        #[arg(long)]
        execute: bool,
    },
    /// Package plugin manifest + wasm into an installable archive.
    Package {
        #[arg(long)]
        manifest: String,
        #[arg(long)]
        wasm: String,
        #[arg(long)]
        out: String,
    },
    /// Run local deterministic plugin dev loop (validate + preflight + optional execute).
    Dev {
        #[arg(long)]
        manifest: String,
        #[arg(long)]
        wasm: String,
        /// Number of deterministic loop iterations to run.
        #[arg(long, default_value_t = 1)]
        iterations: u32,
        /// Execute plugin entrypoint in addition to preflight checks.
        #[arg(long, default_value_t = true)]
        execute: bool,
    },
    /// Install a packaged plugin archive.
    Install {
        #[arg(long)]
        package: String,
        #[arg(long)]
        install_dir: Option<String>,
    },
    /// List installed plugins.
    List {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
        #[arg(long)]
        install_dir: Option<String>,
    },
    /// Remove an installed plugin.
    Remove {
        #[arg(long)]
        id: String,
        /// Optional version; when omitted, removes all installed versions.
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        install_dir: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuthCommands {
    /// Login with OAuth (OpenAI Codex or Gemini).
    Login {
        /// Provider (`openai-codex` or `gemini`). Interactive prompt if omitted.
        #[arg(long)]
        provider: Option<String>,
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
        /// Token value. If omitted, reads from stdin prompt.
        #[arg(long)]
        token: Option<String>,
        /// Auth kind override (`authorization` or `api-key`).
        #[arg(long, value_enum)]
        auth_kind: Option<AuthKind>,
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
        /// Provider.
        #[arg(long)]
        provider: String,
        /// Profile name or full profile id.
        #[arg(long)]
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

#[derive(Debug, Subcommand)]
pub enum MigrateCommands {
    /// Migrate data from a source workspace.
    Openclaw {
        /// Source directory to import from.
        #[arg(long)]
        source: Option<String>,
        /// Validate and preview migration without writing files.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum UpdateCommands {
    /// Check whether an update is available.
    Check {
        /// Optional channel (for compatibility and future use).
        #[arg(long, default_value = "stable")]
        channel: String,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Apply an update target version.
    Apply {
        /// Target version to apply.
        #[arg(long)]
        version: String,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Roll back to the previous applied version.
    Rollback {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Show update state.
    Status {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum RagCommands {
    /// Ingest a text document into the local RAG index.
    Ingest {
        /// Document ID.
        #[arg(long)]
        id: String,
        /// Inline text content.
        #[arg(long)]
        text: Option<String>,
        /// Optional file path to ingest instead of --text.
        #[arg(long)]
        file: Option<String>,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Query local RAG index.
    Query {
        /// Query text.
        #[arg(long)]
        query: String,
        /// Maximum matches to return.
        #[arg(long, default_value_t = 5)]
        limit: usize,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum HardwareCommands {
    /// Discover available hardware boards.
    Discover,
    /// Show details for a specific chip.
    Info {
        /// Chip name (e.g. STM32F401RETx). Default: STM32F401RETx for Nucleo-F401RE.
        #[arg(long, default_value = "STM32F401RETx")]
        chip: String,
    },
    /// Alias for board info/introspection.
    Introspect,
}

#[derive(Debug, Subcommand)]
pub enum PeripheralCommands {
    /// List registered peripherals.
    List {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Add/register a peripheral.
    Add {
        /// Peripheral id.
        #[arg(long)]
        id: Option<String>,
        /// Peripheral kind.
        #[arg(long)]
        kind: Option<String>,
        /// Connection string/descriptor.
        #[arg(long)]
        connection: Option<String>,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Flash firmware to a configured peripheral.
    Flash {
        /// Optional target peripheral id.
        #[arg(long)]
        id: Option<String>,
        /// Optional firmware artifact path.
        #[arg(long)]
        firmware: Option<String>,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Flash firmware to a Nucleo board profile.
    FlashNucleo {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Run setup flow for Uno Q.
    SetupUnoQ {
        /// Uno Q IP (e.g. 192.168.0.48). If omitted, assumes running on-device.
        #[arg(long)]
        host: Option<String>,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ApprovalCommands {
    /// Evaluate an approval request and optionally apply a decision.
    Evaluate {
        #[arg(long)]
        actor: String,
        #[arg(long)]
        action: String,
        #[arg(long, value_enum)]
        risk: ApprovalRisk,
        /// Optional approver id when submitting a decision.
        #[arg(long)]
        approver: Option<String>,
        /// Optional decision (`allow` or `deny`) for high-risk requests.
        #[arg(long, value_enum)]
        decision: Option<ApprovalDecisionMode>,
        /// Optional reason attached to the decision.
        #[arg(long)]
        reason: Option<String>,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ApprovalRisk {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ApprovalDecisionMode {
    Allow,
    Deny,
}

#[derive(Debug, Subcommand)]
pub enum IdentityCommands {
    /// Create or update an identity.
    Upsert {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long, value_enum)]
        kind: IdentityKind,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Show identity by id.
    Get {
        #[arg(long)]
        id: String,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Add a role to an identity.
    AddRole {
        #[arg(long)]
        id: String,
        #[arg(long)]
        role: String,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum CoordinationCommands {
    /// Show current coordination status.
    Status {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Set worker/task counts for local coordination status.
    Set {
        #[arg(long)]
        active_workers: u32,
        #[arg(long)]
        queued_tasks: u32,
    },
}

#[derive(Debug, Subcommand)]
pub enum CostCommands {
    /// Show cost summary.
    Status {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Record usage into the summary.
    Record {
        #[arg(long)]
        tokens: u64,
        #[arg(long)]
        usd: f64,
    },
    /// Reset cost summary.
    Reset,
}

#[derive(Debug, Subcommand)]
pub enum GoalCommands {
    /// List known goals.
    List {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Add or replace a goal.
    Add {
        #[arg(long)]
        id: String,
        #[arg(long)]
        title: String,
    },
    /// Mark a goal complete.
    Complete {
        #[arg(long)]
        id: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum IdentityKind {
    Human,
    Agent,
    Service,
}

#[derive(Debug, Subcommand)]
pub enum EstopCommands {
    /// Print current estop status.
    Status,
    /// Resume from emergency stop.
    Resume {
        /// Resume only network kill.
        #[arg(long)]
        network: bool,
        /// Resume one or more blocked domain patterns.
        #[arg(long = "domain")]
        domains: Vec<String>,
        /// Resume one or more frozen tools.
        #[arg(long = "tool")]
        tools: Vec<String>,
        /// OTP code. If omitted and OTP is required, a prompt is shown.
        #[arg(long)]
        otp: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum EstopLevel {
    KillAll,
    NetworkKill,
    DomainBlock,
    ToolFreeze,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ServiceInit {
    Auto,
    Systemd,
    Openrc,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum AuthKind {
    Authorization,
    ApiKey,
}

#[derive(Debug, Subcommand)]
pub enum ChannelCommands {
    /// Add a channel. Optionally specify the channel name, e.g. `channel add telegram`.
    Add {
        /// Channel name to add. If omitted, prompts interactively or reads AGENTZERO_CHANNEL.
        name: Option<String>,
    },
    /// Run channel diagnostics.
    Doctor,
    /// List available channels.
    List,
    /// Remove a channel. Optionally specify the channel name.
    Remove {
        /// Channel name to remove. If omitted, prompts interactively or reads AGENTZERO_CHANNEL.
        name: Option<String>,
    },
    /// Start configured channels.
    Start,
}

#[derive(Debug, Subcommand)]
pub enum IntegrationsCommands {
    /// Show integration platform info.
    Info,
    /// List available integrations.
    List {
        /// Filter by category (for compatibility).
        #[arg(short = 'c', long)]
        category: Option<String>,
        /// Filter by status (for compatibility).
        #[arg(short = 's', long)]
        status: Option<String>,
    },
    /// Search integrations by free-text query.
    Search {
        /// Search query text.
        #[arg(long)]
        query: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    /// Print config schema/template to stdout.
    Schema {
        /// Emit JSON schema instead of TOML template.
        #[arg(long)]
        json: bool,
    },
    /// Print effective config as JSON (secrets masked).
    Show {
        /// Emit raw JSON without masking secrets.
        #[arg(long)]
        raw: bool,
    },
    /// Query a config value by dot-path (e.g. `provider.model`).
    Get {
        /// Dot-separated config path (e.g. `provider.model`, `agent.max_tool_iterations`).
        key: String,
    },
    /// Set a config value in agentzero.toml.
    Set {
        /// Dot-separated config path (e.g. `provider.model`).
        key: String,
        /// Value to set (type inferred: bool, integer, float, or string).
        value: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum TemplateCommands {
    /// List all template files with their status and source location.
    List {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Show the content of a specific template file.
    Show {
        /// Template name (e.g. AGENTS, BOOT, IDENTITY, SOUL, TOOLS, etc.).
        name: String,
    },
    /// Scaffold one or all template files into the workspace.
    Init {
        /// Scaffold a single template (e.g. AGENTS, BOOT). Omit to scaffold all.
        #[arg(long)]
        name: Option<String>,
        /// Target directory (defaults to workspace root).
        #[arg(long)]
        dir: Option<String>,
        /// Overwrite existing template files.
        #[arg(long)]
        force: bool,
    },
    /// Validate that template files are well-formed and discoverable.
    Validate,
}

#[derive(Debug, Subcommand)]
pub enum MemoryCommands {
    /// List memory entries.
    List {
        /// Maximum number of entries to return.
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Offset for pagination.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Optional category filter (reserved).
        #[arg(long)]
        category: Option<String>,
        /// Optional session filter (reserved).
        #[arg(long)]
        session: Option<String>,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Get a memory entry by key (prefix match).
    Get {
        /// Optional key/prefix. If omitted, returns the most recent entry.
        #[arg(long)]
        key: Option<String>,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Show memory statistics.
    Stats {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Clear memory entries.
    Clear {
        /// Delete entries by key (supports prefix match).
        #[arg(long)]
        key: Option<String>,
        /// Optional category filter (reserved).
        #[arg(long)]
        category: Option<String>,
        /// Skip confirmation for bulk clear.
        #[arg(long)]
        yes: bool,
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
}
