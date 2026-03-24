use crate::skills::sop::{self, SopPlan};
#[cfg(feature = "tools-extended")]
use crate::sop::engine::SopEngine;
#[cfg(feature = "tools-extended")]
use crate::sop::types::SopStepKind;
use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;

const SOP_FILE: &str = ".agentzero/sops.json";

/// Persistent store for SOP plans, keyed by plan ID.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SopStore {
    plans: HashMap<String, SopPlan>,
    /// Steps that require approval before they can be advanced.
    /// Key: "plan_id:step_index", Value: true if approved.
    #[serde(default)]
    approvals: HashMap<String, bool>,
}

impl SopStore {
    async fn load(workspace_root: &str) -> anyhow::Result<Self> {
        let path = Path::new(workspace_root).join(SOP_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(&path)
            .await
            .context("failed to read sop store")?;
        serde_json::from_str(&data).context("failed to parse sop store")
    }

    async fn save(&self, workspace_root: &str) -> anyhow::Result<()> {
        let path = Path::new(workspace_root).join(SOP_FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("failed to create .agentzero directory")?;
        }
        let data = serde_json::to_string_pretty(self).context("failed to serialize sop store")?;
        fs::write(&path, data)
            .await
            .context("failed to write sop store")
    }

    fn approval_key(plan_id: &str, step_index: usize) -> String {
        format!("{plan_id}:{step_index}")
    }
}

// --- sop_list ---

#[derive(Debug, Default, Clone, Copy)]
pub struct SopListTool;

#[async_trait]
impl Tool for SopListTool {
    fn name(&self) -> &'static str {
        "sop_list"
    }

    fn description(&self) -> &'static str {
        "List all standard operating procedures (SOPs) in the workspace."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }))
    }

    async fn execute(&self, _input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let store = SopStore::load(&ctx.workspace_root).await?;

        if store.plans.is_empty() {
            return Ok(ToolResult {
                output: "no SOPs found".to_string(),
            });
        }

        let mut lines: Vec<String> = Vec::new();
        let mut ids: Vec<&String> = store.plans.keys().collect();
        ids.sort();

        for id in ids {
            let plan = &store.plans[id];
            let completed = plan.steps.iter().filter(|s| s.completed).count();
            let total = plan.steps.len();
            let status = if completed == total {
                "completed"
            } else if completed > 0 {
                "in_progress"
            } else {
                "pending"
            };
            lines.push(format!(
                "id={id} status={status} progress={completed}/{total}"
            ));
        }

        Ok(ToolResult {
            output: lines.join("\n"),
        })
    }
}

// --- sop_status ---

#[derive(Debug, Deserialize)]
struct SopStatusInput {
    id: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SopStatusTool;

#[async_trait]
impl Tool for SopStatusTool {
    fn name(&self) -> &'static str {
        "sop_status"
    }

    fn description(&self) -> &'static str {
        "Get the current status and progress of an SOP."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "The SOP plan ID" }
            },
            "required": ["id"],
            "additionalProperties": false
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: SopStatusInput =
            serde_json::from_str(input).context("sop_status expects JSON: {\"id\"}")?;

        if req.id.trim().is_empty() {
            return Err(anyhow!("id must not be empty"));
        }

        let store = SopStore::load(&ctx.workspace_root).await?;
        let plan = store
            .plans
            .get(&req.id)
            .ok_or_else(|| anyhow!("SOP not found: {}", req.id))?;

        let mut lines = vec![format!("sop_id={}", plan.id)];

        // Check for deterministic run state.
        #[cfg(feature = "tools-extended")]
        {
            let det_state = SopEngine::load_state(&req.id, &ctx.workspace_root).await?;
            if let Some(ref ds) = det_state {
                let savings = SopEngine::calculate_savings(ds);
                lines.push(format!(
                    "  execution_mode=deterministic current_step={} steps_executed={} llm_calls_saved={}",
                    ds.current_step, savings.steps_executed, savings.llm_calls_saved
                ));
            }
        }

        for (i, step) in plan.steps.iter().enumerate() {
            let approval_key = SopStore::approval_key(&plan.id, i);
            let needs_approval = store.approvals.contains_key(&approval_key);
            let approved = store.approvals.get(&approval_key).copied().unwrap_or(false);

            let status = if step.completed {
                "completed".to_string()
            } else if needs_approval && !approved {
                "awaiting_approval".to_string()
            } else {
                "pending".to_string()
            };
            lines.push(format!(
                "  step[{i}] title=\"{}\" status={status}",
                step.title
            ));
        }

        Ok(ToolResult {
            output: lines.join("\n"),
        })
    }
}

