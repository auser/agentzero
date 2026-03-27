use crate::command_core::{AgentZeroCommand, CommandContext};
#[cfg(feature = "interactive")]
use crate::commands::ux;
#[cfg(feature = "interactive")]
use agentzero_providers::find_models_for_provider;
use agentzero_providers::find_provider;
#[cfg(feature = "interactive")]
use agentzero_providers::supported_providers;
use anyhow::Context;
use async_trait::async_trait;
#[cfg(feature = "interactive")]
use console::style;
#[cfg(feature = "interactive")]
use inquire::{Confirm, Select, Text};
use std::env;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;

pub struct OnboardOptions {
    pub interactive: bool,
    pub force: bool,
    pub channels_only: bool,
    pub api_key: Option<String>,
    /// Autoaccept all interactive questions
    pub yes: bool,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub memory: Option<String>,
    pub memory_path: Option<String>,
    pub no_totp: bool,
    pub allowed_root: Option<String>,
    pub allowed_commands: Vec<String>,
    /// NL description to bootstrap agents, tools, and channels.
    pub message: Option<String>,
}

pub struct OnboardCommand;

#[async_trait]
impl AgentZeroCommand for OnboardCommand {
    type Options = OnboardOptions;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let path = ctx.config_path.clone();
        let mut stdout = io::stdout();
        let stdin = io::stdin();
        let resolved = resolve_onboard_config(&opts, |key| env::var(key).ok())?;
        let _ = &opts.api_key;
        let _ = opts.no_totp;

        if opts.channels_only {
            writeln!(
                stdout,
                "channels-only flow is not implemented yet; running full onboarding instead"
            )?;
        }

        let interactive = opts.interactive && stdin.is_terminal() && !opts.yes;

        if interactive {
            #[cfg(feature = "interactive")]
            {
                run_with_inquire(ctx, &path, &mut stdout, opts.force, resolved)?;
            }
            #[cfg(not(feature = "interactive"))]
            {
                let mut reader = stdin.lock();
                run_with_io(&path, &mut reader, &mut stdout, true, opts.force, resolved)?;
            }
        } else {
            let mut reader = stdin.lock();
            run_with_io(&path, &mut reader, &mut stdout, false, opts.force, resolved)?;
        }

        // NL bootstrap phase: if the user passed -m, use the LLM to create
        // agents, tools, channels, and schedules from the description.
        if let Some(ref description) = opts.message {
            #[cfg(feature = "nl-evolving")]
            {
                writeln!(stdout)?;
                writeln!(stdout, "Bootstrapping from description...")?;

                let summary = run_nl_bootstrap(
                    ctx,
                    &path,
                    description,
                    opts.provider.as_deref(),
                    opts.model.as_deref(),
                )
                .await?;

                print_bootstrap_summary(&mut stdout, &summary)?;
            }
            #[cfg(not(feature = "nl-evolving"))]
            {
                let _ = description;
                anyhow::bail!(
                    "--message requires the 'nl-evolving' feature. \
                     Rebuild with: cargo build --features nl-evolving"
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct OnboardConfig {
    provider: String,
    base_url: String,
    model: String,
    memory_path: String,
    allowed_root: String,
    allowed_commands: Vec<String>,
}

impl Default for OnboardConfig {
    fn default() -> Self {
        Self {
            provider: "openrouter".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            model: "anthropic/claude-sonnet-4-6".to_string(),
            memory_path: default_memory_path(),
            allowed_root: ".".to_string(),
            allowed_commands: vec![
                "ls".to_string(),
                "pwd".to_string(),
                "cat".to_string(),
                "echo".to_string(),
            ],
        }
    }
}

fn default_memory_path() -> String {
    agentzero_core::common::paths::default_sqlite_path()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "./agentzero.db".to_string())
}

trait OnboardOptionSpec {
    type Value;

    fn env_var() -> &'static str;
    fn parse(raw: &str) -> anyhow::Result<Self::Value>;
}

struct ProviderSpec;
struct BaseUrlSpec;
struct ModelSpec;
struct MemoryPathSpec;
struct AllowedRootSpec;
struct AllowedCommandsSpec;

impl OnboardOptionSpec for ProviderSpec {
    type Value = String;

    fn env_var() -> &'static str {
        "AGENTZERO_PROVIDER"
    }

    fn parse(raw: &str) -> anyhow::Result<Self::Value> {
        parse_provider(raw)
    }
}

impl OnboardOptionSpec for BaseUrlSpec {
    type Value = String;

    fn env_var() -> &'static str {
        "AGENTZERO_BASE_URL"
    }

    fn parse(raw: &str) -> anyhow::Result<Self::Value> {
        let value = raw.trim();
        if value.is_empty() {
            anyhow::bail!("{} cannot be empty", Self::env_var());
        }
        Ok(value.to_string())
    }
}

impl OnboardOptionSpec for ModelSpec {
    type Value = String;

    fn env_var() -> &'static str {
        "AGENTZERO_MODEL"
    }

    fn parse(raw: &str) -> anyhow::Result<Self::Value> {
        let value = raw.trim();
        if value.is_empty() {
            anyhow::bail!("{} cannot be empty", Self::env_var());
        }
        Ok(value.to_string())
    }
}

impl OnboardOptionSpec for MemoryPathSpec {
    type Value = String;

