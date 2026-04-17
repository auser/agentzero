//! LLM-callable tool for querying the agent's performance history.
//!
//! Placed in `agentzero-infra` because it reads trajectory files managed
//! by the infra layer's `TrajectoryRecorder`.

use crate::insights;
use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct Input {
    /// What to report on.
    #[schema(
        enum_values = ["summary", "models", "tools", "failures", "cost"],
        default = "summary"
    )]
    focus: Option<String>,
}

#[tool(
    name = "insights_report",
    description = "Query the agent's own performance history: success rates, model effectiveness, tool usage heatmap, failure patterns, and cost trends. Use this to understand what works and what doesn't."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct InsightsReportTool;

#[async_trait]
impl Tool for InsightsReportTool {
    fn name(&self) -> &'static str {
        "insights_report"
    }

    fn description(&self) -> &'static str {
        "Query the agent's own performance history: success rates, model effectiveness, tool usage heatmap, failure patterns, and cost trends."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(Input::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: Input = serde_json::from_str(input).unwrap_or(Input { focus: None });
        let focus = parsed.focus.as_deref().unwrap_or("summary");

        let data_dir = resolve_data_dir(ctx);
        let report = insights::generate_report(&data_dir)?;

        let output = match focus {
            "models" => format_model_stats(&report),
            "tools" => format_tool_stats(&report),
            "failures" => format_failure_patterns(&report),
            "cost" => format_cost_stats(&report),
            _ => format_summary(&report),
        };

        Ok(ToolResult { output })
    }
}

fn resolve_data_dir(ctx: &ToolContext) -> PathBuf {
    if let Some(ref p) = ctx.config_path {
        if let Some(parent) = PathBuf::from(p).parent() {
            return parent.to_path_buf();
        }
    }
    PathBuf::from(&ctx.workspace_root)
}

fn format_summary(r: &insights::InsightsReport) -> String {
    if r.total_runs == 0 {
        return "No trajectory data yet. Run some tasks first to build history.".to_string();
    }
    let cost_usd = r.total_cost_microdollars as f64 / 1_000_000.0;
    let avg_usd = r.avg_cost_microdollars as f64 / 1_000_000.0;
    format!(
        "## Performance Summary\n\
         - **Total runs:** {}\n\
         - **Success rate:** {:.1}% ({} success, {} failed, {} partial)\n\
         - **Total cost:** ${:.4} (avg ${:.4}/run)\n\
         - **Total tokens:** {}\n\
         - **Models used:** {}\n\
         - **Tools used:** {}\n\
         - **Failure patterns detected:** {}",
        r.total_runs,
        r.success_rate * 100.0,
        r.successful_runs,
        r.failed_runs,
        r.partial_runs,
        cost_usd,
        avg_usd,
        r.total_tokens,
        r.model_stats.len(),
        r.tool_stats.len(),
        r.failure_patterns.len(),
    )
}

fn format_model_stats(r: &insights::InsightsReport) -> String {
    if r.model_stats.is_empty() {
        return "No model data yet.".to_string();
    }
    let mut lines = vec!["## Model Effectiveness".to_string()];
    let mut models: Vec<_> = r.model_stats.iter().collect();
    models.sort_by_key(|b| std::cmp::Reverse(b.1.runs));
    for (name, s) in models {
        let cost_usd = s.total_cost_microdollars as f64 / 1_000_000.0;
        lines.push(format!(
            "- **{}**: {:.0}% success ({}/{} runs), avg {:.0}ms, ${:.4} total",
            name,
            s.success_rate * 100.0,
            s.successes,
            s.runs,
            s.avg_latency_ms,
            cost_usd,
        ));
    }
    lines.join("\n")
}

fn format_tool_stats(r: &insights::InsightsReport) -> String {
    if r.tool_stats.is_empty() {
        return "No tool usage data yet.".to_string();
    }
    let mut lines = vec!["## Tool Usage Heatmap".to_string()];
    let mut tools: Vec<_> = r.tool_stats.iter().collect();
    tools.sort_by_key(|b| std::cmp::Reverse(b.1.total_uses));
    for (name, s) in tools {
        lines.push(format!(
            "- **{}**: {} uses ({:.0}% success, {} failures)",
            name,
            s.total_uses,
            s.success_rate * 100.0,
            s.failure_count,
        ));
    }
    lines.join("\n")
}

fn format_failure_patterns(r: &insights::InsightsReport) -> String {
    if r.failure_patterns.is_empty() {
        return "No failure patterns detected yet.".to_string();
    }
    let mut lines = vec!["## Failure Patterns (tool sequences preceding failures)".to_string()];
    for fp in &r.failure_patterns {
        lines.push(format!(
            "- {} → **failure** ({} occurrences)",
            fp.tool_sequence.join(" → "),
            fp.occurrences,
        ));
    }
    lines.join("\n")
}

fn format_cost_stats(r: &insights::InsightsReport) -> String {
    let total_usd = r.total_cost_microdollars as f64 / 1_000_000.0;
    let avg_usd = r.avg_cost_microdollars as f64 / 1_000_000.0;
    let mut lines = vec![
        "## Cost Analysis".to_string(),
        format!("- **Total cost:** ${:.4}", total_usd),
        format!("- **Avg cost/run:** ${:.4}", avg_usd),
        format!("- **Total tokens:** {}", r.total_tokens),
        format!("- **Total runs:** {}", r.total_runs),
    ];
    if !r.model_stats.is_empty() {
        lines.push("\n### Cost by Model".to_string());
        let mut models: Vec<_> = r.model_stats.iter().collect();
        models.sort_by(|a, b| {
            b.1.total_cost_microdollars
                .cmp(&a.1.total_cost_microdollars)
        });
        for (name, s) in models {
            let cost_usd = s.total_cost_microdollars as f64 / 1_000_000.0;
            let avg = s.avg_cost_microdollars as f64 / 1_000_000.0;
            lines.push(format!(
                "- **{}**: ${:.4} total (${:.4} avg, {} runs)",
                name, cost_usd, avg, s.runs,
            ));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_valid_json() {
        let schema = Input::schema();
        assert!(schema.is_object());
        assert!(schema["properties"]["focus"].is_object());
    }
}