// --- sop_advance ---

#[derive(Debug, Deserialize)]
struct SopAdvanceInput {
    id: String,
    step_index: usize,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SopAdvanceTool;

#[async_trait]
impl Tool for SopAdvanceTool {
    fn name(&self) -> &'static str {
        "sop_advance"
    }

    fn description(&self) -> &'static str {
        "Advance an SOP to the next step."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "The SOP plan ID" },
                "step_index": { "type": "integer", "description": "Index of the step to mark as completed" }
            },
            "required": ["id", "step_index"],
            "additionalProperties": false
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: SopAdvanceInput = serde_json::from_str(input)
            .context("sop_advance expects JSON: {\"id\", \"step_index\"}")?;

        if req.id.trim().is_empty() {
            return Err(anyhow!("id must not be empty"));
        }

        let mut store = SopStore::load(&ctx.workspace_root).await?;

        // Check if step requires approval
        let approval_key = SopStore::approval_key(&req.id, req.step_index);
        if store.approvals.get(&approval_key) == Some(&false) {
            return Err(anyhow!(
                "step {} requires approval before it can be advanced",
                req.step_index
            ));
        }

        // Check if a deterministic run state exists for this plan.
        #[cfg(feature = "tools-extended")]
        {
            let det_state = SopEngine::load_state(&req.id, &ctx.workspace_root).await?;
            if let Some(mut ds) = det_state {
                let plan_ref = store
                    .plans
                    .get(&req.id)
                    .ok_or_else(|| anyhow!("SOP not found: {}", req.id))?;
                let step_kinds = vec![SopStepKind::Execute; plan_ref.steps.len()];
                let det_status = SopEngine::advance_deterministic_step(
                    &mut ds,
                    plan_ref,
                    serde_json::json!({"step_index": req.step_index, "status": "completed"}),
                    &step_kinds,
                )?;
                SopEngine::persist_state(&ds, &ctx.workspace_root).await?;

                // Also mark the step completed in the regular plan.
                {
                    let plan = store
                        .plans
                        .get_mut(&req.id)
                        .ok_or_else(|| anyhow!("SOP not found: {}", req.id))?;
                    sop::advance_step(plan, req.step_index)?;
                }
                store.save(&ctx.workspace_root).await?;

                let title = &store.plans[&req.id].steps[req.step_index].title;
                return Ok(ToolResult {
                    output: format!(
                        "advanced sop={} step={} title=\"{title}\" mode=deterministic det_status={det_status:?}",
                        req.id, req.step_index
                    ),
                });
            }
        }

        {
            let plan = store
                .plans
                .get_mut(&req.id)
                .ok_or_else(|| anyhow!("SOP not found: {}", req.id))?;
            sop::advance_step(plan, req.step_index)?;
        }

        store.save(&ctx.workspace_root).await?;

        let title = &store.plans[&req.id].steps[req.step_index].title;
        Ok(ToolResult {
            output: format!(
                "advanced sop={} step={} title=\"{title}\"",
                req.id, req.step_index
            ),
        })
    }
}

// --- sop_approve ---

#[derive(Debug, Deserialize)]
struct SopApproveInput {
    id: String,
    step_index: usize,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SopApproveTool;

#[async_trait]
impl Tool for SopApproveTool {
    fn name(&self) -> &'static str {
        "sop_approve"
    }

    fn description(&self) -> &'static str {
        "Approve a step in an SOP that requires human approval."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "The SOP plan ID" },
                "step_index": { "type": "integer", "description": "Index of the step to approve" }
            },
            "required": ["id", "step_index"],
            "additionalProperties": false
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: SopApproveInput = serde_json::from_str(input)
            .context("sop_approve expects JSON: {\"id\", \"step_index\"}")?;

        if req.id.trim().is_empty() {
            return Err(anyhow!("id must not be empty"));
        }

        let mut store = SopStore::load(&ctx.workspace_root).await?;

        let plan = store
            .plans
            .get(&req.id)
            .ok_or_else(|| anyhow!("SOP not found: {}", req.id))?;

        if req.step_index >= plan.steps.len() {
            return Err(anyhow!(
                "step index {} is out of range (plan has {} steps)",
                req.step_index,
                plan.steps.len()
            ));
        }

        if plan.steps[req.step_index].completed {
            return Err(anyhow!("step {} is already completed", req.step_index));
        }

        let approval_key = SopStore::approval_key(&req.id, req.step_index);
        store.approvals.insert(approval_key, true);
        store.save(&ctx.workspace_root).await?;

        Ok(ToolResult {
            output: format!(
                "approved sop={} step={} title=\"{}\"",
                req.id, req.step_index, plan.steps[req.step_index].title
            ),
        })
    }
}