    fn env_var() -> &'static str {
        "AGENTZERO_MEMORY_PATH"
    }

    fn parse(raw: &str) -> anyhow::Result<Self::Value> {
        let value = raw.trim();
        if value.is_empty() {
            anyhow::bail!("{} cannot be empty", Self::env_var());
        }
        Ok(value.to_string())
    }
}

impl OnboardOptionSpec for AllowedRootSpec {
    type Value = String;

    fn env_var() -> &'static str {
        "AGENTZERO_ALLOWED_ROOT"
    }

    fn parse(raw: &str) -> anyhow::Result<Self::Value> {
        let value = raw.trim();
        if value.is_empty() {
            anyhow::bail!("{} cannot be empty", Self::env_var());
        }
        Ok(value.to_string())
    }
}

impl OnboardOptionSpec for AllowedCommandsSpec {
    type Value = Vec<String>;

    fn env_var() -> &'static str {
        "AGENTZERO_ALLOWED_COMMANDS"
    }

    fn parse(raw: &str) -> anyhow::Result<Self::Value> {
        let parsed = parse_commands(raw);
        if parsed.is_empty() {
            anyhow::bail!(
                "environment variable {} must contain at least one command",
                Self::env_var()
            );
        }
        Ok(parsed)
    }
}

fn resolve_onboard_config(
    opts: &OnboardOptions,
    get_env: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<OnboardConfig> {
    let mut config = OnboardConfig::default();
    let provider_flag = opts.provider.as_deref().map(parse_provider).transpose()?;
    let base_url_flag = opts
        .base_url
        .as_deref()
        .map(BaseUrlSpec::parse)
        .transpose()?;
    let model_flag = opts.model.as_deref().map(ModelSpec::parse).transpose()?;
    let memory_path_flag = opts
        .memory_path
        .as_deref()
        .map(MemoryPathSpec::parse)
        .transpose()?;
    let memory_backend_flag = opts
        .memory
        .as_deref()
        .map(parse_memory_backend)
        .transpose()?;
    let allowed_root_flag = opts
        .allowed_root
        .as_deref()
        .map(AllowedRootSpec::parse)
        .transpose()?;

    config.provider =
        resolve_value::<ProviderSpec>(provider_flag, &get_env, config.provider.clone())?;
    config.base_url = resolve_optional::<BaseUrlSpec>(base_url_flag, &get_env)?
        .unwrap_or_else(|| default_base_url(&config.provider).to_string());
    config.model = resolve_value::<ModelSpec>(model_flag, &get_env, config.model.clone())?;
    let default_memory_path = memory_backend_flag
        .as_deref()
        .map(default_memory_path_for_backend)
        .unwrap_or_else(|| config.memory_path.clone());
    config.memory_path =
        resolve_value::<MemoryPathSpec>(memory_path_flag, &get_env, default_memory_path)?;
    config.allowed_root =
        resolve_value::<AllowedRootSpec>(allowed_root_flag, &get_env, config.allowed_root.clone())?;
    config.allowed_commands = resolve_value::<AllowedCommandsSpec>(
        explicit_commands(opts),
        &get_env,
        config.allowed_commands.clone(),
    )?;

    Ok(config)
}

fn explicit_commands(opts: &OnboardOptions) -> Option<Vec<String>> {
    let parsed = opts
        .allowed_commands
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

fn resolve_value<S: OnboardOptionSpec>(
    flag_value: Option<S::Value>,
    get_env: &impl Fn(&str) -> Option<String>,
    default_value: S::Value,
) -> anyhow::Result<S::Value> {
    if let Some(value) = flag_value {
        return Ok(value);
    }

    if let Some(raw) = get_env(S::env_var()) {
        return S::parse(&raw);
    }

    Ok(default_value)
}

fn resolve_optional<S: OnboardOptionSpec>(
    flag_value: Option<S::Value>,
    get_env: &impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Option<S::Value>> {
    if let Some(value) = flag_value {
        return Ok(Some(value));
    }

    if let Some(raw) = get_env(S::env_var()) {
        return Ok(Some(S::parse(&raw)?));
    }

    Ok(None)
}

fn parse_provider(raw: &str) -> anyhow::Result<String> {
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Ok("openrouter".to_string());
    }
    if let Some(descriptor) = find_provider(&value) {
        return Ok(descriptor.id.to_string());
    }
    anyhow::bail!("unsupported provider: {value}")
}

fn parse_memory_backend(raw: &str) -> anyhow::Result<String> {
    let value = raw.trim().to_ascii_lowercase();
    match value.as_str() {
        "sqlite" | "lucid" | "markdown" | "none" => Ok(value),
        _ => anyhow::bail!("unsupported memory backend: {value}"),
    }
}

fn default_memory_path_for_backend(backend: &str) -> String {
    match backend {
        "sqlite" => default_memory_path(),
        "markdown" => "./memory.md".to_string(),
        "lucid" => "./memory.lucid".to_string(),
        "none" => ":memory:".to_string(),
        _ => default_memory_path(),
    }
}

fn run_with_io(
    path: &Path,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    interactive: bool,
    force: bool,
    seed: OnboardConfig,
) -> anyhow::Result<()> {
    if path.exists() {
        writeln!(writer, "Config already exists at {}", path.display())
            .context("failed to write output")?;

        if force {
            // Intentionally overwrite without prompting.
        } else if interactive {
            let overwrite = prompt_yes_no(reader, writer, "Overwrite existing config?", false)?;
            if !overwrite {
                writeln!(writer, "Aborted. Existing config was left unchanged.")
                    .context("failed to write output")?;
                return Ok(());
            }
        } else {
            return Ok(());
        }
    }

    let config = if interactive {
        collect_interactive_config(reader, writer, &seed)?
    } else {
        seed
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create config directory")?;
    }
    fs::write(path, render_template(&config)).context("failed to write agentzero.toml")?;
    writeln!(writer, "Created {}", path.display()).context("failed to write output")?;
    writeln!(
        writer,
        "Set OPENAI_API_KEY and run: cargo run -p agentzero -- agent -m \"hello\""
    )
    .context("failed to write output")?;
    Ok(())
}

#[cfg(feature = "interactive")]
fn run_with_inquire(
    ctx: &CommandContext,
    path: &Path,
    writer: &mut dyn Write,
    force: bool,
    defaults: OnboardConfig,
) -> anyhow::Result<()> {
    ux::print_brand_header(writer)?;
    ux::print_intro(
        writer,
        "Quick setup: generating config with sensible defaults...\n",
    )?;

    if path.exists() && !force {
        let overwrite = Confirm::new(&format!(
            "Config already exists at {}. Overwrite?",
            path.display()
        ))
        .with_default(false)
        .with_help_message("Select no to keep your existing setup.")
        .prompt()?;
        if !overwrite {
            writeln!(writer, "{}", style("No changes made.").yellow())
                .context("failed to write output")?;
            return Ok(());
        }
    }

    ux::print_section(writer, "Provider Setup")?;
    let providers: Vec<String> = supported_providers()
        .iter()
        .map(|p| p.id.to_string())
        .collect();
    let provider_index = providers
        .iter()
        .position(|value| value == &defaults.provider)
        .unwrap_or(0);
    let provider = Select::new("Provider", providers)
        .with_starting_cursor(provider_index)
        .with_help_message("Type to filter options.")
        .with_page_size(12)
        .prompt()?;

    let base_url_selection = Select::new(
        "Provider base URL",
        base_url_options(&provider, &defaults.base_url),
    )
    .with_help_message("Type to filter options. Choose custom to enter your own URL.")
    .with_starting_cursor(base_url_start_cursor(&provider, &defaults.base_url))
    .prompt()?;
    let base_url = if base_url_selection == "custom" {
        Text::new("Provider base URL")
            .with_help_message("Press Enter to accept the default URL.")
            .with_initial_value(&defaults.base_url)
            .prompt()?
    } else {
        base_url_selection
    };

    let model_selection = Select::new("Model", model_options(&provider))
        .with_help_message("Type to filter options. Choose custom to enter your own model.")
        .with_starting_cursor(model_start_cursor(&provider, &defaults.model))
        .prompt()?;

    let model = if model_selection == "custom" {
        Text::new("Custom model ID")
            .with_help_message("Example: gpt-4o-mini, claude-3-5-sonnet, etc.")
            .with_initial_value(&defaults.model)
            .prompt()?
    } else {
        model_selection
    };
    ux::print_success_line(
        writer,
        &format!(
            "Provider configured: {} / {}",
            ux::cyan_value(&provider),
            ux::cyan_value(&model)
        ),
    )?;

    ux::print_section(writer, "Memory Setup")?;
    let memory_path_selection =
        Select::new("Memory db path", memory_path_options(&defaults.memory_path))
            .with_help_message("Type to filter options. Choose custom to enter your own path.")
            .with_starting_cursor(memory_path_start_cursor(&defaults.memory_path))
            .prompt()?;
    let memory_path = if memory_path_selection == "custom" {
        Text::new("Memory db path")
            .with_initial_value(&defaults.memory_path)
            .prompt()?
    } else {
        memory_path_selection
    };
    ux::print_success_line(
        writer,
        &format!("Memory configured: {}", ux::cyan_value(&memory_path)),
    )?;

    ux::print_section(writer, "Security Setup")?;
    let allowed_root_selection = Select::new(
        "Security allowed root",
        allowed_root_options(&defaults.allowed_root),
    )
    .with_help_message("Type to filter options. Choose custom to enter your own root.")
    .with_starting_cursor(allowed_root_start_cursor(&defaults.allowed_root))
    .prompt()?;
    let allowed_root = if allowed_root_selection == "custom" {
        Text::new("Security allowed root")
            .with_initial_value(&defaults.allowed_root)
            .prompt()?
    } else {
        allowed_root_selection
    };

    let allowed_commands = loop {
        let defaults_joined = defaults.allowed_commands.join(",");
        let commands_selection = Select::new(
            "Allowed shell commands",
            allowed_commands_options(&defaults_joined),
        )
        .with_help_message("Type to filter options. Choose custom to enter a comma-separated list.")
        .with_starting_cursor(allowed_commands_start_cursor(&defaults_joined))
        .prompt()?;
        let value = if commands_selection == "custom" {
            Text::new("Allowed shell commands (comma-separated)")
                .with_initial_value(&defaults_joined)
                .with_help_message("Example: ls,pwd,cat,echo")
                .prompt()?
        } else {
            commands_selection
        };
        let parsed = parse_commands(&value);
        if parsed.is_empty() {
            writeln!(
                writer,
                "{}",
                style("At least one command is required. Please try again.").yellow()
            )
            .context("failed to write output")?;
            continue;
        }
        break parsed;
    };
    ux::print_success_line(
        writer,
        &format!(
            "Security configured: root={} commands={}",
            ux::cyan_value(&allowed_root),
            ux::cyan_value(allowed_commands.join(","))
        ),
    )?;

    let config = OnboardConfig {
        provider,
        base_url,
        model,
        memory_path,
        allowed_root,
        allowed_commands,
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create config directory")?;
    }
    fs::write(path, render_template(&config)).context("failed to write agentzero.toml")?;
    print_summary(writer, ctx, path, &config)?;
    Ok(())
}

fn collect_interactive_config(
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    defaults: &OnboardConfig,
) -> anyhow::Result<OnboardConfig> {
    writeln!(writer, "AgentZero onboarding").context("failed to write output")?;
    writeln!(writer, "Press Enter to accept defaults.").context("failed to write output")?;

    let base_url = prompt_with_default(reader, writer, "Provider base URL", &defaults.base_url)?;
    let model = prompt_with_default(reader, writer, "Provider model", &defaults.model)?;
    let memory_path = prompt_with_default(reader, writer, "Memory db path", &defaults.memory_path)?;
    let allowed_root = prompt_with_default(
        reader,
        writer,
        "Security allowed root",
        &defaults.allowed_root,
    )?;
    let allowed_commands = prompt_commands(
        reader,
        writer,
        "Allowed shell commands",
        &defaults.allowed_commands,
    )?;

    Ok(OnboardConfig {
        provider: defaults.provider.clone(),
        base_url,
        model,
        memory_path,
        allowed_root,
        allowed_commands,
    })
}

fn prompt_with_default(
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    label: &str,
    default: &str,
) -> anyhow::Result<String> {
    write!(writer, "{label} [{default}]: ").context("failed to write output")?;
    writer.flush().context("failed to flush output")?;

    let input = read_line(reader)?;
    if input.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(input)
    }
}

fn prompt_commands(
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    label: &str,
    default: &[String],
) -> anyhow::Result<Vec<String>> {
    loop {
        let default_value = default.join(",");
        let input = prompt_with_default(reader, writer, label, &default_value)?;
        let commands = parse_commands(&input);

        if commands.is_empty() {
            writeln!(
                writer,
                "Please provide at least one command (comma-separated)."
            )
            .context("failed to write output")?;
            continue;
        }

        return Ok(commands);
    }
}

fn prompt_yes_no(
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    label: &str,
    default: bool,
) -> anyhow::Result<bool> {
    let hint = if default { "Y/n" } else { "y/N" };

    loop {
        write!(writer, "{label} [{hint}]: ").context("failed to write output")?;
        writer.flush().context("failed to flush output")?;

        let input = read_line(reader)?;
        if input.is_empty() {
            return Ok(default);
        }

        match input.to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => {
                writeln!(writer, "Please answer with y/yes or n/no.")
                    .context("failed to write output")?;
            }
        }
    }
}

