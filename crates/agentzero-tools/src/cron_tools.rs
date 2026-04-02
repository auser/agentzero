use crate::cron_store::CronStore;
use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;

fn cron_data_dir(workspace_root: &str) -> PathBuf {
    PathBuf::from(workspace_root).join(".agentzero")
}

// ── cron_add ──

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct CronAddInput {
    /// Unique task ID
    id: String,
    /// Cron expression (e.g. '0 * * * *')
    schedule: String,
    /// Command to execute
    command: String,
}

#[tool(
    name = "cron_add",
    description = "Add a new cron task with a schedule and command."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct CronAddTool;

#[async_trait]
impl Tool for CronAddTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(CronAddInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: CronAddInput = serde_json::from_str(input)
            .context("cron_add expects JSON: {\"id\", \"schedule\", \"command\"}")?;
        let store = CronStore::new(cron_data_dir(&ctx.workspace_root))?;
        let task = store.add(&req.id, &req.schedule, &req.command)?;
        Ok(ToolResult {
            output: format!(
                "added cron task: id={}, schedule={}, command={}, enabled={}",
                task.id, task.schedule, task.command, task.enabled
            ),
        })
    }
}

// ── cron_list ──

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct CronListInput {}

#[tool(name = "cron_list", description = "List all registered cron tasks.")]
#[derive(Debug, Default, Clone, Copy)]
pub struct CronListTool;

#[async_trait]
impl Tool for CronListTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(CronListInput::schema())
    }

    async fn execute(&self, _input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let store = CronStore::new(cron_data_dir(&ctx.workspace_root))?;
        let tasks = store.list()?;
        if tasks.is_empty() {
            return Ok(ToolResult {
                output: "no cron tasks".to_string(),
            });
        }
        let lines: Vec<String> = tasks
            .iter()
            .map(|t| {
                format!(
                    "id={} schedule={} command={} enabled={}",
                    t.id, t.schedule, t.command, t.enabled
                )
            })
            .collect();
        Ok(ToolResult {
            output: lines.join("\n"),
        })
    }
}

// ── cron_remove ──

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct CronRemoveInput {
    /// ID of the cron task to remove
    id: String,
}

#[tool(name = "cron_remove", description = "Remove a cron task by ID.")]
#[derive(Debug, Default, Clone, Copy)]
pub struct CronRemoveTool;

#[async_trait]
impl Tool for CronRemoveTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(CronRemoveInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: CronRemoveInput =
            serde_json::from_str(input).context("cron_remove expects JSON: {\"id\": \"...\"}")?;
        let store = CronStore::new(cron_data_dir(&ctx.workspace_root))?;
        store.remove(&req.id)?;
        Ok(ToolResult {
            output: format!("removed cron task: {}", req.id),
        })
    }
}

// ── cron_update ──

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct CronUpdateInput {
    /// ID of the cron task to update
    id: String,
    /// New cron schedule expression
    #[serde(default)]
    schedule: Option<String>,
    /// New command to execute
    #[serde(default)]
    command: Option<String>,
}

#[tool(
    name = "cron_update",
    description = "Update an existing cron task's schedule or command."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct CronUpdateTool;

#[async_trait]
impl Tool for CronUpdateTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(CronUpdateInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: CronUpdateInput =
            serde_json::from_str(input).context("cron_update expects JSON: {\"id\", ...}")?;
        if req.schedule.is_none() && req.command.is_none() {
            return Err(anyhow!(
                "at least one of schedule or command must be provided"
            ));
        }
        let store = CronStore::new(cron_data_dir(&ctx.workspace_root))?;
        let task = store.update(&req.id, req.schedule.as_deref(), req.command.as_deref())?;
        Ok(ToolResult {
            output: format!(
                "updated cron task: id={}, schedule={}, command={}",
                task.id, task.schedule, task.command
            ),
        })
    }
}

// ── cron_pause ──

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct CronPauseInput {
    /// ID of the cron task to pause
    id: String,
}

#[tool(
    name = "cron_pause",
    description = "Pause a cron task (disable without removing)."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct CronPauseTool;

#[async_trait]
impl Tool for CronPauseTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(CronPauseInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: CronPauseInput =
            serde_json::from_str(input).context("cron_pause expects JSON: {\"id\": \"...\"}")?;
        let store = CronStore::new(cron_data_dir(&ctx.workspace_root))?;
        let task = store.pause(&req.id)?;
        Ok(ToolResult {
            output: format!("paused cron task: id={}, enabled={}", task.id, task.enabled),
        })
    }
}

// ── cron_resume ──

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct CronResumeInput {
    /// ID of the cron task to resume
    id: String,
}

#[tool(name = "cron_resume", description = "Resume a paused cron task.")]
#[derive(Debug, Default, Clone, Copy)]
pub struct CronResumeTool;

#[async_trait]
impl Tool for CronResumeTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(CronResumeInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: CronResumeInput =
            serde_json::from_str(input).context("cron_resume expects JSON: {\"id\": \"...\"}")?;
        let store = CronStore::new(cron_data_dir(&ctx.workspace_root))?;
        let task = store.resume(&req.id)?;
        Ok(ToolResult {
            output: format!(
                "resumed cron task: id={}, enabled={}",
                task.id, task.enabled
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
            "agentzero-cron-tools-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn cron_add_list_remove_roundtrip() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let add = CronAddTool;
        let result = add
            .execute(
                r#"{"id": "backup", "schedule": "0 * * * *", "command": "echo hello"}"#,
                &ctx,
            )
            .await
            .expect("add should succeed");
        assert!(result.output.contains("backup"));

        let list = CronListTool;
        let result = list.execute("{}", &ctx).await.expect("list should succeed");
        assert!(result.output.contains("backup"));

        let remove = CronRemoveTool;
        let result = remove
            .execute(r#"{"id": "backup"}"#, &ctx)
            .await
            .expect("remove should succeed");
        assert!(result.output.contains("removed"));

        let result = list.execute("{}", &ctx).await.expect("list should succeed");
        assert!(result.output.contains("no cron tasks"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn cron_update_changes_schedule() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let add = CronAddTool;
        add.execute(
            r#"{"id": "test", "schedule": "0 * * * *", "command": "echo"}"#,
            &ctx,
        )
        .await
        .expect("add should succeed");

        let update = CronUpdateTool;
        let result = update
            .execute(r#"{"id": "test", "schedule": "*/5 * * * *"}"#, &ctx)
            .await
            .expect("update should succeed");
        assert!(result.output.contains("*/5 * * * *"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn cron_pause_resume() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let add = CronAddTool;
        add.execute(
            r#"{"id": "job", "schedule": "0 * * * *", "command": "test"}"#,
            &ctx,
        )
        .await
        .expect("add should succeed");

        let pause = CronPauseTool;
        let result = pause
            .execute(r#"{"id": "job"}"#, &ctx)
            .await
            .expect("pause should succeed");
        assert!(result.output.contains("enabled=false"));

        let resume = CronResumeTool;
        let result = resume
            .execute(r#"{"id": "job"}"#, &ctx)
            .await
            .expect("resume should succeed");
        assert!(result.output.contains("enabled=true"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn cron_remove_nonexistent_fails() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        let remove = CronRemoveTool;
        let err = remove
            .execute(r#"{"id": "nope"}"#, &ctx)
            .await
            .expect_err("removing nonexistent should fail");
        assert!(err.to_string().contains("not found"));
        fs::remove_dir_all(dir).ok();
    }
}
