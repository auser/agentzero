//! CLI command for autonomous swarm execution.
//!
//! `agentzero swarm "Build a REST API with auth"` decomposes the goal
//! into a workflow graph and executes it using the swarm supervisor.

use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_infra::runtime::build_provider_from_config;
use agentzero_orchestrator::{parse_planner_response, GoalPlanner, NodeStatus, SwarmSupervisor};
use async_trait::async_trait;

use super::workflow::build_cli_dispatcher;

pub struct SwarmCommand;

#[async_trait]
impl AgentZeroCommand for SwarmCommand {
    type Options = SwarmOptions;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        cmd_swarm(ctx, opts).await
    }
}

/// Options for the swarm command (parsed from CLI args by clap in cli.rs).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SwarmOptions {
    pub goal: String,
    pub plan_file: Option<std::path::PathBuf>,
    pub sandbox_level: String,
}

async fn cmd_swarm(ctx: &CommandContext, opts: SwarmOptions) -> anyhow::Result<()> {
    let plan = if let Some(ref path) = opts.plan_file {
        // Load a pre-generated plan from file.
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read plan file: {e}"))?;
        parse_planner_response(&raw)?
    } else {
        // Decompose the goal into a multi-agent workflow using the GoalPlanner.
        eprintln!("Goal: {}", opts.goal);
        eprintln!("Decomposing goal into agent tasks...\n");

        let provider = build_provider_from_config(&ctx.config_path, None, None, None).await?;
        let planner = GoalPlanner::new(provider);

        // Build tool summaries so the planner can assign tool_hints per node.
        let tool_summaries = build_tool_summaries_for_planner(&ctx.config_path)?;

        planner.plan(&opts.goal, &tool_summaries).await?
    };

    eprintln!("Workflow: {}", plan.title);
    eprintln!("Agents:   {}", plan.nodes.len());
    for node in &plan.nodes {
        let deps = if node.depends_on.is_empty() {
            "root".to_string()
        } else {
            format!("after {}", node.depends_on.join(", "))
        };
        eprintln!("  {} — {} ({})", node.id, node.name, deps);
    }
    eprintln!();

    // Compile and build dispatcher.
    let (nodes, edges) = plan.to_workflow_json();
    let exec_plan = agentzero_orchestrator::compile_workflow("swarm", &nodes, &edges)
        .map_err(|e| anyhow::anyhow!("compilation failed: {e}"))?;

    let dispatcher = build_cli_dispatcher(ctx, &exec_plan);

    // Set up status streaming.
    let (status_tx, mut status_rx) =
        tokio::sync::mpsc::channel::<agentzero_orchestrator::StatusUpdate>(64);

    // Spawn status printer.
    tokio::spawn(async move {
        while let Some(update) = status_rx.recv().await {
            let icon = match update.status {
                NodeStatus::Running => "▶",
                NodeStatus::Completed => "✓",
                NodeStatus::Failed => "✗",
                NodeStatus::Skipped => "⊘",
                _ => "·",
            };
            eprint!("  {icon} {}: {:?}", update.node_name, update.status);
            if let Some(ref out) = update.output {
                let preview = if out.len() > 80 {
                    format!("{}...", &out[..80])
                } else {
                    out.clone()
                };
                eprint!(" — {preview}");
            }
            eprintln!();
        }
    });

    // Execute via SwarmSupervisor.
    let supervisor = SwarmSupervisor::new();
    let result = supervisor
        .execute(&plan, &opts.goal, dispatcher, Some(status_tx))
        .await?;

    // Print summary.
    eprintln!();
    println!("─── Swarm Results ─────────────────────────────────────");
    println!("Run ID:   {}", result.run_id);
    println!("Title:    {}", result.workflow_title);
    println!(
        "Status:   {}",
        if result.success { "SUCCESS" } else { "FAILED" }
    );
    println!("Agents:   {}", result.node_count);

    println!("\n{:<20} {:<12}", "Agent", "Status");
    println!("{}", "-".repeat(34));
    let mut sorted: Vec<_> = result.node_statuses.iter().collect();
    sorted.sort_by_key(|(k, _)| (*k).clone());
    for (node_id, status) in &sorted {
        let s = match status {
            NodeStatus::Completed => "completed",
            NodeStatus::Failed => "FAILED",
            NodeStatus::Skipped => "skipped",
            _ => "other",
        };
        println!("{:<20} {:<12}", node_id, s);
    }

    if !result.outputs.is_empty() {
        println!("\n─── Outputs ───────────────────────────────────────────");
        for (node_id, output) in &result.outputs {
            let display = if output.len() > 500 {
                format!("{}...", &output[..500])
            } else {
                output.clone()
            };
            println!("\n[{node_id}]\n{display}");
        }
    }

    if !result.success {
        std::process::exit(1);
    }

    Ok(())
}

/// Build lightweight tool summaries from config for the goal planner.
///
/// Loads the security policy and builds tool names + descriptions without
/// constructing full tool instances. Uses `default_tools_with_store` with a
/// no-op agent store, then extracts `ToolSummary` from each tool.
fn build_tool_summaries_for_planner(
    config_path: &std::path::Path,
) -> anyhow::Result<Vec<agentzero_core::ToolSummary>> {
    let workspace_root = config_path.parent().unwrap_or(std::path::Path::new("."));
    let policy = agentzero_config::load_tool_security_policy(workspace_root, config_path)?;
    let tools = agentzero_infra::tools::default_tools_with_store(&policy, None, None, None)?;
    Ok(tools
        .iter()
        .filter_map(|t| {
            let desc = t.description();
            if desc.is_empty() {
                return None;
            }
            Some(agentzero_core::ToolSummary {
                name: t.name().to_string(),
                description: desc.to_string(),
            })
        })
        .collect())
}
