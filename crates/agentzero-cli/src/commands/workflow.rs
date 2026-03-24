//! CLI commands for workflow management and execution.
//!
//! Provides `workflow list`, `workflow run`, `workflow import`, and
//! `workflow export` subcommands. Workflows can be run directly from a
//! JSON file or from the persistent store.

use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_infra::runtime::{
    build_runtime_execution, run_agent_once, run_agent_with_runtime, RunAgentRequest,
};
use agentzero_infra::tool_selection::{HintedToolSelector, KeywordToolSelector};
use agentzero_orchestrator::workflow_executor::{
    compile, execute, ExecutionPlan, ExecutionStep, NodeStatus, NodeType, StepDispatcher,
};
use agentzero_orchestrator::{WorkflowRecord, WorkflowStore};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum WorkflowCommands {
    /// List all saved workflows.
    List,
    /// Execute a workflow by ID or from a JSON file.
    Run {
        /// Workflow ID from the store (e.g. wf-1234).
        #[arg(long)]
        id: Option<String>,
        /// Path to a workflow JSON file.
        #[arg(long, short)]
        file: Option<PathBuf>,
        /// Input message to seed the workflow.
        #[arg(short, long)]
        input: Option<String>,
    },
    /// Import a workflow from a JSON file into the store.
    Import {
        /// Path to a workflow JSON file.
        file: PathBuf,
    },
    /// Export a workflow from the store to a JSON file.
    Export {
        /// Workflow ID to export.
        id: String,
        /// Output path (default: <workflow-id>.json).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

pub struct WorkflowCommand;

#[async_trait]
impl AgentZeroCommand for WorkflowCommand {
    type Options = WorkflowCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            WorkflowCommands::List => cmd_list(ctx).await,
            WorkflowCommands::Run { id, file, input } => cmd_run(ctx, id, file, input).await,
            WorkflowCommands::Import { file } => cmd_import(ctx, file).await,
            WorkflowCommands::Export { id, output } => cmd_export(ctx, id, output).await,
        }
    }
}

// ── List ────────────────────────────────────────────────────────────────────

async fn cmd_list(ctx: &CommandContext) -> anyhow::Result<()> {
    let store = open_store(ctx)?;
    let workflows = store.list();

    if workflows.is_empty() {
        println!("No workflows saved. Use `workflow import` or create one in the UI.");
        return Ok(());
    }

    println!("{:<30} {:<40} {:<6} {:<6}", "ID", "Name", "Nodes", "Edges");
    println!("{}", "-".repeat(84));
    for wf in &workflows {
        println!(
            "{:<30} {:<40} {:<6} {:<6}",
            wf.workflow_id,
            wf.name,
            wf.nodes.len(),
            wf.edges.len(),
        );
    }
    println!("\n{} workflow(s) total.", workflows.len());
    Ok(())
}

// ── Run ─────────────────────────────────────────────────────────────────────

async fn cmd_run(
    ctx: &CommandContext,
    id: Option<String>,
    file: Option<PathBuf>,
    input: Option<String>,
) -> anyhow::Result<()> {
    let (workflow_id, nodes, edges) = match (id, file) {
        (_, Some(path)) => load_from_file(&path)?,
        (Some(wf_id), None) => load_from_store(ctx, &wf_id)?,
        (None, None) => anyhow::bail!("provide either --id <workflow-id> or --file <path.json>"),
    };

    let input_text = input.as_deref().unwrap_or("");

    println!("Compiling workflow {workflow_id}...");
    let plan = compile(&workflow_id, &nodes, &edges)
        .map_err(|e| anyhow::anyhow!("compilation failed: {e}"))?;

    println!(
        "Execution plan: {} level(s), {} step(s)",
        plan.levels.len(),
        plan.levels.iter().map(|l| l.len()).sum::<usize>(),
    );

    let dispatcher: std::sync::Arc<dyn StepDispatcher> =
        std::sync::Arc::new(CliStepDispatcher::new(ctx, &plan));

    println!("Executing...\n");
    let run = execute(&plan, input_text, dispatcher).await?;

    // Print results
    println!("─── Results ───────────────────────────────────────────");
    println!("Run ID:   {}", run.run_id);
    println!("Workflow: {workflow_id}\n");

    println!("{:<30} {:<12}", "Node", "Status");
    println!("{}", "-".repeat(42));
    let mut sorted_statuses: Vec<_> = run.node_statuses.iter().collect();
    sorted_statuses.sort_by_key(|(k, _)| (*k).clone());
    for (node_id, status) in &sorted_statuses {
        let status_str = match status {
            NodeStatus::Completed => "completed",
            NodeStatus::Failed => "FAILED",
            NodeStatus::Skipped => "skipped",
            NodeStatus::Running => "running",
            NodeStatus::Pending => "pending",
            NodeStatus::Suspended => "suspended",
        };
        println!("{:<30} {:<12}", node_id, status_str);
    }

    // Print outputs
    println!("\n─── Outputs ───────────────────────────────────────────");
    let mut sorted_outputs: Vec<_> = run.outputs.iter().collect();
    sorted_outputs.sort_by_key(|((n, p), _)| format!("{n}:{p}"));
    for ((node_id, port), value) in &sorted_outputs {
        let display = match value {
            serde_json::Value::String(s) => {
                if s.len() > 200 {
                    format!("{}...", &s[..200])
                } else {
                    s.clone()
                }
            }
            other => other.to_string(),
        };
        println!("{node_id}:{port} = {display}");
    }

    Ok(())
}