// --- sop_execute ---

#[derive(Debug, Deserialize)]
struct SopExecuteInput {
    id: String,
    steps: Vec<String>,
    #[serde(default)]
    approval_required: Vec<usize>,
    /// Run in deterministic mode (bypass LLM for step transitions).
    #[serde(default)]
    deterministic: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SopExecuteTool;

#[async_trait]
impl Tool for SopExecuteTool {
    fn name(&self) -> &'static str {
        "sop_execute"
    }

    fn description(&self) -> &'static str {
        "Create and begin executing a new SOP with defined steps."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Unique SOP plan ID" },
                "steps": { "type": "array", "items": { "type": "string" }, "description": "List of step titles" },
                "approval_required": { "type": "array", "items": { "type": "integer" }, "description": "Indices of steps requiring human approval" },
                "deterministic": { "type": "boolean", "description": "Run in deterministic mode (bypass LLM for step transitions)", "default": false }
            },
            "required": ["id", "steps"],
            "additionalProperties": false
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: SopExecuteInput = serde_json::from_str(input).context(
            "sop_execute expects JSON: {\"id\", \"steps\": [...], \"approval_required\"?: [...]}",
        )?;

        if req.id.trim().is_empty() {
            return Err(anyhow!("id must not be empty"));
        }
        if req.steps.is_empty() {
            return Err(anyhow!("steps must not be empty"));
        }

        let mut store = SopStore::load(&ctx.workspace_root).await?;

        if store.plans.contains_key(&req.id) {
            return Err(anyhow!("SOP already exists: {}", req.id));
        }

        let step_refs: Vec<&str> = req.steps.iter().map(|s| s.as_str()).collect();
        let plan = sop::create_plan(&req.id, &step_refs)?;

        // Register approval requirements
        for &idx in &req.approval_required {
            if idx >= plan.steps.len() {
                return Err(anyhow!(
                    "approval_required index {} is out of range (plan has {} steps)",
                    idx,
                    plan.steps.len()
                ));
            }
            let key = SopStore::approval_key(&req.id, idx);
            store.approvals.insert(key, false);
        }

        let step_count = plan.steps.len();

        // If deterministic mode, create and persist deterministic run state.
        #[cfg(feature = "tools-extended")]
        let mode_label = if req.deterministic {
            let det_state = SopEngine::start_deterministic_run(&plan);
            SopEngine::persist_state(&det_state, &ctx.workspace_root).await?;
            "deterministic"
        } else {
            "supervised"
        };
        #[cfg(not(feature = "tools-extended"))]
        let mode_label = "supervised";

        store.plans.insert(req.id.clone(), plan);
        store.save(&ctx.workspace_root).await?;

        Ok(ToolResult {
            output: format!(
                "created sop={} steps={} approval_required={} mode={}",
                req.id,
                step_count,
                req.approval_required.len(),
                mode_label
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::{Tool, ToolContext};
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
            "agentzero-sop-tools-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn sop_execute_create_and_list() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let result = SopExecuteTool
            .execute(
                r#"{"id": "deploy", "steps": ["build", "test", "ship"]}"#,
                &ctx,
            )
            .await
            .expect("execute should succeed");
        assert!(result.output.contains("created sop=deploy"));
        assert!(result.output.contains("steps=3"));

        let result = SopListTool
            .execute("{}", &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("id=deploy"));
        assert!(result.output.contains("progress=0/3"));
        assert!(result.output.contains("status=pending"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_advance_and_status() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        SopExecuteTool
            .execute(
                r#"{"id": "release", "steps": ["prepare", "review", "publish"]}"#,
                &ctx,
            )
            .await
            .unwrap();

        SopAdvanceTool
            .execute(r#"{"id": "release", "step_index": 0}"#, &ctx)
            .await
            .expect("advance should succeed");

        let result = SopStatusTool
            .execute(r#"{"id": "release"}"#, &ctx)
            .await
            .expect("status should succeed");
        assert!(result.output.contains("sop_id=release"));
        assert!(result.output.contains("step[0]"));
        assert!(result.output.contains("completed"));
        assert!(result.output.contains("step[1]"));
        assert!(result.output.contains("pending"));

        // List shows in_progress
        let result = SopListTool
            .execute("{}", &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("status=in_progress"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_approval_flow() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        // Create SOP with step 1 requiring approval
        SopExecuteTool
            .execute(
                r#"{"id": "prod-deploy", "steps": ["build", "approve-deploy", "deploy"], "approval_required": [1]}"#,
                &ctx,
            )
            .await
            .unwrap();

        // Advance step 0 (no approval needed)
        SopAdvanceTool
            .execute(r#"{"id": "prod-deploy", "step_index": 0}"#, &ctx)
            .await
            .expect("step 0 should advance");

        // Try to advance step 1 without approval — should fail
        let err = SopAdvanceTool
            .execute(r#"{"id": "prod-deploy", "step_index": 1}"#, &ctx)
            .await
            .expect_err("unapproved step should fail");
        assert!(err.to_string().contains("requires approval"));

        // Status shows awaiting_approval
        let result = SopStatusTool
            .execute(r#"{"id": "prod-deploy"}"#, &ctx)
            .await
            .unwrap();
        assert!(result.output.contains("awaiting_approval"));

        // Approve step 1
        let result = SopApproveTool
            .execute(r#"{"id": "prod-deploy", "step_index": 1}"#, &ctx)
            .await
            .expect("approve should succeed");
        assert!(result.output.contains("approved"));

        // Now advance step 1
        SopAdvanceTool
            .execute(r#"{"id": "prod-deploy", "step_index": 1}"#, &ctx)
            .await
            .expect("approved step should advance");

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_list_empty() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let result = SopListTool
            .execute("{}", &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("no SOPs found"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_execute_rejects_empty_id() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let err = SopExecuteTool
            .execute(r#"{"id": "", "steps": ["a"]}"#, &ctx)
            .await
            .expect_err("empty id should fail");
        assert!(err.to_string().contains("id must not be empty"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_execute_rejects_duplicate_id() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        SopExecuteTool
            .execute(r#"{"id": "dup", "steps": ["a"]}"#, &ctx)
            .await
            .unwrap();

        let err = SopExecuteTool
            .execute(r#"{"id": "dup", "steps": ["b"]}"#, &ctx)
            .await
            .expect_err("duplicate should fail");
        assert!(err.to_string().contains("already exists"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_status_not_found() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let err = SopStatusTool
            .execute(r#"{"id": "nonexistent"}"#, &ctx)
            .await
            .expect_err("not found should fail");
        assert!(err.to_string().contains("not found"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_advance_wrong_plan_fails() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let err = SopAdvanceTool
            .execute(r#"{"id": "no-such-plan", "step_index": 0}"#, &ctx)
            .await
            .expect_err("non-existent plan should fail");
        assert!(err.to_string().contains("not found"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_advance_out_of_range() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        SopExecuteTool
            .execute(r#"{"id": "small", "steps": ["only"]}"#, &ctx)
            .await
            .unwrap();

        let err = SopAdvanceTool
            .execute(r#"{"id": "small", "step_index": 5}"#, &ctx)
            .await
            .expect_err("out of range should fail");
        assert!(err.to_string().contains("out of range"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_execute_creates_plan() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let result = SopExecuteTool
            .execute(
                r#"{"id":"test-plan","steps":["step1","step2","step3"]}"#,
                &ctx,
            )
            .await
            .expect("execute should succeed");
        assert!(result.output.contains("created sop=test-plan"));
        assert!(result.output.contains("steps=3"));
        assert!(result.output.contains("approval_required=0"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_list_shows_created_plan() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        SopExecuteTool
            .execute(
                r#"{"id":"test-plan","steps":["step1","step2","step3"]}"#,
                &ctx,
            )
            .await
            .unwrap();

        let result = SopListTool
            .execute("{}", &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("id=test-plan"));
        assert!(result.output.contains("progress=0/3"));
        assert!(result.output.contains("status=pending"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_status_shows_steps() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        SopExecuteTool
            .execute(
                r#"{"id":"test-plan","steps":["step1","step2","step3"]}"#,
                &ctx,
            )
            .await
            .unwrap();

        let result = SopStatusTool
            .execute(r#"{"id":"test-plan"}"#, &ctx)
            .await
            .expect("status should succeed");
        assert!(result.output.contains("sop_id=test-plan"));
        assert!(result.output.contains("step[0]"));
        assert!(result.output.contains("step[1]"));
        assert!(result.output.contains("step[2]"));
        assert!(result.output.contains("pending"));
        // All steps should be pending
        assert!(!result.output.contains("completed"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_advance_marks_step_complete() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        SopExecuteTool
            .execute(
                r#"{"id":"test-plan","steps":["step1","step2","step3"]}"#,
                &ctx,
            )
            .await
            .unwrap();

        let result = SopAdvanceTool
            .execute(r#"{"id":"test-plan","step_index":0}"#, &ctx)
            .await
            .expect("advance should succeed");
        assert!(result.output.contains("advanced sop=test-plan step=0"));
        assert!(result.output.contains("title=\"step1\""));

        // Verify via status that step 0 is completed and step 1 is still pending
        let status = SopStatusTool
            .execute(r#"{"id":"test-plan"}"#, &ctx)
            .await
            .unwrap();
        assert!(status.output.contains("step[0]"));
        assert!(status.output.contains("completed"));
        assert!(status.output.contains("step[1]"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_approve_then_advance() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        // Create plan with step 1 requiring approval
        SopExecuteTool
            .execute(
                r#"{"id":"test-plan","steps":["step1","step2","step3"],"approval_required":[1]}"#,
                &ctx,
            )
            .await
            .unwrap();

        // Advance step 0 (no approval needed)
        SopAdvanceTool
            .execute(r#"{"id":"test-plan","step_index":0}"#, &ctx)
            .await
            .expect("step 0 should advance without approval");

        // Try to advance step 1 without approval — should fail
        let err = SopAdvanceTool
            .execute(r#"{"id":"test-plan","step_index":1}"#, &ctx)
            .await
            .expect_err("unapproved step should fail");
        assert!(err.to_string().contains("requires approval"));

        // Approve step 1
        SopApproveTool
            .execute(r#"{"id":"test-plan","step_index":1}"#, &ctx)
            .await
            .expect("approve should succeed");

        // Now advance step 1 — should succeed
        let result = SopAdvanceTool
            .execute(r#"{"id":"test-plan","step_index":1}"#, &ctx)
            .await
            .expect("approved step should advance");
        assert!(result.output.contains("advanced sop=test-plan step=1"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sop_approve_completed_step_fails() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        SopExecuteTool
            .execute(r#"{"id": "done", "steps": ["a"]}"#, &ctx)
            .await
            .unwrap();

        SopAdvanceTool
            .execute(r#"{"id": "done", "step_index": 0}"#, &ctx)
            .await
            .unwrap();

        let err = SopApproveTool
            .execute(r#"{"id": "done", "step_index": 0}"#, &ctx)
            .await
            .expect_err("approving completed step should fail");
        assert!(err.to_string().contains("already completed"));

        fs::remove_dir_all(dir).ok();
    }

    #[cfg(feature = "tools-extended")]
    #[tokio::test]
    async fn sop_execute_deterministic_creates_run_state() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let result = SopExecuteTool
            .execute(
                r#"{"id": "det-plan", "steps": ["a", "b", "c"], "deterministic": true}"#,
                &ctx,
            )
            .await
            .expect("execute should succeed");
        assert!(result.output.contains("mode=deterministic"));

        // Verify the deterministic state file exists.
        let state_path = dir
            .join(".agentzero")
            .join("sop_runs")
            .join("det-plan.json");
        assert!(state_path.exists(), "deterministic state file should exist");

        // Read and verify the state.
        let data = fs::read_to_string(&state_path).expect("should read state file");
        let state: crate::sop::types::DeterministicRunState =
            serde_json::from_str(&data).expect("should parse state");
        assert_eq!(state.plan_id, "det-plan");
        assert_eq!(state.current_step, 0);

        fs::remove_dir_all(dir).ok();
    }

    #[cfg(feature = "tools-extended")]
    #[tokio::test]
    async fn sop_status_shows_deterministic_info() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        // Create a deterministic SOP.
        SopExecuteTool
            .execute(
                r#"{"id": "det-status", "steps": ["x", "y"], "deterministic": true}"#,
                &ctx,
            )
            .await
            .expect("execute should succeed");

        // Status should show deterministic execution mode.
        let result = SopStatusTool
            .execute(r#"{"id": "det-status"}"#, &ctx)
            .await
            .expect("status should succeed");
        assert!(result.output.contains("execution_mode=deterministic"));
        assert!(result.output.contains("current_step=0"));
        assert!(result.output.contains("steps_executed=0"));

        fs::remove_dir_all(dir).ok();
    }
}
