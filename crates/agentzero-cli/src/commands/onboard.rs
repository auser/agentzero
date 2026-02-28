use crate::command_core::{AgentZeroCommand, CommandContext};
use crate::commands::ux;
use anyhow::Context;
use async_trait::async_trait;
use console::style;
use inquire::{Confirm, Select, Text};
use std::env;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;

pub struct OnboardOptions {
    /// Autoaccept all interactive questions
    pub yes: bool,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub memory_path: Option<String>,
    pub allowed_root: Option<String>,
    pub allowed_commands: Vec<String>,
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

        if stdin.is_terminal() && !opts.yes {
            run_with_inquire(ctx, &path, &mut stdout, resolved)?;
        } else {
            let mut reader = stdin.lock();
            run_with_io(&path, &mut reader, &mut stdout, false, resolved)?;
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
            provider: "openai".to_string(),
            base_url: "https://api.openai.com".to_string(),
            model: "gpt-4o-mini".to_string(),
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
    agentzero_common::paths::default_sqlite_path()
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
    config.memory_path =
        resolve_value::<MemoryPathSpec>(memory_path_flag, &get_env, config.memory_path.clone())?;
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
    match value.as_str() {
        "" => Ok("openai".to_string()),
        "openai" | "openrouter" | "anthropic" => Ok(value),
        _ => anyhow::bail!("unsupported provider: {value}"),
    }
}

fn run_with_io(
    path: &Path,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    interactive: bool,
    seed: OnboardConfig,
) -> anyhow::Result<()> {
    if path.exists() {
        writeln!(writer, "Config already exists at {}", path.display())
            .context("failed to write output")?;

        if interactive {
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

    fs::write(path, render_template(&config)).context("failed to write agentzero.toml")?;
    writeln!(writer, "Created {}", path.display()).context("failed to write output")?;
    writeln!(
        writer,
        "Set OPENAI_API_KEY and run: cargo run -p agentzero -- agent -m \"hello\""
    )
    .context("failed to write output")?;
    Ok(())
}

fn run_with_inquire(
    ctx: &CommandContext,
    path: &Path,
    writer: &mut dyn Write,
    defaults: OnboardConfig,
) -> anyhow::Result<()> {
    ux::print_brand_header(writer)?;
    ux::print_intro(
        writer,
        "Quick setup: generating config with sensible defaults...\n",
    )?;

    if path.exists() {
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
    let providers = vec![
        "openai".to_string(),
        "openrouter".to_string(),
        "anthropic".to_string(),
    ];
    let provider_index = providers
        .iter()
        .position(|value| value == &defaults.provider)
        .unwrap_or(0);
    let provider = Select::new("Provider", providers)
        .with_starting_cursor(provider_index)
        .with_help_message("Type to filter options.")
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

    let model_selection = Select::new(
        "Model",
        model_options(&provider)
            .into_iter()
            .map(ToString::to_string)
            .collect(),
    )
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
        "[provider]\nname = {}\nbase_url = {}\nmodel = {}\n\n[memory]\npath = {}\n\n[agent]\nmode = \"development\"\nmax_tool_iterations = 4\nrequest_timeout_ms = 30000\nmemory_window_size = 8\nmax_prompt_chars = 8000\n\n[agent.hooks]\nenabled = false\ntimeout_ms = 250\nfail_closed = false\n\n[security]\nallowed_root = {}\nallowed_commands = [{}]\n\n[security.read_file]\nmax_read_bytes = 65536\nallow_binary = false\n\n[security.write_file]\nenabled = false\nmax_write_bytes = 65536\n\n[security.shell]\nmax_args = 8\nmax_arg_length = 128\nmax_output_bytes = 8192\nforbidden_chars = \";&|><$`\\n\\r\"\n\n[security.mcp]\nenabled = false\nallowed_servers = []\n\n[security.plugin]\nenabled = false\n\n[security.audit]\nenabled = false\npath = \"./agentzero-audit.log\"\n",
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
        _ => "https://api.openai.com",
    }
}

fn model_options(provider: &str) -> Vec<&'static str> {
    match provider {
        "openrouter" => vec![
            "anthropic/claude-3.5-sonnet",
            "openai/gpt-4o-mini",
            "custom",
        ],
        "anthropic" => vec![
            "claude-3-5-sonnet-latest",
            "claude-3-5-haiku-latest",
            "custom",
        ],
        _ => vec!["gpt-4o-mini", "gpt-4.1-mini", "custom"],
    }
}

fn model_start_cursor(provider: &str, current_model: &str) -> usize {
    let options = model_options(provider);
    options
        .iter()
        .position(|value| *value == current_model)
        .or_else(|| options.iter().position(|value| *value == "custom"))
        .unwrap_or(0)
}

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

fn memory_path_start_cursor(current_path: &str) -> usize {
    let options = memory_path_options(current_path);
    options
        .iter()
        .position(|value| value == current_path)
        .unwrap_or(0)
}

fn allowed_root_options(current_root: &str) -> Vec<String> {
    unique_options(vec![
        ".".to_string(),
        "./workspace".to_string(),
        current_root.to_string(),
        "custom".to_string(),
    ])
}

fn allowed_root_start_cursor(current_root: &str) -> usize {
    let options = allowed_root_options(current_root);
    options
        .iter()
        .position(|value| value == current_root)
        .unwrap_or(0)
}

fn allowed_commands_options(current_commands: &str) -> Vec<String> {
    unique_options(vec![
        "ls,pwd,cat,echo".to_string(),
        "ls,pwd,cat".to_string(),
        "ls,pwd".to_string(),
        current_commands.to_string(),
        "custom".to_string(),
    ])
}

fn allowed_commands_start_cursor(current_commands: &str) -> usize {
    let options = allowed_commands_options(current_commands);
    options
        .iter()
        .position(|value| value == current_commands)
        .unwrap_or(0)
}

fn unique_options(options: Vec<String>) -> Vec<String> {
    let mut unique = Vec::new();
    for option in options {
        if !option.trim().is_empty() && !unique.contains(&option) {
            unique.push(option);
        }
    }
    unique
}

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

#[cfg(test)]
mod tests {
    use super::{
        allowed_commands_options, base_url_options, resolve_onboard_config, run_with_io,
        OnboardConfig, OnboardOptions,
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
        let dir = std::env::temp_dir().join(format!("agentzero-onboard-{nanos}-{seq}"));
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

        fs::remove_dir_all(dir).expect("temp dir should be removed");
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
            OnboardConfig::default(),
        )
        .expect("declining overwrite should not error");

        let content = fs::read_to_string(&config_path).expect("existing file should still exist");
        assert_eq!(content, "sentinel = true\n");

        let stdout = String::from_utf8(output).expect("output should be utf8");
        assert!(stdout.contains("Config already exists"));
        assert!(stdout.contains("Aborted. Existing config was left unchanged."));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolves_onboard_config_with_env_values() {
        let opts = OnboardOptions {
            yes: false,
            provider: None,
            base_url: None,
            model: None,
            memory_path: None,
            allowed_root: None,
            allowed_commands: vec![],
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
            yes: false,
            provider: Some("anthropic".to_string()),
            base_url: Some("https://example.invalid".to_string()),
            model: Some("claude-3-5-haiku-latest".to_string()),
            memory_path: Some("./flag.db".to_string()),
            allowed_root: Some("./flag-root".to_string()),
            allowed_commands: vec!["cat".to_string(), "echo".to_string()],
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
}