// ── Import ──────────────────────────────────────────────────────────────────

async fn cmd_import(ctx: &CommandContext, path: PathBuf) -> anyhow::Result<()> {
    let (_, nodes, edges) = load_from_file(&path)?;

    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "imported".to_string());

    let store = open_store(ctx)?;
    let record = store.create(WorkflowRecord {
        workflow_id: String::new(),
        name,
        description: String::new(),
        nodes,
        edges,
        created_at: 0,
        updated_at: 0,
    })?;
    println!(
        "Imported workflow: {} ({})",
        record.name, record.workflow_id
    );
    Ok(())
}

// ── Export ───────────────────────────────────────────────────────────────────

async fn cmd_export(
    ctx: &CommandContext,
    id: String,
    output: Option<PathBuf>,
) -> anyhow::Result<()> {
    let store = open_store(ctx)?;
    let workflow = store
        .get(&id)
        .ok_or_else(|| anyhow::anyhow!("workflow '{id}' not found"))?;

    let out_path = output.unwrap_or_else(|| PathBuf::from(format!("{}.json", id)));
    let json = serde_json::json!({
        "workflow_id": workflow.workflow_id,
        "name": workflow.name,
        "description": workflow.description,
        "nodes": workflow.nodes,
        "edges": workflow.edges,
    });

    std::fs::write(&out_path, serde_json::to_string_pretty(&json)?)?;
    println!("Exported {} → {}", id, out_path.display());
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn open_store(ctx: &CommandContext) -> anyhow::Result<WorkflowStore> {
    WorkflowStore::persistent(&ctx.data_dir)
}

fn load_from_file(
    path: &std::path::Path,
) -> anyhow::Result<(String, Vec<serde_json::Value>, Vec<serde_json::Value>)> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    let doc: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("invalid JSON in {}: {e}", path.display()))?;

    // Support both { nodes, edges } and { layout: { nodes, edges } } formats
    let (nodes_val, edges_val) = if doc.get("layout").is_some() {
        (
            doc["layout"]["nodes"].clone(),
            doc["layout"]["edges"].clone(),
        )
    } else {
        (doc["nodes"].clone(), doc["edges"].clone())
    };

    let nodes: Vec<serde_json::Value> = serde_json::from_value(nodes_val)
        .map_err(|e| anyhow::anyhow!("invalid nodes array: {e}"))?;
    let edges: Vec<serde_json::Value> = serde_json::from_value(edges_val)
        .map_err(|e| anyhow::anyhow!("invalid edges array: {e}"))?;

    let workflow_id = doc["workflow_id"]
        .as_str()
        .unwrap_or("file-workflow")
        .to_string();

    Ok((workflow_id, nodes, edges))
}

fn load_from_store(
    ctx: &CommandContext,
    id: &str,
) -> anyhow::Result<(String, Vec<serde_json::Value>, Vec<serde_json::Value>)> {
    let store = open_store(ctx)?;
    let workflow = store.get(id).ok_or_else(|| {
        anyhow::anyhow!(
            "workflow '{id}' not found — use `workflow list` to see available workflows"
        )
    })?;
    Ok((
        workflow.workflow_id.clone(),
        workflow.nodes.clone(),
        workflow.edges.clone(),
    ))
}

/// Build an `Arc<dyn StepDispatcher>` for CLI workflow execution.
///
/// Public so the `swarm` command can reuse it.
pub fn build_cli_dispatcher(ctx: &CommandContext, plan: &ExecutionPlan) -> Arc<dyn StepDispatcher> {
    Arc::new(CliStepDispatcher::new(ctx, plan))
}

// ── CLI Step Dispatcher ─────────────────────────────────────────────────────

/// Step dispatcher for CLI workflow execution.
///
/// Uses the same `run_agent_once` infrastructure as the `agent` CLI command,
/// and `default_tools()` for tool execution. Agents are injected with a
/// `ConverseTool` so they can have multi-turn conversations with other agents
/// in the same workflow.
struct CliStepDispatcher {
    workspace_root: PathBuf,
    config_path: PathBuf,
    agent_store: Option<Arc<dyn agentzero_core::agent_store::AgentStoreApi>>,
    /// Agent endpoints for all agent nodes in the workflow, keyed by node name.
    agent_endpoints: HashMap<String, Arc<dyn agentzero_core::AgentEndpoint>>,
}

