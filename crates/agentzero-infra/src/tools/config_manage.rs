//! LLM-callable tool for reading and modifying agentzero configuration.
//!
//! Placed in `agentzero-infra` (not `agentzero-tools`) to avoid a circular
//! dependency: this module needs `agentzero-config::writer`, and
//! `agentzero-config` already depends on `agentzero-tools`.

use agentzero_config::writer;
use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use anyhow::{bail, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Path, PathBuf};

const MAX_BACKUPS: usize = 10;

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct Input {
    /// The config operation to perform
    #[schema(enum_values = ["get", "set", "validate", "diff", "backup_list", "rollback"])]
    action: String,
    /// Config section name (e.g. 'provider', 'security', 'agents'). Omit for full config on get.
    #[serde(default)]
    section: Option<String>,
    /// For set/validate/diff: the JSON value to merge into the section
    #[serde(default)]
    value: Option<serde_json::Value>,
    /// For rollback: the backup timestamp to restore
    #[serde(default)]
    backup_id: Option<String>,
}

#[tool(
    name = "config_manage",
    description = "Read and modify the agentzero configuration. Actions: get (read config section), set (update config with validation + backup), validate (dry-run a change), diff (preview what would change), backup_list (list backups), rollback (restore a backup)."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct ConfigManageTool;

impl ConfigManageTool {
    fn resolve_config_path(ctx: &ToolContext) -> anyhow::Result<PathBuf> {
        if let Some(ref p) = ctx.config_path {
            return Ok(PathBuf::from(p));
        }
        let workspace = PathBuf::from(&ctx.workspace_root);
        let candidate = workspace.join("agentzero.toml");
        if candidate.exists() {
            return Ok(candidate);
        }
        bail!("config_path not set and no agentzero.toml found in workspace root")
    }
}

#[async_trait]
impl Tool for ConfigManageTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(Input::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: Input = serde_json::from_str(input).context("invalid config_manage input")?;

        if ctx.depth > 0 {
            bail!("config_manage is not available to sub-agents (depth > 0)");
        }

        let config_path = Self::resolve_config_path(ctx)?;

        match req.action.as_str() {
            "get" => action_get(&config_path, req.section.as_deref()),
            "set" => action_set(&config_path, req.section.as_deref(), req.value),
            "validate" => action_validate(&config_path, req.section.as_deref(), req.value),
            "diff" => action_diff(&config_path, req.section.as_deref(), req.value),
            "backup_list" => action_backup_list(&config_path),
            "rollback" => action_rollback(&config_path, req.backup_id),
            other => bail!("unknown config_manage action: {other}"),
        }
    }
}

fn action_get(config_path: &Path, section: Option<&str>) -> anyhow::Result<ToolResult> {
    let value = writer::read_section(config_path, section)?;
    let output = serde_json::to_string_pretty(&value).context("failed to format config")?;
    Ok(ToolResult { output })
}

fn build_sections(
    section: Option<&str>,
    value: Option<serde_json::Value>,
) -> anyhow::Result<Vec<writer::ConfigSection>> {
    let value = value.context("'value' is required for this action")?;
    let key = section.context("'section' is required for this action")?;
    Ok(vec![writer::ConfigSection {
        key: key.to_string(),
        value,
    }])
}

fn action_set(
    config_path: &Path,
    section: Option<&str>,
    value: Option<serde_json::Value>,
) -> anyhow::Result<ToolResult> {
    let sections = build_sections(section, value)?;
    let (merged_str, _) = writer::read_and_merge(config_path, &sections)?;
    let backup = writer::write_with_backup(config_path, &merged_str, MAX_BACKUPS)?;

    let mut msg = format!("Config section '{}' updated successfully.", sections[0].key);
    if let Some(backup_path) = backup {
        msg.push_str(&format!(" Backup saved to: {}", backup_path.display()));
    }
    msg.push_str(" The config watcher will hot-reload the changes.");

    tracing::info!(section = %sections[0].key, "config updated via config_manage tool");
    Ok(ToolResult { output: msg })
}

