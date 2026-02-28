use crate::model::AgentZeroConfig;
use anyhow::{anyhow, Context};
use config::{Config, Environment, File};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn load(path: &Path) -> anyhow::Result<AgentZeroConfig> {
    let dotenv_overrides = load_dotenv_chain(path)?;
    let settings = Config::builder()
        .add_source(File::from(path.to_path_buf()).required(false))
        .add_source(
            Environment::with_prefix("AGENTZERO")
                .separator("__")
                .list_separator(",")
                .try_parsing(true),
        )
        .build()
        .context("failed to build layered config")?;

    let parsed: AgentZeroConfig = settings
        .try_deserialize()
        .context("failed to deserialize config into typed model")?;
    let config = apply_dotenv_overrides(parsed, &dotenv_overrides)?;
    let config = apply_legacy_env_overrides(config)?;
    config.validate()?;
    Ok(config)
}

pub fn load_env_var(path: &Path, key: &str) -> anyhow::Result<Option<String>> {
    if let Ok(value) = std::env::var(key) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }

    let dotenv_overrides = load_dotenv_chain(path)?;
    Ok(first_nonempty_value(&dotenv_overrides, &[key]))
}

fn load_dotenv_chain(config_path: &Path) -> anyhow::Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    let root = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    for file in [root.join(".env"), root.join(".env.local")] {
        if !file.exists() {
            continue;
        }
        for entry in dotenvy::from_path_iter(&file)
            .with_context(|| format!("failed to read dotenv file at {}", file.display()))?
        {
            let (key, value) = entry
                .with_context(|| format!("failed to parse dotenv entry in {}", file.display()))?;
            out.insert(key, value);
        }
    }

    if let Some(env) = first_nonempty_env(&["AGENTZERO_ENV", "APP_ENV", "NODE_ENV"])
        .or_else(|| first_nonempty_value(&out, &["AGENTZERO_ENV", "APP_ENV", "NODE_ENV"]))
    {
        let file = root.join(format!(".env.{env}"));
        if file.exists() {
            for entry in dotenvy::from_path_iter(&file)
                .with_context(|| format!("failed to read dotenv file at {}", file.display()))?
            {
                let (key, value) = entry.with_context(|| {
                    format!("failed to parse dotenv entry in {}", file.display())
                })?;
                out.insert(key, value);
            }
        }
    }

    Ok(out)
}

fn apply_dotenv_overrides(
    mut config: AgentZeroConfig,
    dotenv_overrides: &HashMap<String, String>,
) -> anyhow::Result<AgentZeroConfig> {
    if let Some(value) = first_nonempty_value(dotenv_overrides, &["AGENTZERO_PROVIDER__KIND"]) {
        config.provider.kind = value;
    }
    if let Some(value) = first_nonempty_value(dotenv_overrides, &["AGENTZERO_PROVIDER__BASE_URL"]) {
        config.provider.base_url = value;
    }
    if let Some(value) = first_nonempty_value(dotenv_overrides, &["AGENTZERO_PROVIDER__MODEL"]) {
        config.provider.model = value;
    }
    if let Some(value) = first_nonempty_value(dotenv_overrides, &["AGENTZERO_MEMORY__BACKEND"]) {
        config.memory.backend = value;
    }
    if let Some(value) = first_nonempty_value(dotenv_overrides, &["AGENTZERO_MEMORY__SQLITE_PATH"])
    {
        config.memory.sqlite_path = value;
    }
    if let Some(value) =
        first_nonempty_value(dotenv_overrides, &["AGENTZERO_AGENT__MEMORY_WINDOW_SIZE"])
    {
        config.agent.memory_window_size = value.parse().with_context(|| {
            "AGENTZERO_AGENT__MEMORY_WINDOW_SIZE must be a positive integer".to_string()
        })?;
    }
    if let Some(value) =
        first_nonempty_value(dotenv_overrides, &["AGENTZERO_AGENT__MAX_PROMPT_CHARS"])
    {
        config.agent.max_prompt_chars = value.parse().with_context(|| {
            "AGENTZERO_AGENT__MAX_PROMPT_CHARS must be a positive integer".to_string()
        })?;
    }
    if let Some(value) =
        first_nonempty_value(dotenv_overrides, &["AGENTZERO_SECURITY__ALLOWED_ROOT"])
    {
        config.security.allowed_root = value;
    }
    if let Some(value) =
        first_nonempty_value(dotenv_overrides, &["AGENTZERO_SECURITY__ALLOWED_COMMANDS"])
    {
        let commands = value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if !commands.is_empty() {
            config.security.allowed_commands = commands;
        } else {
            return Err(anyhow!(
                "AGENTZERO_SECURITY__ALLOWED_COMMANDS must contain at least one command when set"
            ));
        }
    }

    Ok(config)
}

fn apply_legacy_env_overrides(mut config: AgentZeroConfig) -> anyhow::Result<AgentZeroConfig> {
    if let Some(value) = first_nonempty_env(&["AGENTZERO_PROVIDER", "AGENTZERO_PROVIDER__KIND"]) {
        config.provider.kind = value;
    }
    if let Some(value) = first_nonempty_env(&["AGENTZERO_BASE_URL", "AGENTZERO_PROVIDER__BASE_URL"])
    {
        config.provider.base_url = value;
    }
    if let Some(value) = first_nonempty_env(&["AGENTZERO_MODEL", "AGENTZERO_PROVIDER__MODEL"]) {
        config.provider.model = value;
    }
    if let Some(value) =
        first_nonempty_env(&["AGENTZERO_MEMORY_BACKEND", "AGENTZERO_MEMORY__BACKEND"])
    {
        config.memory.backend = value;
    }
    if let Some(value) =
        first_nonempty_env(&["AGENTZERO_MEMORY_PATH", "AGENTZERO_MEMORY__SQLITE_PATH"])
    {
        config.memory.sqlite_path = value;
    }
    if let Some(value) = first_nonempty_env(&["AGENTZERO_AGENT__MEMORY_WINDOW_SIZE"]) {
        config.agent.memory_window_size = value.parse().with_context(|| {
            "AGENTZERO_AGENT__MEMORY_WINDOW_SIZE must be a positive integer".to_string()
        })?;
    }
    if let Some(value) = first_nonempty_env(&["AGENTZERO_AGENT__MAX_PROMPT_CHARS"]) {
        config.agent.max_prompt_chars = value.parse().with_context(|| {
            "AGENTZERO_AGENT__MAX_PROMPT_CHARS must be a positive integer".to_string()
        })?;
    }
    if let Some(value) =
        first_nonempty_env(&["AGENTZERO_ALLOWED_ROOT", "AGENTZERO_SECURITY__ALLOWED_ROOT"])
    {
        config.security.allowed_root = value;
    }
    if let Some(value) = first_nonempty_env(&[
        "AGENTZERO_ALLOWED_COMMANDS",
        "AGENTZERO_SECURITY__ALLOWED_COMMANDS",
    ]) {
        let commands = value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if !commands.is_empty() {
            config.security.allowed_commands = commands;
        } else {
            return Err(anyhow!(
                "AGENTZERO_ALLOWED_COMMANDS must contain at least one command when set"
            ));
        }
    }

    Ok(config)
}

fn first_nonempty_env(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key).ok().and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    })
}

fn first_nonempty_value(values: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        values.get(*key).and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    })
}