impl CliStepDispatcher {
    fn new(ctx: &CommandContext, plan: &ExecutionPlan) -> Self {
        let agent_store =
            match agentzero_orchestrator::agent_store::AgentStore::persistent(&ctx.data_dir) {
                Ok(store) => {
                    Some(Arc::new(store) as Arc<dyn agentzero_core::agent_store::AgentStoreApi>)
                }
                Err(e) => {
                    tracing::debug!(error = %e, "could not open agent store for workflow");
                    None
                }
            };

        // Build endpoints for all agent nodes in the workflow.
        let mut agent_endpoints: HashMap<String, Arc<dyn agentzero_core::AgentEndpoint>> =
            HashMap::new();
        for level in &plan.levels {
            for step in level {
                if matches!(step.node_type, NodeType::Agent | NodeType::SubAgent) {
                    let ep: Arc<dyn agentzero_core::AgentEndpoint> =
                        Arc::new(CliWorkflowAgentEndpoint {
                            agent_name: step.name.clone(),
                            config_path: ctx.config_path.clone(),
                            workspace_root: ctx.workspace_root.clone(),
                            provider: step.config.provider.clone(),
                            model: step.config.model.clone(),
                            role_description: step.config.role_description.clone(),
                            agent_store: agent_store.clone(),
                        });
                    agent_endpoints.insert(step.name.clone(), ep);
                }
            }
        }

        Self {
            workspace_root: ctx.workspace_root.clone(),
            config_path: ctx.config_path.clone(),
            agent_store,
            agent_endpoints,
        }
    }
}

#[async_trait]
impl StepDispatcher for CliStepDispatcher {
    async fn run_agent(
        &self,
        step: &ExecutionStep,
        input: &str,
        context: Option<&serde_json::Value>,
    ) -> anyhow::Result<String> {
        let mut message = input.to_string();
        if let Some(ctx) = context {
            message = format!("Context: {ctx}\n\nTask: {input}");
        }
        if let Some(ref role_desc) = step.config.role_description {
            message = format!("Role: {role_desc}\n\n{message}");
        }

        eprintln!("  → Running agent \"{}\"...", step.name);

        // Build ConverseTool with endpoints to peer agents in the workflow.
        let mut extra_tools: Vec<Box<dyn agentzero_core::Tool>> = Vec::new();
        if self.agent_endpoints.len() > 1 {
            let peer_endpoints: HashMap<String, Arc<dyn agentzero_core::AgentEndpoint>> = self
                .agent_endpoints
                .iter()
                .filter(|(name, _)| *name != &step.name)
                .map(|(name, ep)| (name.clone(), Arc::clone(ep)))
                .collect();

            if !peer_endpoints.is_empty() {
                extra_tools.push(Box::new(agentzero_tools::ConverseTool::new(peer_endpoints)));
            }
        }

        let req = RunAgentRequest {
            workspace_root: self.workspace_root.clone(),
            config_path: self.config_path.clone(),
            message: message.clone(),
            provider_override: step.config.provider.clone(),
            model_override: step.config.model.clone(),
            profile_override: None,
            extra_tools,
            conversation_id: None,
            agent_store: self.agent_store.clone(),
            memory_override: Some(Box::new(agentzero_core::EphemeralMemory::default())),
        };

        // Extract tool_hints from step metadata for per-node tool filtering.
        let tool_hints: Vec<String> = step
            .metadata
            .get("tool_hints")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        if tool_hints.is_empty() {
            // No hints — use default tool selection (all tools).
            let output = run_agent_once(req).await?;
            return Ok(output.response_text);
        }

        // Build execution with hinted tool selector.
        let workspace_root = req.workspace_root.clone();
        let mut execution = build_runtime_execution(req).await?;
        execution.tool_selector = Some(Box::new(HintedToolSelector {
            hints: tool_hints,
            recipes: None,
            fallback: KeywordToolSelector::default(),
        }));
        let output = run_agent_with_runtime(execution, workspace_root, message).await?;
        Ok(output.response_text)
    }

    async fn run_tool(&self, tool_name: &str, input: &serde_json::Value) -> anyhow::Result<String> {
        eprintln!("  → Running tool \"{tool_name}\"...");

        let policy =
            agentzero_config::load_tool_security_policy(&self.workspace_root, &self.config_path)?;
        let tools = agentzero_infra::tools::default_tools(&policy, None, None)?;

        let tool = tools
            .iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| anyhow::anyhow!("tool '{tool_name}' not found"))?;