fn action_validate(
    config_path: &Path,
    section: Option<&str>,
    value: Option<serde_json::Value>,
) -> anyhow::Result<ToolResult> {
    let sections = build_sections(section, value)?;
    match writer::read_and_merge(config_path, &sections) {
        Ok(_) => Ok(ToolResult {
            output: format!(
                "Validation passed: section '{}' would be valid.",
                sections[0].key
            ),
        }),
        Err(e) => Ok(ToolResult {
            output: format!("Validation failed: {e}"),
        }),
    }
}

fn action_diff(
    config_path: &Path,
    section: Option<&str>,
    value: Option<serde_json::Value>,
) -> anyhow::Result<ToolResult> {
    let sections = build_sections(section, value)?;
    let diff = writer::diff_sections(config_path, &sections)?;
    Ok(ToolResult { output: diff })
}

fn action_backup_list(config_path: &Path) -> anyhow::Result<ToolResult> {
    let backups = writer::list_backups(config_path)?;
    if backups.is_empty() {
        return Ok(ToolResult {
            output: "No config backups found.".to_string(),
        });
    }
    let mut output = format!("Found {} backup(s):\n", backups.len());
    for b in &backups {
        output.push_str(&format!("  - {} ({})\n", b.timestamp, b.path.display()));
    }
    Ok(ToolResult { output })
}

fn action_rollback(config_path: &Path, backup_id: Option<String>) -> anyhow::Result<ToolResult> {
    let backup_id = backup_id.context("'backup_id' is required for rollback")?;
    let backups = writer::list_backups(config_path)?;
    let backup = backups
        .iter()
        .find(|b| b.timestamp == backup_id)
        .with_context(|| format!("backup '{backup_id}' not found"))?;

    writer::rollback(config_path, &backup.path)?;
    tracing::info!(backup_id = %backup_id, "config rolled back via config_manage tool");

    Ok(ToolResult {
        output: format!(
            "Config restored from backup '{backup_id}'. The config watcher will hot-reload."
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-cfgmgr-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn make_ctx(dir: &std::path::Path) -> ToolContext {
        let config_path = dir.join("agentzero.toml");
        ToolContext {
            config_path: Some(config_path.to_string_lossy().to_string()),
            ..ToolContext::new(dir.to_string_lossy().to_string())
        }
    }

    #[tokio::test]
    async fn get_returns_config() {
        let dir = temp_dir();
        fs::write(dir.join("agentzero.toml"), "").expect("write");
        let ctx = make_ctx(&dir);
        let tool = ConfigManageTool;

        let result = tool
            .execute(r#"{"action": "get"}"#, &ctx)
            .await
            .expect("get should succeed");
        assert!(!result.output.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn set_creates_backup() {
        let dir = temp_dir();
        fs::write(dir.join("agentzero.toml"), "").expect("write");
        let ctx = make_ctx(&dir);
        let tool = ConfigManageTool;

        let input = serde_json::json!({
            "action": "set",
            "section": "identity",
            "value": { "agent_name": "test-bot", "version": "0.1.0" }
        });
        let result = tool
            .execute(&serde_json::to_string(&input).expect("json"), &ctx)
            .await
            .expect("set should succeed");
        assert!(result.output.contains("updated successfully"));

        let content = fs::read_to_string(dir.join("agentzero.toml")).expect("read");
        assert!(content.contains("test-bot"));
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn depth_blocks_sub_agents() {
        let dir = temp_dir();
        fs::write(dir.join("agentzero.toml"), "").expect("write");
        let mut ctx = make_ctx(&dir);
        ctx.depth = 1;
        let tool = ConfigManageTool;

        let err = tool
            .execute(r#"{"action": "get"}"#, &ctx)
            .await
            .expect_err("sub-agent should be blocked");
        assert!(err.to_string().contains("not available to sub-agents"));
        let _ = fs::remove_dir_all(dir);
    }
}
