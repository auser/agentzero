//! Insights engine — analyzes trajectory history to produce actionable stats.
//!
//! Reads the append-only JSONL trajectory files lazily (no in-memory cache)
//! and computes per-model effectiveness, tool usage heatmaps, cost trends,
//! and failure clustering so the agent can learn from its own history.

use crate::trajectory::{Outcome, TrajectoryRecord, TrajectoryRecorder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Aggregated insights from trajectory history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightsReport {
    /// Total trajectories analyzed.
    pub total_runs: usize,
    pub successful_runs: usize,
    pub failed_runs: usize,
    pub partial_runs: usize,
    /// Overall success rate (0.0–1.0).
    pub success_rate: f64,

    /// Per-model stats: model name → { runs, successes, avg_cost, avg_latency }.
    pub model_stats: HashMap<String, ModelStats>,

    /// Per-tool stats: tool name → { total_uses, success_count, failure_count }.
    pub tool_stats: HashMap<String, ToolStats>,

    /// Total cost in micro-dollars across all analyzed runs.
    pub total_cost_microdollars: u64,
    /// Average cost per run.
    pub avg_cost_microdollars: u64,
    /// Total tokens consumed.
    pub total_tokens: u64,

    /// Most common 3-tool sequences that precede failures.
    pub failure_patterns: Vec<FailurePattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStats {
    pub runs: usize,
    pub successes: usize,
    pub failures: usize,
    pub success_rate: f64,
    pub avg_cost_microdollars: u64,
    pub avg_latency_ms: u64,
    pub total_cost_microdollars: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStats {
    pub total_uses: usize,
    pub success_count: usize,
    pub failure_count: usize,
    pub success_rate: f64,
}

/// A 3-tool sequence that commonly precedes a failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailurePattern {
    pub tool_sequence: Vec<String>,
    pub occurrences: usize,
}

/// Generate an insights report by scanning both successful and failed trajectories.
pub fn generate_report(data_dir: &Path) -> anyhow::Result<InsightsReport> {
    let traj_dir = data_dir.join("trajectories");
    let success_path = traj_dir.join("successful.jsonl");
    let failure_path = traj_dir.join("failed.jsonl");

    let mut total_runs = 0usize;
    let mut successful_runs = 0usize;
    let mut failed_runs = 0usize;
    let mut partial_runs = 0usize;
    let mut total_cost = 0u64;
    let mut total_tokens = 0u64;

    let mut model_agg: HashMap<String, ModelAgg> = HashMap::new();
    let mut tool_agg: HashMap<String, ToolAgg> = HashMap::new();
    let mut failure_sequences: HashMap<Vec<String>, usize> = HashMap::new();

    let mut process = |record: TrajectoryRecord| {
        total_runs += 1;
        total_cost += record.cost_microdollars;
        total_tokens += record.input_tokens + record.output_tokens;

        let is_success = matches!(&record.outcome, Outcome::Success);
        match &record.outcome {
            Outcome::Success => successful_runs += 1,
            Outcome::Failure { .. } => failed_runs += 1,
            Outcome::Partial { .. } => partial_runs += 1,
        }

        // Model stats.
        let m = model_agg.entry(record.model.clone()).or_default();
        m.runs += 1;
        if is_success {
            m.successes += 1;
        } else {
            m.failures += 1;
        }
        m.total_cost += record.cost_microdollars;
        m.total_latency += record.latency_ms;

        // Tool stats.
        for exec in &record.tool_executions {
            let t = tool_agg.entry(exec.tool_name.clone()).or_default();
            t.total_uses += 1;
            if exec.success {
                t.success_count += 1;
            } else {
                t.failure_count += 1;
            }
        }

        // Failure pattern: extract 3-tool sliding window before failed executions.
        if !is_success && record.tool_executions.len() >= 3 {
            let names: Vec<String> = record
                .tool_executions
                .iter()
                .map(|e| e.tool_name.clone())
                .collect();
            // Take the last 3 tools as the failure-preceding sequence.
            let start = names.len().saturating_sub(3);
            let seq = names[start..].to_vec();
            *failure_sequences.entry(seq).or_insert(0) += 1;
        }
    };

    TrajectoryRecorder::scan(&success_path, &mut process)?;
    TrajectoryRecorder::scan(&failure_path, &mut process)?;

    // Build model stats.
    let model_stats: HashMap<String, ModelStats> = model_agg
        .into_iter()
        .map(|(name, agg)| {
            let runs = agg.runs.max(1);
            (
                name,
                ModelStats {
                    runs: agg.runs,
                    successes: agg.successes,
                    failures: agg.failures,
                    success_rate: agg.successes as f64 / runs as f64,
                    avg_cost_microdollars: agg.total_cost / runs as u64,
                    avg_latency_ms: agg.total_latency / runs as u64,
                    total_cost_microdollars: agg.total_cost,
                },
            )
        })
        .collect();

    // Build tool stats.
    let tool_stats: HashMap<String, ToolStats> = tool_agg
        .into_iter()
        .map(|(name, agg)| {
            let total = agg.total_uses.max(1);
            (
                name,
                ToolStats {
                    total_uses: agg.total_uses,
                    success_count: agg.success_count,
                    failure_count: agg.failure_count,
                    success_rate: agg.success_count as f64 / total as f64,
                },
            )
        })
        .collect();

    // Top failure patterns sorted by frequency.
    let mut failure_patterns: Vec<FailurePattern> = failure_sequences
        .into_iter()
        .map(|(seq, count)| FailurePattern {
            tool_sequence: seq,
            occurrences: count,
        })
        .collect();
    failure_patterns.sort_by(|a, b| b.occurrences.cmp(&a.occurrences));
    failure_patterns.truncate(10);

    let runs_for_avg = total_runs.max(1) as u64;
    Ok(InsightsReport {
        total_runs,
        successful_runs,
        failed_runs,
        partial_runs,
        success_rate: successful_runs as f64 / total_runs.max(1) as f64,
        model_stats,
        tool_stats,
        total_cost_microdollars: total_cost,
        avg_cost_microdollars: total_cost / runs_for_avg,
        total_tokens,
        failure_patterns,
    })
}

#[derive(Default)]
struct ModelAgg {
    runs: usize,
    successes: usize,
    failures: usize,
    total_cost: u64,
    total_latency: u64,
}

#[derive(Default)]
struct ToolAgg {
    total_uses: usize,
    success_count: usize,
    failure_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::{build_record, Outcome, TrajectoryInput, TrajectoryRecorder};
    use agentzero_core::ToolExecutionRecord;

    #[tokio::test]
    async fn empty_report_on_no_trajectories() {
        let dir = tempfile::tempdir().expect("temp dir");
        // Create the trajectories subdir so the recorder can be built.
        std::fs::create_dir_all(dir.path().join("trajectories")).expect("mkdir");
        let report = generate_report(dir.path()).expect("generate");
        assert_eq!(report.total_runs, 0);
        assert_eq!(report.success_rate, 0.0);
    }

    #[tokio::test]
    async fn report_from_mixed_trajectories() {
        let dir = tempfile::tempdir().expect("temp dir");
        let recorder = TrajectoryRecorder::new(dir.path()).expect("recorder");

        // 2 successes
        for i in 0..2 {
            let run_id = format!("run-s{i}");
            recorder
                .record(build_record(TrajectoryInput {
                    session_id: "s1",
                    run_id: &run_id,
                    outcome: Outcome::Success,
                    goal_summary: "succeed",
                    response_text: "ok",
                    tool_executions: &[
                        ToolExecutionRecord {
                            tool_name: "shell".to_string(),
                            success: true,
                            error: None,
                            latency_ms: 100,
                            timestamp: 1000,
                        },
                        ToolExecutionRecord {
                            tool_name: "read_file".to_string(),
                            success: true,
                            error: None,
                            latency_ms: 20,
                            timestamp: 1001,
                        },
                    ],
                    input_tokens: 500,
                    output_tokens: 200,
                    cost_microdollars: 10,
                    model: "opus",
                    latency_ms: 2000,
                }))
                .await
                .expect("record");
        }

        // 1 failure with 3 tools (triggers failure pattern)
        recorder
            .record(build_record(TrajectoryInput {
                session_id: "s1",
                run_id: "run-f0",
                outcome: Outcome::Failure {
                    reason: "error".to_string(),
                },
                goal_summary: "fail",
                response_text: "",
                tool_executions: &[
                    ToolExecutionRecord {
                        tool_name: "shell".to_string(),
                        success: true,
                        error: None,
                        latency_ms: 50,
                        timestamp: 2000,
                    },
                    ToolExecutionRecord {
                        tool_name: "write_file".to_string(),
                        success: false,
                        error: Some("denied".to_string()),
                        latency_ms: 10,
                        timestamp: 2001,
                    },
                    ToolExecutionRecord {
                        tool_name: "shell".to_string(),
                        success: false,
                        error: Some("exit 1".to_string()),
                        latency_ms: 30,
                        timestamp: 2002,
                    },
                ],
                input_tokens: 300,
                output_tokens: 50,
                cost_microdollars: 5,
                model: "sonnet",
                latency_ms: 1000,
            }))
            .await
            .expect("record");

        let report = generate_report(dir.path()).expect("report");
        assert_eq!(report.total_runs, 3);
        assert_eq!(report.successful_runs, 2);
        assert_eq!(report.failed_runs, 1);
        assert!((report.success_rate - 2.0 / 3.0).abs() < 0.01);

        // Model stats
        let opus = &report.model_stats["opus"];
        assert_eq!(opus.runs, 2);
        assert_eq!(opus.successes, 2);
        assert_eq!(opus.success_rate, 1.0);

        let sonnet = &report.model_stats["sonnet"];
        assert_eq!(sonnet.runs, 1);
        assert_eq!(sonnet.failures, 1);

        // Tool stats: 2 success runs × 1 shell each + 1 failure run × 2 shell = 4 total
        let shell = &report.tool_stats["shell"];
        assert_eq!(shell.total_uses, 4);
        assert_eq!(shell.success_count, 3); // 2 from success runs + 1 from failure run
        assert_eq!(shell.failure_count, 1); // 1 failed shell in failure run

        // Failure patterns
        assert!(!report.failure_patterns.is_empty());
        assert_eq!(report.failure_patterns[0].tool_sequence.len(), 3);

        // Cost
        assert_eq!(report.total_cost_microdollars, 25); // 10+10+5
        assert_eq!(report.total_tokens, 1750); // (500+200)*2 + (300+50)
    }
}
