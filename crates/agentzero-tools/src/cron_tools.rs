use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_cron::CronStore;
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;

fn cron_data_dir(workspace_root: &str) -> PathBuf {
    PathBuf::from(workspace_root).join(".agentzero")
}

// ── cron_add ──

#[derive(Debug, Deserialize)]
struct CronAddInput {
    id: String,
    schedule: String,
    command: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CronAddTool;

#[async_trait]
impl Tool for CronAddTool {
    fn name(&self) -> &'static str {
        "cron_add"
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

#[derive(Debug, Default, Clone, Copy)]
pub struct CronListTool;

#[async_trait]
impl Tool for CronListTool {
    fn name(&self) -> &'static str {
        "cron_list"
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

#[derive(Debug, Deserialize)]
struct CronRemoveInput {
    id: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CronRemoveTool;

#[async_trait]
impl Tool for CronRemoveTool {
    fn name(&self) -> &'static str {
        "cron_remove"
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

#[derive(Debug, Deserialize)]
struct CronUpdateInput {
    id: String,
    #[serde(default)]
    schedule: Option<String>,
    #[serde(default)]
    command: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CronUpdateTool;

#[async_trait]
impl Tool for CronUpdateTool {
    fn name(&self) -> &'static str {
        "cron_update"
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

#[derive(Debug, Deserialize)]
struct CronPauseInput {
    id: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CronPauseTool;

#[async_trait]
impl Tool for CronPauseTool {
    fn name(&self) -> &'static str {
        "cron_pause"
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

#[derive(Debug, Deserialize)]
struct CronResumeInput {
    id: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CronResumeTool;

#[async_trait]
impl Tool for CronResumeTool {
    fn name(&self) -> &'static str {
        "cron_resume"
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