fn read_line(reader: &mut dyn BufRead) -> anyhow::Result<String> {
    let mut buf = String::new();
    let read = reader
        .read_line(&mut buf)
        .context("failed to read interactive input")?;
    if read == 0 {
        anyhow::bail!("unexpected end of input");
    }
    Ok(buf.trim().to_string())
}

fn parse_commands(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn toml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn render_template(config: &OnboardConfig) -> String {
    let commands = config
        .allowed_commands
        .iter()
        .map(|command| toml_string(command))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "[provider]\nname = {}\nbase_url = {}\nmodel = {}\n\n[memory]\npath = {}\n\n[agent]\nmode = \"development\"\nmax_tool_iterations = 4\nrequest_timeout_ms = 30000\nmemory_window_size = 8\nmax_prompt_chars = 8000\n\n[agent.hooks]\nenabled = false\ntimeout_ms = 250\nfail_closed = false\non_error_default = \"warn\"\non_error_low = \"ignore\"\non_error_medium = \"warn\"\non_error_high = \"block\"\n\n[security]\nallowed_root = {}\nallowed_commands = [{}]\n\n[security.read_file]\nmax_read_bytes = 65536\nallow_binary = false\n\n[security.write_file]\nenabled = false\nmax_write_bytes = 65536\n\n[security.shell]\nmax_args = 8\nmax_arg_length = 128\nmax_output_bytes = 8192\nforbidden_chars = \";&|><$`\\n\\r\"\n\n[security.mcp]\nenabled = false\nallowed_servers = []\n\n[security.plugin]\nenabled = false\n\n[security.audit]\nenabled = false\npath = \"./agentzero-audit.log\"\n",
        toml_string(&config.provider),
        toml_string(&config.base_url),
        toml_string(&config.model),
        toml_string(&config.memory_path),
        toml_string(&config.allowed_root),
        commands
    )
}

