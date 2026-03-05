use crate::cli::ConfigCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use async_trait::async_trait;
use serde_json::Value;

pub struct ConfigCommand;

#[async_trait]
impl AgentZeroCommand for ConfigCommand {
    type Options = ConfigCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            ConfigCommands::Schema { json } => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&json_schema())?);
                } else {
                    println!("{}", toml_schema_template());
                }
            }
            ConfigCommands::Show { raw } => {
                let cfg = agentzero_config::load(&ctx.config_path)?;
                let mut json = serde_json::to_value(&cfg)?;
                if !raw {
                    mask_secrets(&mut json);
                }
                println!("{}", serde_json::to_string_pretty(&json)?);
            }
            ConfigCommands::Get { key } => {
                let cfg = agentzero_config::load(&ctx.config_path)?;
                let json = serde_json::to_value(&cfg)?;
                match resolve_dot_path(&json, &key) {
                    Some(value) => {
                        let formatted = match value {
                            Value::String(s) => s.to_string(),
                            other => serde_json::to_string_pretty(other)?,
                        };
                        println!("{formatted}");
                    }
                    None => {
                        anyhow::bail!("config key `{key}` not found");
                    }
                }
            }
            ConfigCommands::Set { key, value } => {
                set_config_value(&ctx.config_path, &key, &value)?;
                println!("set {key} = {value}");
            }
        }
        Ok(())
    }
}

/// Resolve a dot-separated path against a JSON value tree.
fn resolve_dot_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = root;
    for segment in path.split('.') {
        match current {
            Value::Object(map) => {
                current = map.get(segment)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

/// Mask values whose keys look like secrets.
fn mask_secrets(value: &mut Value) {
    const SECRET_KEYS: &[&str] = &[
        "api_key",
        "api_keys",
        "token",
        "auth_token",
        "clawhub_token",
        "brave_api_key",
        "perplexity_api_key",
        "exa_api_key",
        "jina_api_key",
    ];

    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if SECRET_KEYS.contains(&k.as_str()) {
                    if let Value::String(s) = v {
                        if !s.is_empty() {
                            *v = Value::String("***".to_string());
                        }
                    }
                } else {
                    mask_secrets(v);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                mask_secrets(item);
            }
        }
        _ => {}
    }
}

/// Set a config value by editing the TOML file on disk.
fn set_config_value(
    config_path: &std::path::Path,
    dot_key: &str,
    raw_value: &str,
) -> anyhow::Result<()> {
    let content = if config_path.exists() {
        std::fs::read_to_string(config_path)?
    } else {
        String::new()
    };

    let mut doc: toml::Value = if content.trim().is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        content.parse::<toml::Value>()?
    };

    let segments: Vec<&str> = dot_key.split('.').collect();
    if segments.is_empty() {
        anyhow::bail!("config key must not be empty");
    }

    // Navigate/create intermediate tables.
    let mut current = &mut doc;
    for segment in &segments[..segments.len() - 1] {
        current = current
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("path segment `{segment}` is not a table"))?
            .entry(segment.to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    }

    // segments is non-empty: validated by the `segments.is_empty()` check above
    let leaf = segments.last().expect("segments must be non-empty");
    let typed_value = infer_toml_value(raw_value);

    current
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("parent of `{leaf}` is not a table"))?
        .insert(leaf.to_string(), typed_value);

    let serialized = toml::to_string_pretty(&doc)?;
    std::fs::write(config_path, serialized)?;
    Ok(())
}

/// Infer a TOML value from a raw string: bool, integer, float, or string.
fn infer_toml_value(raw: &str) -> toml::Value {
    match raw {
        "true" => toml::Value::Boolean(true),
        "false" => toml::Value::Boolean(false),
        _ => {
            if let Ok(i) = raw.parse::<i64>() {
                toml::Value::Integer(i)
            } else if let Ok(f) = raw.parse::<f64>() {
                toml::Value::Float(f)
            } else {
                toml::Value::String(raw.to_string())
            }
        }
    }
}