        let ctx =
            agentzero_core::ToolContext::new(self.workspace_root.to_string_lossy().to_string());
        let result = tool.execute(&input.to_string(), &ctx).await?;
        Ok(result.output)
    }

    async fn send_channel(&self, channel_type: &str, message: &str) -> anyhow::Result<()> {
        eprintln!(
            "  → Channel send ({channel_type}): {}",
            &message[..message.len().min(100)]
        );
        // In CLI mode, channel sends are logged but not dispatched.
        // Future: wire up real channel delivery via agentzero-channels.
        Ok(())
    }
}

// ── CLI Workflow Agent Endpoint ─────────────────────────────────────────────

/// An [`AgentEndpoint`] for CLI workflow execution.
///
/// Used by `ConverseTool` so agents in a workflow can converse with each other.
struct CliWorkflowAgentEndpoint {
    agent_name: String,
    config_path: PathBuf,
    workspace_root: PathBuf,
    provider: Option<String>,
    model: Option<String>,
    role_description: Option<String>,
    agent_store: Option<Arc<dyn agentzero_core::agent_store::AgentStoreApi>>,
}

#[async_trait]
impl agentzero_core::AgentEndpoint for CliWorkflowAgentEndpoint {
    async fn send(&self, message: &str, _conversation_id: &str) -> anyhow::Result<String> {
        let mut full_message = message.to_string();
        if let Some(ref role_desc) = self.role_description {
            full_message = format!("Role: {role_desc}\n\n{full_message}");
        }

        eprintln!("    → Converse: calling agent \"{}\"...", self.agent_name);

        let req = RunAgentRequest {
            workspace_root: self.workspace_root.clone(),
            config_path: self.config_path.clone(),
            message: full_message,
            provider_override: self.provider.clone(),
            model_override: self.model.clone(),
            profile_override: None,
            extra_tools: vec![],
            conversation_id: None,
            agent_store: self.agent_store.clone(),
            memory_override: Some(Box::new(agentzero_core::EphemeralMemory::default())),
        };

        let output = run_agent_once(req).await?;
        Ok(output.response_text)
    }

    fn agent_id(&self) -> &str {
        &self.agent_name
    }
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
            .expect("time should be after epoch")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-cli-workflow-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn load_workflow_from_json_file() {
        let dir = temp_dir();
        let path = dir.join("test-workflow.json");
        let json = serde_json::json!({
            "workflow_id": "wf-test",
            "name": "Test Workflow",
            "nodes": [
                {
                    "id": "a1",
                    "data": { "name": "agent1", "nodeType": "agent", "metadata": {} }
                }
            ],
            "edges": []
        });
        fs::write(
            &path,
            serde_json::to_string_pretty(&json).expect("serialize"),
        )
        .expect("write file");

        let (id, nodes, edges) = load_from_file(&path).expect("should parse");
        assert_eq!(id, "wf-test");
        assert_eq!(nodes.len(), 1);
        assert!(edges.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_workflow_from_layout_format() {
        let dir = temp_dir();
        let path = dir.join("layout-workflow.json");
        let json = serde_json::json!({
            "workflow_id": "wf-layout",
            "layout": {
                "nodes": [
                    { "id": "n1", "data": { "name": "n1", "nodeType": "agent", "metadata": {} } }
                ],
                "edges": [
                    { "id": "e1", "source": "n1", "target": "n2", "sourceHandle": "response", "targetHandle": "input" }
                ]
            }
        });
        fs::write(
            &path,
            serde_json::to_string_pretty(&json).expect("serialize"),
        )
        .expect("write file");

        let (id, nodes, edges) = load_from_file(&path).expect("should parse");
        assert_eq!(id, "wf-layout");
        assert_eq!(nodes.len(), 1);
        assert_eq!(edges.len(), 1);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_workflow_from_file_missing() {
        let result = load_from_file(std::path::Path::new("/tmp/nonexistent-workflow.json"));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cmd_list_empty_store() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };
        // Should not error on empty store
        let result = cmd_list(&ctx).await;
        assert!(result.is_ok());
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn cmd_import_and_list() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let path = dir.join("import-test.json");
        let json = serde_json::json!({
            "nodes": [
                { "id": "a1", "data": { "name": "importer", "nodeType": "agent", "metadata": {} } }
            ],
            "edges": []
        });
        fs::write(
            &path,
            serde_json::to_string_pretty(&json).expect("serialize"),
        )
        .expect("write file");

        cmd_import(&ctx, path).await.expect("import should succeed");

        let store = open_store(&ctx).expect("store");
        let workflows = store.list();
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0].name, "import-test");

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn cmd_export_missing_id() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let result = cmd_export(&ctx, "wf-nonexistent".to_string(), None).await;
        assert!(result.is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