fn default_base_url(provider: &str) -> &str {
    match provider {
        "openrouter" => "https://openrouter.ai/api",
        "anthropic" => "https://api.anthropic.com",
        // In-process providers don't need a base URL
        "builtin" | "candle" => "",
        _ => "https://api.openai.com",
    }
}

#[cfg(feature = "interactive")]
fn model_options(provider: &str) -> Vec<String> {
    let mut options: Vec<String> = find_models_for_provider(provider)
        .map(|(_, models)| models.iter().map(|m| m.id.to_string()).collect())
        .unwrap_or_default();
    options.push("custom".to_string());
    options
}

#[cfg(feature = "interactive")]
fn model_start_cursor(provider: &str, current_model: &str) -> usize {
    let options = model_options(provider);
    options
        .iter()
        .position(|value| value == current_model)
        .or_else(|| options.iter().position(|value| value == "custom"))
        .unwrap_or(0)
}

#[cfg(feature = "interactive")]
fn base_url_options(provider: &str, current_url: &str) -> Vec<String> {
    unique_options(vec![
        default_base_url(provider).to_string(),
        "https://api.openai.com".to_string(),
        "https://openrouter.ai/api".to_string(),
        "https://api.anthropic.com".to_string(),
        current_url.to_string(),
        "custom".to_string(),
    ])
}

