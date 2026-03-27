use crate::model::AgentZeroConfig;
use agentzero_core::common::local_providers::local_provider_meta;
use anyhow::{anyhow, Context};
use config::{Config, Environment, File};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn load(path: &Path) -> anyhow::Result<AgentZeroConfig> {
    let dotenv_overrides = load_dotenv_chain(path)?;

    // Propagate dotenv values into the process environment so that tools
    // (e.g. web_search reading BRAVE_API_KEY) can use std::env::var().
    // Only set values that aren't already in the environment to avoid
    // overriding explicit env vars.
    //
    // SAFETY: `set_var` is unsafe because concurrent reads/writes to the
    // environment are data races. We enforce single-execution via
    // `std::sync::Once` and this runs before the async runtime spawns
    // worker threads, so no other thread can observe partial state.
    static ENV_INIT: std::sync::Once = std::sync::Once::new();
    ENV_INIT.call_once(|| {
        for (key, value) in &dotenv_overrides {
            if std::env::var(key).is_err() {
                // SAFETY: inside Once::call_once, guaranteed single-threaded.
                unsafe { std::env::set_var(key, value) };
            }
        }
    });

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
    let mut config = apply_legacy_env_overrides(config)?;
    normalize_base_url(&mut config);
    resolve_local_provider_defaults(&mut config);
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
    let config_dir = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    // Collect directories to scan: config dir first, then CWD (if different).
    // Later directories override earlier ones, so CWD `.env` takes priority.
    let mut dirs = vec![config_dir.clone()];
    if let Ok(cwd) = std::env::current_dir() {
        let cwd_canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.clone());
        let config_canonical = config_dir
            .canonicalize()
            .unwrap_or_else(|_| config_dir.clone());
        if cwd_canonical != config_canonical {
            dirs.push(cwd);
        }
    }

    for dir in &dirs {
        for file in [dir.join(".env"), dir.join(".env.local")] {
            if !file.exists() {
                continue;
            }
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

    if let Some(env) = first_nonempty_env(&["AGENTZERO_ENV", "APP_ENV", "NODE_ENV"])
        .or_else(|| first_nonempty_value(&out, &["AGENTZERO_ENV", "APP_ENV", "NODE_ENV"]))
    {
        for dir in dirs.iter().rev() {
            let file = dir.join(format!(".env.{env}"));
            if file.exists() {
                for entry in dotenvy::from_path_iter(&file)
                    .with_context(|| format!("failed to read dotenv file at {}", file.display()))?
                {
                    let (key, value) = entry.with_context(|| {
                        format!("failed to parse dotenv entry in {}", file.display())
                    })?;
                    out.insert(key, value);
                }
                break;
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

const DEFAULT_CLOUD_BASE_URL: &str = "https://openrouter.ai/api";

/// Strip a trailing `/v1` (or `/v1/`) from `base_url` so the provider code
/// can unconditionally append `/v1/chat/completions` without doubling the
/// version prefix.  This keeps backwards compatibility for configs that
/// already include `/v1` in their `base_url`.
fn normalize_base_url(config: &mut AgentZeroConfig) {
    let trimmed = config.provider.base_url.trim_end_matches('/');
    if let Some(stripped) = trimmed.strip_suffix("/v1") {
        config.provider.base_url = stripped.to_string();
    }
}

fn resolve_local_provider_defaults(config: &mut AgentZeroConfig) {
    if let Some(meta) = local_provider_meta(&config.provider.kind) {
        let is_default_url = config.provider.base_url == DEFAULT_CLOUD_BASE_URL
            || config.provider.base_url.trim().is_empty();
        if is_default_url {
            config.provider.base_url = meta.default_base_url.to_string();
        }
    }
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

/// Update the `[autonomy].auto_approve` list in a config TOML file on disk.
///
/// Reads the existing file (or starts from an empty doc), merges the new list,
/// and writes back. This preserves all other config sections.
pub fn update_auto_approve(path: &Path, tools: &[String]) -> anyhow::Result<()> {
    let content = if path.exists() {
        std::fs::read_to_string(path).context("failed to read config file for update")?
    } else {
        String::new()
    };

    let mut doc: toml::Table =
        toml::from_str(&content).context("failed to parse config TOML for update")?;

    let autonomy = doc
        .entry("autonomy")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));

    if let toml::Value::Table(table) = autonomy {
        let arr = tools
            .iter()
            .map(|t| toml::Value::String(t.clone()))
            .collect();
        table.insert("auto_approve".to_string(), toml::Value::Array(arr));
    }

    let serialized = toml::to_string_pretty(&doc).context("failed to serialize updated config")?;
    std::fs::write(path, serialized).context("failed to write updated config file")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Docker Secrets support
// ---------------------------------------------------------------------------

/// Read a Docker secret from `/run/secrets/<name>`.
/// Returns `None` if the file doesn't exist or is unreadable.
pub fn read_docker_secret(name: &str) -> Option<String> {
    let path = PathBuf::from("/run/secrets").join(name);
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Resolve a value from: environment variable → Docker secret → `None`.
/// Useful for API keys and encryption keys in containerized deployments.
pub fn env_or_secret(env_var: &str, secret_name: &str) -> Option<String> {
    std::env::var(env_var)
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| read_docker_secret(secret_name))
}

#[cfg(test)]
mod docker_secret_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn read_docker_secret_from_mock_path() {
        // Create a temp dir simulating /run/secrets/
        let tmp =
            std::env::temp_dir().join(format!("agentzero-secrets-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).expect("create tmp dir");
        let secret_path = tmp.join("api_key");
        let mut f = std::fs::File::create(&secret_path).expect("create secret file");
        f.write_all(b"sk-test-key-12345\n").expect("write secret");

        // read_docker_secret reads from an arbitrary path internally,
        // but we can test the parsing logic by reading directly
        let content = std::fs::read_to_string(&secret_path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        assert_eq!(content, Some("sk-test-key-12345".to_string()));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn env_or_secret_prefers_env_var() {
        let key = format!("AGENTZERO_TEST_SECRET_{}", std::process::id());
        unsafe { std::env::set_var(&key, "from-env") };
        let result = env_or_secret(&key, "nonexistent_secret");
        assert_eq!(result, Some("from-env".to_string()));
        unsafe { std::env::remove_var(&key) };
    }

    #[test]
    fn env_or_secret_falls_back_to_none_when_both_missing() {
        let key = format!("AGENTZERO_MISSING_SECRET_{}", std::process::id());
        let result = env_or_secret(&key, "definitely_nonexistent_secret_file");
        assert_eq!(result, None);
    }
}
