//! LLM-callable tool for creating, listing, and managing skills via chat.
//!
//! Wraps the existing `SkillStore` and `SkillForge` from `agentzero-tools`.

use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use agentzero_tools::skills::skillforge::{render_skill_markdown, SkillTemplate};
use agentzero_tools::skills::SkillStore;
use anyhow::{bail, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct Input {
    /// The skill operation to perform
    #[schema(enum_values = ["create", "list", "get", "update", "remove", "test"])]
    action: String,
    /// Skill name (alphanumeric, hyphens, underscores)
    #[serde(default)]
    name: Option<String>,
    /// For create: what the skill does
    #[serde(default)]
    description: Option<String>,
    /// For create/update: the markdown source content. If omitted on create, auto-generated from description.
    #[serde(default)]
    source: Option<String>,
}

#[tool(
    name = "skill_manage",
    description = "Create, list, update, and remove skills. Skills are reusable AI behavior templates. Actions: create (generate + install), list, get, update (replace source), remove, test."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct SkillManageTool;

impl SkillManageTool {
    fn data_dir(ctx: &ToolContext) -> PathBuf {
        PathBuf::from(&ctx.workspace_root).join(".agentzero")
    }
}

#[async_trait]
impl Tool for SkillManageTool {
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
        let req: Input = serde_json::from_str(input).context("invalid skill_manage input")?;

        if ctx.depth > 0 {
            bail!("skill_manage is not available to sub-agents (depth > 0)");
        }

        let store = SkillStore::new(Self::data_dir(ctx))?;

        match req.action.as_str() {
            "create" => action_create(&store, req.name, req.description, req.source),
            "list" => action_list(&store),
            "get" => action_get(&store, req.name),
            "update" => action_update(&store, req.name, req.source),
            "remove" => action_remove(&store, req.name),
            "test" => action_test(&store, req.name),
            other => bail!("unknown skill_manage action: {other}"),
        }
    }
}

fn action_create(
    store: &SkillStore,
    name: Option<String>,
    description: Option<String>,
    source: Option<String>,
) -> anyhow::Result<ToolResult> {
    let name = name.context("'name' is required for create")?;
    let description = description.context("'description' is required for create")?;

    let source = match source {
        Some(s) => s,
        None => render_skill_markdown(&SkillTemplate {
            name: name.clone(),
            description: description.clone(),
        })?,
    };

    let record = store.install(&name, &source)?;
    Ok(ToolResult {
        output: format!(
            "Skill '{}' created and installed (enabled: {}).",
            record.name, record.enabled
        ),
    })
}

fn action_list(store: &SkillStore) -> anyhow::Result<ToolResult> {
    let skills = store.list()?;
    if skills.is_empty() {
        return Ok(ToolResult {
            output: "No skills installed.".to_string(),
        });
    }
    let mut output = format!("{} skill(s) installed:\n", skills.len());
    for s in &skills {
        output.push_str(&format!(
            "  - {} (enabled: {}, source: {} chars)\n",
            s.name,
            s.enabled,
            s.source.len()
        ));
    }
    Ok(ToolResult { output })
}

fn action_get(store: &SkillStore, name: Option<String>) -> anyhow::Result<ToolResult> {
    let name = name.context("'name' is required for get")?;
    let record = store.get(&name)?;
    Ok(ToolResult {
        output: format!(
            "Skill: {}\nEnabled: {}\nSource:\n{}",
            record.name, record.enabled, record.source
        ),
    })
}

fn action_update(
    store: &SkillStore,
    name: Option<String>,
    source: Option<String>,
) -> anyhow::Result<ToolResult> {
    let name = name.context("'name' is required for update")?;
    let source = source.context("'source' is required for update")?;

    // Remove and re-install with new source
    store.remove(&name)?;
    store.install(&name, &source)?;
    Ok(ToolResult {
        output: format!("Skill '{}' updated.", name),
    })
}

fn action_remove(store: &SkillStore, name: Option<String>) -> anyhow::Result<ToolResult> {
    let name = name.context("'name' is required for remove")?;
    store.remove(&name)?;
    Ok(ToolResult {
        output: format!("Skill '{}' removed.", name),
    })
}

fn action_test(store: &SkillStore, name: Option<String>) -> anyhow::Result<ToolResult> {
    let name = name.context("'name' is required for test")?;
    let output = store.test(&name)?;
    Ok(ToolResult { output })
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
            "agentzero-skillmgr-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn make_ctx(dir: &std::path::Path) -> ToolContext {
        ToolContext::new(dir.to_string_lossy().to_string())
    }

    #[tokio::test]
    async fn create_and_list_skills() {
        let dir = temp_dir();
        let ctx = make_ctx(&dir);
        let tool = SkillManageTool;

        let input = serde_json::json!({
            "action": "create",
            "name": "code_review",
            "description": "Review code for quality and style"
        });
        let result = tool
            .execute(&serde_json::to_string(&input).expect("json"), &ctx)
            .await
            .expect("create should succeed");
        assert!(result.output.contains("created and installed"));

        let result = tool
            .execute(r#"{"action": "list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("code_review"));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn depth_blocks_sub_agents() {
        let dir = temp_dir();
        let mut ctx = make_ctx(&dir);
        ctx.depth = 1;
        let tool = SkillManageTool;

        let err = tool
            .execute(r#"{"action": "list"}"#, &ctx)
            .await
            .expect_err("sub-agent should be blocked");
        assert!(err.to_string().contains("not available to sub-agents"));
        let _ = fs::remove_dir_all(dir);
    }
}