#[cfg(feature = "interactive")]
fn base_url_start_cursor(provider: &str, current_url: &str) -> usize {
    let options = base_url_options(provider, current_url);
    options
        .iter()
        .position(|value| value == current_url)
        .or_else(|| {
            options
                .iter()
                .position(|value| value == default_base_url(provider))
        })
        .unwrap_or(0)
}

#[cfg(feature = "interactive")]
fn memory_path_options(current_path: &str) -> Vec<String> {
    let default_path = default_memory_path();
    unique_options(vec![
        default_path,
        "./agentzero.db".to_string(),
        "./.agentzero/agentzero.db".to_string(),
        current_path.to_string(),
        "custom".to_string(),
    ])
}

#[cfg(feature = "interactive")]
fn memory_path_start_cursor(current_path: &str) -> usize {
    let options = memory_path_options(current_path);
    options
        .iter()
        .position(|value| value == current_path)
        .unwrap_or(0)
}

#[cfg(feature = "interactive")]
fn allowed_root_options(current_root: &str) -> Vec<String> {
    unique_options(vec![
        ".".to_string(),
        "./workspace".to_string(),
        current_root.to_string(),
        "custom".to_string(),
    ])
}

#[cfg(feature = "interactive")]
fn allowed_root_start_cursor(current_root: &str) -> usize {
    let options = allowed_root_options(current_root);
    options
        .iter()
        .position(|value| value == current_root)
        .unwrap_or(0)
}

#[cfg(feature = "interactive")]
fn allowed_commands_options(current_commands: &str) -> Vec<String> {
    unique_options(vec![
        "ls,pwd,cat,echo".to_string(),
        "ls,pwd,cat".to_string(),
        "ls,pwd".to_string(),
        current_commands.to_string(),
        "custom".to_string(),
    ])
}

#[cfg(feature = "interactive")]
fn allowed_commands_start_cursor(current_commands: &str) -> usize {
    let options = allowed_commands_options(current_commands);
    options
        .iter()
        .position(|value| value == current_commands)
        .unwrap_or(0)
}

#[cfg(feature = "interactive")]
fn unique_options(options: Vec<String>) -> Vec<String> {
    let mut unique = Vec::new();
    for option in options {
        if !option.trim().is_empty() && !unique.contains(&option) {
            unique.push(option);
        }
    }
    unique
}

#[cfg(feature = "interactive")]
fn print_summary(
    writer: &mut dyn Write,
    ctx: &CommandContext,
    path: &Path,
    config: &OnboardConfig,
) -> anyhow::Result<()> {
    ux::print_success_line(writer, "Config generated successfully")?;
    ux::print_success_line(writer, "Created 1 file, skipped 0 existing")?;
    ux::print_success_line(
        writer,
        &format!(
            "Workspace: {}",
            ux::cyan_value(ctx.workspace_root.display())
        ),
    )?;
    ux::print_success_line(
        writer,
        &format!("Provider: {}", ux::cyan_value(&config.provider)),
    )?;
    ux::print_success_line(writer, &format!("Model: {}", ux::cyan_value(&config.model)))?;
    ux::print_success_line(
        writer,
        &format!(
            "Security: {}",
            ux::cyan_value(format!(
                "root={}, commands={}",
                config.allowed_root,
                config.allowed_commands.join(",")
            ))
        ),
    )?;
    ux::print_success_line(
        writer,
        &format!("Memory: {}", ux::cyan_value(&config.memory_path)),
    )?;
    ux::print_success_line(
        writer,
        &format!("Config saved: {}", ux::cyan_value(path.display())),
    )?;
    writeln!(writer, "\nNext steps:").context("failed to write output")?;
    writeln!(writer, "  1) export OPENAI_API_KEY=\"sk-...\"").context("failed to write output")?;
    writeln!(writer, "  2) cargo run -p agentzero -- agent -m \"Hello\"")
        .context("failed to write output")?;
    writeln!(writer, "  3) cargo run -p agentzero -- gateway").context("failed to write output")?;
    Ok(())
}

// ── NL Bootstrap ────────────────────────────────────────────────────────────