fn toml_schema_template() -> &'static str {
    r#"# agentzero.toml schema template

[provider]
kind = "openrouter"
base_url = "https://openrouter.ai/api/v1"
model = "anthropic/claude-sonnet-4-6"
default_temperature = 0.7

[memory]
backend = "sqlite"
sqlite_path = "./agentzero.db"

[agent]
max_tool_iterations = 20
memory_window_size = 50

[security]
allowed_root = "."
allowed_commands = ["ls", "pwd", "cat", "echo"]

[audit]
enabled = true
path = "./audit.log"
"#
}

fn json_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["provider", "memory", "security"],
        "properties": {
            "provider": {
                "type": "object",
                "required": ["kind", "base_url", "model"]
            },
            "memory": {
                "type": "object",
                "required": ["backend", "sqlite_path"]
            },
            "security": {
                "type": "object",
                "required": ["allowed_root", "allowed_commands"]
            },
            "audit": {
                "type": "object",
                "required": ["enabled", "path"]
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ConfigCommands;
    use crate::command_core::{AgentZeroCommand, CommandContext};

    #[tokio::test]
    async fn config_schema_command_success_path() {
        let ctx = CommandContext {
            workspace_root: std::env::temp_dir(),
            data_dir: std::env::temp_dir(),
            config_path: std::env::temp_dir().join("agentzero.toml"),
        };
        ConfigCommand::run(&ctx, ConfigCommands::Schema { json: false })
            .await
            .expect("schema command should succeed");
    }

    #[test]
    fn toml_schema_template_contains_provider_section_negative_path() {
        let schema = toml_schema_template();
        assert!(schema.contains("[provider]"));
    }

    #[test]
    fn resolve_dot_path_traverses_nested_objects() {
        let json: Value = serde_json::json!({
            "provider": {
                "kind": "openrouter",
                "model": "test-model"
            },
            "agent": {
                "max_tool_iterations": 20
            }
        });
        assert_eq!(
            resolve_dot_path(&json, "provider.model"),
            Some(&Value::String("test-model".to_string()))
        );
        assert_eq!(
            resolve_dot_path(&json, "agent.max_tool_iterations"),
            Some(&Value::Number(20.into()))
        );
        assert_eq!(resolve_dot_path(&json, "missing.key"), None);
    }

    #[test]
    fn mask_secrets_redacts_api_key_fields() {
        let mut json: Value = serde_json::json!({
            "provider": {
                "kind": "openrouter",
                "api_key": "sk-secret-123"
            },
            "web_search": {
                "brave_api_key": "brv-key",
                "enabled": true
            }
        });
        mask_secrets(&mut json);
        assert_eq!(
            json["provider"]["api_key"],
            Value::String("***".to_string())
        );
        assert_eq!(
            json["web_search"]["brave_api_key"],
            Value::String("***".to_string())
        );
        assert_eq!(json["web_search"]["enabled"], Value::Bool(true));
    }

    #[test]
    fn infer_toml_value_detects_types() {
        assert_eq!(infer_toml_value("true"), toml::Value::Boolean(true));
        assert_eq!(infer_toml_value("false"), toml::Value::Boolean(false));
        assert_eq!(infer_toml_value("42"), toml::Value::Integer(42));
        assert_eq!(infer_toml_value("0.7"), toml::Value::Float(0.7));
        assert_eq!(
            infer_toml_value("hello"),
            toml::Value::String("hello".to_string())
        );
    }

    #[test]
    fn set_config_value_creates_nested_keys() {
        let dir = std::env::temp_dir().join("agentzero-config-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-set.toml");
        let _ = std::fs::remove_file(&path);

        set_config_value(&path, "provider.model", "gpt-4o").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: toml::Value = content.parse().unwrap();
        assert_eq!(
            parsed["provider"]["model"],
            toml::Value::String("gpt-4o".to_string())
        );

        let _ = std::fs::remove_file(&path);
    }
}
