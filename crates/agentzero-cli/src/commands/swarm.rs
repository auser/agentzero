//! CLI command for autonomous swarm execution.
//!
//! `agentzero swarm "Build a REST API with auth"` decomposes the goal
//! into a workflow graph and executes it using the swarm supervisor.

use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_orchestrator::{
    parse_planner_response, NodeStatus, PlannedWorkflow, SwarmSupervisor,
};
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
        // Use the goal to create a simple single-agent plan.
        // In production, this would call the GoalPlanner with an LLM to decompose.
        // For now, wrap the goal in a single agent node.
        eprintln!("Goal: {}", opts.goal);
        eprintln!("Decomposing goal into agent tasks...\n");

        PlannedWorkflow {
            title: opts.goal.clone(),
            nodes: vec![agentzero_orchestrator::PlannedNode {
                id: "agent-1".to_string(),
                name: "executor".to_string(),
                task: opts.goal.clone(),
                depends_on: vec![],
                file_scopes: vec![],
                sandbox_level: opts.sandbox_level.clone(),
            }],
        }
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