#[cfg(feature = "nl-evolving")]
mod nl_bootstrap {
    use super::*;
    use crate::command_core::CommandContext;
    use agentzero_infra::runtime::build_provider_from_config;
    use agentzero_infra::tools::agent_manage::create_agent_from_nl;
    use agentzero_infra::tools::dynamic_tool::DynamicToolRegistry;
    use agentzero_infra::tools::tool_create::create_tool_from_nl;
    use agentzero_orchestrator::agent_store::AgentStore;
    use agentzero_tools::cron_store::CronStore;
    use serde::Deserialize;
    use std::sync::Arc;

    const BOOTSTRAP_PLANNER_PROMPT: &str = r#"You are a setup planner for an AI agent system called AgentZero. Given a natural language description of what the user wants, output a JSON plan for what needs to be created.

Output a JSON object with this exact structure:
{
  "agents": [
    {
      "description": "Natural language description of this agent's role and behavior"
    }
  ],
  "channels": [
    {
      "type": "email",
      "reason": "User mentioned watching inbox"
    }
  ],
  "tools": [
    {
      "description": "Natural language description of a custom tool to create",
      "strategy_hint": "shell"
    }
  ],
  "schedules": [
    {
      "agent_index": 0,
      "cron": "*/5 * * * *",
      "reason": "Check inbox every 5 minutes"
    }
  ]
}

Rules:
- agents[].description: detailed NL description that will be passed to the agent creation system. Include the agent's purpose, expertise, and behavioral guidelines.
- channels[].type: one of: email, telegram, discord, slack, matrix, irc, sms, webhook, signal, whatsapp, nostr, mqtt, cli
- tools[]: only include if the user needs a CUSTOM tool not already built-in. Common built-in tools already exist: shell, read_file, write_file, web_search, web_fetch, http_request, git_operations, content_search, cron_add, send_message
- schedules[].agent_index: index into the agents array (0-based) indicating which agent this schedule is for
- schedules[].cron: standard 5-field cron expression
- Infer channels from context clues: "inbox"/"email" -> email, "Slack messages" -> slack, "Discord" -> discord, "text me" -> sms, "Telegram" -> telegram
- Keep it minimal — prefer fewer agents with broader scope over many narrow agents
- Output ONLY the JSON object, no markdown fences or explanation"#;

    #[derive(Debug, Deserialize)]
    struct BootstrapPlan {
        #[serde(default)]
        agents: Vec<BootstrapAgent>,
        #[serde(default)]
        channels: Vec<BootstrapChannel>,
        #[serde(default)]
        tools: Vec<BootstrapTool>,
        #[serde(default)]
        schedules: Vec<BootstrapSchedule>,
    }

    #[derive(Debug, Deserialize)]
    struct BootstrapAgent {
        description: String,
    }

    #[derive(Debug, Deserialize)]
    struct BootstrapChannel {
        #[serde(rename = "type")]
        channel_type: String,
        reason: String,
    }

    #[derive(Debug, Deserialize)]
    struct BootstrapTool {
        description: String,
        #[serde(default)]
        strategy_hint: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct BootstrapSchedule {
        agent_index: usize,
        cron: String,
        reason: String,
    }

    pub(crate) struct BootstrapSummary {
        pub agents_created: Vec<String>,
        pub channels_needed: Vec<(String, String)>,
        pub tools_created: Vec<String>,
        pub schedules_added: Vec<String>,
        pub warnings: Vec<String>,
    }

    /// Parse a JSON plan from an LLM response, handling markdown fences.
    fn parse_bootstrap_json(response: &str) -> anyhow::Result<BootstrapPlan> {
        let trimmed = response.trim();

        if let Some(start) = trimmed.find("```json") {
            let after = &trimmed[start + 7..];
            if let Some(end) = after.find("```") {
                if let Ok(v) = serde_json::from_str(after[..end].trim()) {
                    return Ok(v);
                }
            }
        }

        if let Some(start) = trimmed.find("```") {
            let after = &trimmed[start + 3..];
            if let Some(end) = after.find("```") {
                if let Ok(v) = serde_json::from_str(after[..end].trim()) {
                    return Ok(v);
                }
            }
        }

        if let Some(start) = trimmed.find('{') {
            if let Some(end) = trimmed.rfind('}') {
                if let Ok(v) = serde_json::from_str(&trimmed[start..=end]) {
                    return Ok(v);
                }
            }
        }

        serde_json::from_str(trimmed)
            .map_err(|e| anyhow::anyhow!("failed to parse bootstrap plan from LLM: {e}"))
    }

