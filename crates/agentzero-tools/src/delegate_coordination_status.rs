use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::Context;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use tokio::fs;

const COORDINATION_FILE: &str = ".agentzero/coordination.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DelegationRecord {
    agent_name: String,
    status: String,
    prompt_summary: String,
    #[serde(default)]
    iterations_used: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CoordinationStore {
    delegations: Vec<DelegationRecord>,
}

impl CoordinationStore {
    async fn load(workspace_root: &str) -> anyhow::Result<Self> {
        let path = Path::new(workspace_root).join(COORDINATION_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(&path)
            .await
            .context("failed to read coordination store")?;
        serde_json::from_str(&data).context("failed to parse coordination store")
    }

    async fn save(&self, workspace_root: &str) -> anyhow::Result<()> {
        let path = Path::new(workspace_root).join(COORDINATION_FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("failed to create .agentzero directory")?;
        }
        let data =
            serde_json::to_string_pretty(self).context("failed to serialize coordination store")?;
        fs::write(&path, data)
            .await
            .context("failed to write coordination store")
    }
}

#[derive(Debug, Deserialize)]
struct Input {
    op: String,
    #[serde(default)]
    agent_name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    prompt_summary: Option<String>,
    #[serde(default)]
    iterations_used: Option<usize>,
}

/// Query and update delegation coordination state across sub-agents.
///
/// Operations:
/// - `list`: List all delegation records
/// - `record`: Record a delegation event
/// - `clear`: Clear all delegation records
#[derive(Debug, Default, Clone, Copy)]
pub struct DelegateCoordinationStatusTool;

#[async_trait]
impl Tool for DelegateCoordinationStatusTool {
    fn name(&self) -> &'static str {
        "delegate_coordination_status"
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: Input = serde_json::from_str(input)
            .context("delegate_coordination_status expects JSON: {\"op\", ...}")?;

        match parsed.op.as_str() {
            "list" => {
                let store = CoordinationStore::load(&ctx.workspace_root).await?;
                if store.delegations.is_empty() {
                    return Ok(ToolResult {
                        output: "no delegation records".to_string(),
                    });
                }
                let records: Vec<serde_json::Value> = store
                    .delegations
                    .iter()
                    .map(|d| {
                        json!({
                            "agent_name": d.agent_name,
                            "status": d.status,
                            "prompt_summary": d.prompt_summary,
                            "iterations_used": d.iterations_used,
                        })
                    })
                    .collect();
                Ok(ToolResult {
                    output: serde_json::to_string_pretty(&records)
                        .unwrap_or_else(|_| "[]".to_string()),
                })
            }
            "record" => {
                let agent_name = parsed
                    .agent_name
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("record requires `agent_name`"))?;
                let status = parsed
                    .status
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("record requires `status`"))?;
                let prompt_summary = parsed
                    .prompt_summary
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("record requires `prompt_summary`"))?;

                if agent_name.trim().is_empty() {
                    return Err(anyhow::anyhow!("agent_name must not be empty"));
                }

                let mut store = CoordinationStore::load(&ctx.workspace_root).await?;
                store.delegations.push(DelegationRecord {
                    agent_name: agent_name.to_string(),
                    status: status.to_string(),
                    prompt_summary: prompt_summary.to_string(),
                    iterations_used: parsed.iterations_used.unwrap_or(0),
                });
                store.save(&ctx.workspace_root).await?;

                Ok(ToolResult {
                    output: format!("recorded delegation: agent={agent_name} status={status}"),
                })
            }
            "clear" => {
                let store = CoordinationStore::default();
                store.save(&ctx.workspace_root).await?;
                Ok(ToolResult {
                    output: "cleared all delegation records".to_string(),
                })
            }
            other => Ok(ToolResult {
                output: json!({ "error": format!("unknown op: {other}") }).to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolContext;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-coord-tools-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn list_empty_returns_no_records() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let result = DelegateCoordinationStatusTool
            .execute(r#"{"op": "list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("no delegation records"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn record_and_list_roundtrip() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        DelegateCoordinationStatusTool
            .execute(
                r#"{"op": "record", "agent_name": "researcher", "status": "completed", "prompt_summary": "Find docs", "iterations_used": 3}"#,
                &ctx,
            )
            .await
            .expect("record should succeed");

        let result = DelegateCoordinationStatusTool
            .execute(r#"{"op": "list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("researcher"));
        assert!(result.output.contains("completed"));
        assert!(result.output.contains("Find docs"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn clear_removes_all_records() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        DelegateCoordinationStatusTool
            .execute(
                r#"{"op": "record", "agent_name": "a1", "status": "done", "prompt_summary": "task"}"#,
                &ctx,
            )
            .await
            .unwrap();

        DelegateCoordinationStatusTool
            .execute(r#"{"op": "clear"}"#, &ctx)
            .await
            .expect("clear should succeed");

        let result = DelegateCoordinationStatusTool
            .execute(r#"{"op": "list"}"#, &ctx)
            .await
            .unwrap();
        assert!(result.output.contains("no delegation records"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn record_empty_agent_name_fails() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let err = DelegateCoordinationStatusTool
            .execute(
                r#"{"op": "record", "agent_name": "", "status": "done", "prompt_summary": "x"}"#,
                &ctx,
            )
            .await
            .expect_err("empty agent_name should fail");
        assert!(err.to_string().contains("agent_name must not be empty"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn record_missing_fields_fails() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let err = DelegateCoordinationStatusTool
            .execute(r#"{"op": "record", "agent_name": "a1"}"#, &ctx)
            .await
            .expect_err("missing status should fail");
        assert!(err.to_string().contains("requires `status`"));

        fs::remove_dir_all(dir).ok();
    }
}