    pub(crate) async fn run_nl_bootstrap(
        ctx: &CommandContext,
        config_path: &std::path::Path,
        description: &str,
        provider_override: Option<&str>,
        model_override: Option<&str>,
    ) -> anyhow::Result<BootstrapSummary> {
        let provider =
            build_provider_from_config(config_path, provider_override, model_override, None)
                .await
                .context("failed to initialise LLM provider for NL bootstrap")?;

        let planner_prompt = format!("{BOOTSTRAP_PLANNER_PROMPT}\n\nUser request: {description}");
        let plan_result = provider
            .complete(&planner_prompt)
            .await
            .context("LLM call for bootstrap planning failed")?;
        let plan = parse_bootstrap_json(&plan_result.output_text)?;

        let mut summary = BootstrapSummary {
            agents_created: Vec::new(),
            channels_needed: Vec::new(),
            tools_created: Vec::new(),
            schedules_added: Vec::new(),
            warnings: Vec::new(),
        };

        let mut agent_names: Vec<String> = Vec::new();

        // ── Create agents ───────────────────────────────────────────────
        if !plan.agents.is_empty() {
            let agent_store =
                AgentStore::persistent(&ctx.data_dir).context("failed to open agent store")?;

            for (i, agent_def) in plan.agents.iter().enumerate() {
                match create_agent_from_nl(&agent_store, provider.as_ref(), &agent_def.description)
                    .await
                {
                    Ok((record, _schedule)) => {
                        agent_names.push(record.name.clone());
                        summary.agents_created.push(record.name);
                    }
                    Err(e) => {
                        agent_names.push(format!("agent_{i}"));
                        summary
                            .warnings
                            .push(format!("failed to create agent {i}: {e}"));
                    }
                }
            }
        }

        // ── Create custom tools ─────────────────────────────────────────
        if !plan.tools.is_empty() {
            let registry = Arc::new(
                DynamicToolRegistry::open(&ctx.data_dir)
                    .context("failed to open dynamic tool registry")?,
            );

            for tool_def in &plan.tools {
                match create_tool_from_nl(
                    &registry,
                    provider.as_ref(),
                    &tool_def.description,
                    tool_def.strategy_hint.as_deref(),
                )
                .await
                {
                    Ok(name) => summary.tools_created.push(name),
                    Err(e) => summary.warnings.push(format!(
                        "failed to create tool '{}': {e}",
                        tool_def.description
                    )),
                }
            }
        }

        // ── Note channels that need setup ───────────────────────────────
        for ch in &plan.channels {
            summary
                .channels_needed
                .push((ch.channel_type.clone(), ch.reason.clone()));
        }

        // ── Create cron schedules ───────────────────────────────────────
        if !plan.schedules.is_empty() {
            let cron_store = CronStore::new(&ctx.data_dir)?;

            for sched in &plan.schedules {
                let agent_name = agent_names
                    .get(sched.agent_index)
                    .cloned()
                    .unwrap_or_else(|| format!("agent_{}", sched.agent_index));

                let task_id = format!("bootstrap_{agent_name}");
                let command = format!("agent -m \"run scheduled task\" --agent {agent_name}");

                match cron_store.add(&task_id, &sched.cron, &command) {
                    Ok(_) => summary.schedules_added.push(format!(
                        "{} ({}) — {}",
                        agent_name, sched.cron, sched.reason
                    )),
                    Err(e) => summary
                        .warnings
                        .push(format!("failed to add schedule for {agent_name}: {e}")),
                }
            }
        }

        Ok(summary)
    }

    pub(crate) fn print_bootstrap_summary(
        writer: &mut dyn Write,
        summary: &BootstrapSummary,
    ) -> anyhow::Result<()> {
        writeln!(writer, "Bootstrap complete!")?;
        writeln!(writer)?;

        if !summary.agents_created.is_empty() {
            writeln!(writer, "Agents created:")?;
            for name in &summary.agents_created {
                writeln!(writer, "  + {name}")?;
            }
        }

        if !summary.tools_created.is_empty() {
            writeln!(writer, "Custom tools created:")?;
            for name in &summary.tools_created {
                writeln!(writer, "  + {name}")?;
            }
        }

        if !summary.schedules_added.is_empty() {
            writeln!(writer, "Schedules added:")?;
            for desc in &summary.schedules_added {
                writeln!(writer, "  + {desc}")?;
            }
        }

        if !summary.channels_needed.is_empty() {
            writeln!(writer)?;
            writeln!(writer, "Channels to configure:")?;
            for (ch_type, reason) in &summary.channels_needed {
                writeln!(writer, "  -> {ch_type} ({reason})")?;
                writeln!(writer, "     Run: agentzero channel add {ch_type}")?;
            }
        }

        if !summary.warnings.is_empty() {
            writeln!(writer)?;
            writeln!(writer, "Warnings:")?;
            for w in &summary.warnings {
                writeln!(writer, "  ! {w}")?;
            }
        }

        Ok(())
    }
}

#[cfg(feature = "nl-evolving")]
use nl_bootstrap::{print_bootstrap_summary, run_nl_bootstrap};

#[cfg(test)]
mod tests {
    use super::{
        allowed_commands_options, base_url_options, model_options, parse_provider,
        resolve_onboard_config, run_with_io, OnboardConfig, OnboardOptions,
    };
    use std::fs;
    use std::io::Cursor;
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
            "agentzero-onboard-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn creates_config_from_interactive_answers() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        let mut input = Cursor::new(
            "https://example.invalid\n\
             gpt-custom\n\
             ./custom.db\n\
             ./workspace\n\
             ls,pwd\n",
        );
        let mut output = Vec::new();

        run_with_io(
            &config_path,
            &mut input,
            &mut output,
            true,
            false,
            OnboardConfig::default(),
        )
        .expect("onboard should succeed");

        let content =
            fs::read_to_string(&config_path).expect("interactive onboarding should create config");
        assert!(content.contains("base_url = \"https://example.invalid\""));
        assert!(content.contains("model = \"gpt-custom\""));
        assert!(content.contains("path = \"./custom.db\""));
        assert!(content.contains("allowed_root = \"./workspace\""));
        assert!(content.contains("allowed_commands = [\"ls\", \"pwd\"]"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn does_not_overwrite_when_user_declines() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "sentinel = true\n").expect("seed config should be written");

        let mut input = Cursor::new("n\n");
        let mut output = Vec::new();

        run_with_io(
            &config_path,
            &mut input,
            &mut output,
            true,
            false,
            OnboardConfig::default(),
        )
        .expect("declining overwrite should not error");

        let content = fs::read_to_string(&config_path).expect("existing file should still exist");
        assert_eq!(content, "sentinel = true\n");

        let stdout = String::from_utf8(output).expect("output should be utf8");
        assert!(stdout.contains("Config already exists"));
        assert!(stdout.contains("Aborted. Existing config was left unchanged."));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn resolves_onboard_config_with_env_values() {
        let opts = OnboardOptions {
            interactive: false,
            force: false,
            channels_only: false,
            api_key: None,
            yes: false,
            provider: None,
            base_url: None,
            model: None,
            memory: None,
            memory_path: None,
            no_totp: false,
            allowed_root: None,
            allowed_commands: vec![],
            message: None,
        };

        let cfg = resolve_onboard_config(&opts, |key| match key {
            "AGENTZERO_PROVIDER" => Some("openrouter".to_string()),
            "AGENTZERO_BASE_URL" => Some("https://openrouter.ai/api".to_string()),
            "AGENTZERO_MODEL" => Some("anthropic/claude-3.5-sonnet".to_string()),
            "AGENTZERO_MEMORY_PATH" => Some("./env.db".to_string()),
            "AGENTZERO_ALLOWED_ROOT" => Some("./workspace".to_string()),
            "AGENTZERO_ALLOWED_COMMANDS" => Some("ls,pwd".to_string()),
            _ => None,
        })
        .expect("config should resolve");

        assert_eq!(cfg.provider, "openrouter");
        assert_eq!(cfg.base_url, "https://openrouter.ai/api");
        assert_eq!(cfg.model, "anthropic/claude-3.5-sonnet");
        assert_eq!(cfg.memory_path, "./env.db");
        assert_eq!(cfg.allowed_root, "./workspace");
        assert_eq!(
            cfg.allowed_commands,
            vec!["ls".to_string(), "pwd".to_string()]
        );
    }

    #[test]
    fn flag_values_override_env_values() {
        let opts = OnboardOptions {
            interactive: false,
            force: false,
            channels_only: false,
            api_key: None,
            yes: false,
            provider: Some("anthropic".to_string()),
            base_url: Some("https://example.invalid".to_string()),
            model: Some("claude-3-5-haiku-latest".to_string()),
            memory: None,
            memory_path: Some("./flag.db".to_string()),
            no_totp: false,
            allowed_root: Some("./flag-root".to_string()),
            allowed_commands: vec!["cat".to_string(), "echo".to_string()],
            message: None,
        };

        let cfg = resolve_onboard_config(&opts, |key| match key {
            "AGENTZERO_PROVIDER" => Some("openrouter".to_string()),
            "AGENTZERO_BASE_URL" => Some("https://openrouter.ai/api".to_string()),
            "AGENTZERO_MODEL" => Some("anthropic/claude-3.5-sonnet".to_string()),
            "AGENTZERO_MEMORY_PATH" => Some("./env.db".to_string()),
            "AGENTZERO_ALLOWED_ROOT" => Some("./workspace".to_string()),
            "AGENTZERO_ALLOWED_COMMANDS" => Some("ls,pwd".to_string()),
            _ => None,
        })
        .expect("config should resolve");

        assert_eq!(cfg.provider, "anthropic");
        assert_eq!(cfg.base_url, "https://example.invalid");
        assert_eq!(cfg.model, "claude-3-5-haiku-latest");
        assert_eq!(cfg.memory_path, "./flag.db");
        assert_eq!(cfg.allowed_root, "./flag-root");
        assert_eq!(
            cfg.allowed_commands,
            vec!["cat".to_string(), "echo".to_string()]
        );
    }

    #[test]
    fn base_url_options_include_current_and_custom_without_duplicates() {
        let options = base_url_options("openai", "https://example.invalid");
        assert!(options.contains(&"https://api.openai.com".to_string()));
        assert!(options.contains(&"https://example.invalid".to_string()));
        assert!(options.contains(&"custom".to_string()));
        assert_eq!(options.len(), 5);
    }

    #[test]
    fn allowed_commands_options_include_custom_and_filter_empty() {
        let options = allowed_commands_options("");
        assert!(options.contains(&"ls,pwd,cat,echo".to_string()));
        assert!(options.contains(&"custom".to_string()));
        assert!(!options.contains(&"".to_string()));
    }

    #[test]
    fn parse_provider_resolves_alias_success_path() {
        let result = parse_provider("github-copilot").expect("alias should resolve");
        assert_eq!(result, "copilot");
    }

    #[test]
    fn parse_provider_rejects_unknown_negative_path() {
        let err = parse_provider("not-real").expect_err("unknown provider should fail");
        assert!(err.to_string().contains("unsupported provider"));
    }

    #[test]
    fn model_options_returns_catalog_models_with_custom_success_path() {
        let options = model_options("openrouter");
        assert!(
            options.last().map(|s| s.as_str()) == Some("custom"),
            "last option should be 'custom'"
        );
        assert!(
            options.iter().any(|m| m.contains("claude")),
            "openrouter models should include a claude model"
        );
    }
}
