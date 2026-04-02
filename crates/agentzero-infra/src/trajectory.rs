//! Session-level trajectory recording for self-improving learning.
//!
//! After every agent run, [`TrajectoryRecorder`] captures the full session
//! (goal, outcome, tool executions, token usage, cost) as an append-only
//! JSONL record. Successful and failed runs go to separate files so the
//! insights engine can analyze patterns without loading everything into memory.

use agentzero_core::ToolExecutionRecord;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

/// Outcome classification for a completed agent run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Outcome {
    /// Agent completed normally (EndTurn or StopSequence).
    Success,
    /// Agent failed (timeout, loop detection, error).
    Failure { reason: String },
    /// Agent produced output but hit a soft limit (e.g. max iterations warning).
    Partial { reason: String },
}

/// A single trajectory record capturing a complete agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryRecord {
    pub session_id: String,
    pub run_id: String,
    pub timestamp_ms: u64,
    pub outcome: Outcome,
    pub goal_summary: String,
    pub response_text: String,
    pub tool_executions: Vec<ToolExecutionRecord>,
    /// Total input tokens across all LLM calls in this run.
    pub input_tokens: u64,
    /// Total output tokens across all LLM calls in this run.
    pub output_tokens: u64,
    /// Total cost in micro-dollars.
    pub cost_microdollars: u64,
    pub model: String,
    /// Wall-clock duration of the entire run.
    pub latency_ms: u64,
    /// Auto-derived tags: tool names used, model name, outcome type.
    pub tags: Vec<String>,
}

/// Append-only trajectory recorder with separate files for success/failure.
pub struct TrajectoryRecorder {
    success_path: PathBuf,
    failure_path: PathBuf,
}

impl TrajectoryRecorder {
    /// Create a recorder writing to `<dir>/trajectories/successful.jsonl`
    /// and `<dir>/trajectories/failed.jsonl`.
    pub fn new(data_dir: &Path) -> anyhow::Result<Self> {
        let traj_dir = data_dir.join("trajectories");
        std::fs::create_dir_all(&traj_dir)
            .with_context(|| format!("failed to create trajectory dir {}", traj_dir.display()))?;
        Ok(Self {
            success_path: traj_dir.join("successful.jsonl"),
            failure_path: traj_dir.join("failed.jsonl"),
        })
    }

    /// Record a trajectory. Writes are atomic via `spawn_blocking` + `O_APPEND`.
    pub async fn record(&self, record: TrajectoryRecord) -> anyhow::Result<()> {
        let path = match &record.outcome {
            Outcome::Success => self.success_path.clone(),
            Outcome::Failure { .. } | Outcome::Partial { .. } => self.failure_path.clone(),
        };

        let mut line =
            serde_json::to_string(&record).context("failed to serialize trajectory record")?;
        line.push('\n');

        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("failed to open trajectory file {}", path.display()))?;
            file.write_all(line.as_bytes())
                .context("failed to write trajectory record")?;
            Ok(())
        })
        .await
        .context("trajectory write task panicked")??;
        Ok(())
    }

    /// Iterate over records from a JSONL file, calling `f` for each valid record.
    /// Invalid lines are skipped with a warning. Returns the count of records processed.
    pub fn scan<F>(path: &Path, mut f: F) -> anyhow::Result<usize>
    where
        F: FnMut(TrajectoryRecord),
    {
        use std::io::BufRead;
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e).with_context(|| format!("failed to open {}", path.display())),
        };
        let reader = std::io::BufReader::new(file);
        let mut count = 0;
        for (i, line) in reader.lines().enumerate() {
            let line = line
                .with_context(|| format!("failed to read line {} of {}", i + 1, path.display()))?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<TrajectoryRecord>(&line) {
                Ok(record) => {
                    f(record);
                    count += 1;
                }
                Err(e) => {
                    warn!(
                        line = i + 1,
                        error = %e,
                        file = %path.display(),
                        "skipping malformed trajectory record"
                    );
                }
            }
        }
        Ok(count)
    }

    /// Path to the successful trajectories file.
    pub fn success_path(&self) -> &Path {
        &self.success_path
    }

    /// Path to the failed trajectories file.
    pub fn failure_path(&self) -> &Path {
        &self.failure_path
    }
}

/// Input data for building a trajectory record.
pub struct TrajectoryInput<'a> {
    pub session_id: &'a str,
    pub run_id: &'a str,
    pub outcome: Outcome,
    pub goal_summary: &'a str,
    pub response_text: &'a str,
    pub tool_executions: &'a [ToolExecutionRecord],
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_microdollars: u64,
    pub model: &'a str,
    pub latency_ms: u64,
}

/// Build a `TrajectoryRecord` from post-run data.
/// Tags are auto-derived from tool names + model + outcome.
pub fn build_record(input: TrajectoryInput<'_>) -> TrajectoryRecord {
    let mut tags: Vec<String> = input
        .tool_executions
        .iter()
        .map(|r| r.tool_name.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    tags.sort();
    tags.push(format!("model:{}", input.model));
    match &input.outcome {
        Outcome::Success => tags.push("outcome:success".to_string()),
        Outcome::Failure { .. } => tags.push("outcome:failure".to_string()),
        Outcome::Partial { .. } => tags.push("outcome:partial".to_string()),
    }

    let ts_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    TrajectoryRecord {
        session_id: input.session_id.to_string(),
        run_id: input.run_id.to_string(),
        timestamp_ms: ts_ms,
        outcome: input.outcome,
        goal_summary: input.goal_summary.to_string(),
        response_text: input.response_text.to_string(),
        tool_executions: input.tool_executions.to_vec(),
        input_tokens: input.input_tokens,
        output_tokens: input.output_tokens,
        cost_microdollars: input.cost_microdollars,
        model: input.model.to_string(),
        latency_ms: input.latency_ms,
        tags,
    }
}

/// Generate a session-end summary of learnings from the conversation.
///
/// Feeds the goal + response + tool executions to a cheap LLM with a
/// structured prompt asking for learnings, preferences, and mistakes.
/// The output can be appended to persistent memory.
pub async fn summarize_session_learnings(
    provider: &dyn agentzero_core::Provider,
    goal: &str,
    response: &str,
    tool_executions: &[ToolExecutionRecord],
    timeout_secs: u64,
) -> Option<String> {
    let tools_used: Vec<&str> = tool_executions
        .iter()
        .map(|t| t.tool_name.as_str())
        .collect();
    let failures: Vec<String> = tool_executions
        .iter()
        .filter(|t| !t.success)
        .map(|t| {
            format!(
                "{}: {}",
                t.tool_name,
                t.error.as_deref().unwrap_or("unknown error")
            )
        })
        .collect();

    let prompt = format!(
        "Analyze this completed agent session and extract learnings worth remembering.\n\n\
         Goal: {goal}\n\
         Response summary: {}\n\
         Tools used: {}\n\
         Failures: {}\n\n\
         Output a concise summary (under 500 chars) of:\n\
         1. What worked well\n\
         2. What failed and why\n\
         3. User preferences or patterns to remember\n\
         4. Mistakes to avoid next time\n\n\
         Only include non-obvious learnings. Skip if the session was routine.",
        if response.len() > 500 {
            &response[..500]
        } else {
            response
        },
        tools_used.join(", "),
        if failures.is_empty() {
            "none".to_string()
        } else {
            failures.join("; ")
        },
    );

    let timeout = std::time::Duration::from_secs(timeout_secs);
    match tokio::time::timeout(timeout, provider.complete(&prompt)).await {
        Ok(Ok(result)) => {
            let summary = result.output_text.trim().to_string();
            if summary.is_empty() || summary.len() < 20 {
                None
            } else {
                Some(summary)
            }
        }
        Ok(Err(e)) => {
            warn!(error = %e, "session summarization failed");
            None
        }
        Err(_) => {
            warn!("session summarization timed out");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolExecutionRecord;

    fn sample_executions() -> Vec<ToolExecutionRecord> {
        vec![
            ToolExecutionRecord {
                tool_name: "shell".to_string(),
                success: true,
                error: None,
                latency_ms: 150,
                timestamp: 1000,
            },
            ToolExecutionRecord {
                tool_name: "read_file".to_string(),
                success: true,
                error: None,
                latency_ms: 20,
                timestamp: 1001,
            },
            ToolExecutionRecord {
                tool_name: "shell".to_string(),
                success: false,
                error: Some("exit code 1".to_string()),
                latency_ms: 300,
                timestamp: 1002,
            },
        ]
    }

    #[test]
    fn build_record_auto_tags() {
        let record = build_record(TrajectoryInput {
            session_id: "sess-1",
            run_id: "run-1",
            outcome: Outcome::Success,
            goal_summary: "deploy the app",
            response_text: "Done!",
            tool_executions: &sample_executions(),
            input_tokens: 1000,
            output_tokens: 500,
            cost_microdollars: 42,
            model: "claude-sonnet-4-6",
            latency_ms: 3500,
        });

        assert!(record.tags.contains(&"shell".to_string()));
        assert!(record.tags.contains(&"read_file".to_string()));
        assert!(record.tags.contains(&"model:claude-sonnet-4-6".to_string()));
        assert!(record.tags.contains(&"outcome:success".to_string()));
        assert_eq!(record.tool_executions.len(), 3);
    }

    #[test]
    fn build_record_failure_tags() {
        let record = build_record(TrajectoryInput {
            session_id: "sess-2",
            run_id: "run-2",
            outcome: Outcome::Failure {
                reason: "loop detected".to_string(),
            },
            goal_summary: "fix the bug",
            response_text: "",
            tool_executions: &[],
            input_tokens: 200,
            output_tokens: 100,
            cost_microdollars: 5,
            model: "claude-haiku-4-5-20251001",
            latency_ms: 1200,
        });
        assert!(record.tags.contains(&"outcome:failure".to_string()));
        assert!(record
            .tags
            .contains(&"model:claude-haiku-4-5-20251001".to_string()));
    }

    #[tokio::test]
    async fn records_to_success_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let recorder = TrajectoryRecorder::new(dir.path()).expect("create recorder");

        let record = build_record(TrajectoryInput {
            session_id: "sess-1",
            run_id: "run-1",
            outcome: Outcome::Success,
            goal_summary: "test goal",
            response_text: "response",
            tool_executions: &sample_executions(),
            input_tokens: 100,
            output_tokens: 50,
            cost_microdollars: 10,
            model: "test-model",
            latency_ms: 500,
        });
        recorder
            .record(record)
            .await
            .expect("record should succeed");

        let content =
            std::fs::read_to_string(recorder.success_path()).expect("should read success file");
        assert!(!content.is_empty());
        let parsed: TrajectoryRecord =
            serde_json::from_str(content.trim()).expect("should parse as TrajectoryRecord");
        assert_eq!(parsed.session_id, "sess-1");
        assert_eq!(parsed.goal_summary, "test goal");
        assert!(matches!(parsed.outcome, Outcome::Success));
    }

    #[tokio::test]
    async fn records_to_failure_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let recorder = TrajectoryRecorder::new(dir.path()).expect("create recorder");

        let record = build_record(TrajectoryInput {
            session_id: "sess-2",
            run_id: "run-2",
            outcome: Outcome::Failure {
                reason: "timeout".to_string(),
            },
            goal_summary: "broken goal",
            response_text: "",
            tool_executions: &[],
            input_tokens: 0,
            output_tokens: 0,
            cost_microdollars: 0,
            model: "test-model",
            latency_ms: 120000,
        });
        recorder
            .record(record)
            .await
            .expect("record should succeed");

        assert!(
            !std::fs::metadata(recorder.success_path())
                .map(|m| m.len() > 0)
                .unwrap_or(false),
            "success file should not exist or be empty"
        );
        let content =
            std::fs::read_to_string(recorder.failure_path()).expect("should read failure file");
        assert!(content.contains("timeout"));
    }

    #[tokio::test]
    async fn scan_reads_records() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let recorder = TrajectoryRecorder::new(dir.path()).expect("create recorder");

        for i in 0..5 {
            let run_id = format!("run-{i}");
            let goal = format!("goal {i}");
            let record = build_record(TrajectoryInput {
                session_id: "sess-1",
                run_id: &run_id,
                outcome: Outcome::Success,
                goal_summary: &goal,
                response_text: "ok",
                tool_executions: &[],
                input_tokens: 10,
                output_tokens: 5,
                cost_microdollars: 1,
                model: "test-model",
                latency_ms: 100,
            });
            recorder.record(record).await.expect("record");
        }

        let mut records = Vec::new();
        let count = TrajectoryRecorder::scan(recorder.success_path(), |r| records.push(r))
            .expect("scan should succeed");
        assert_eq!(count, 5);
        assert_eq!(records.len(), 5);
        assert_eq!(records[0].run_id, "run-0");
        assert_eq!(records[4].run_id, "run-4");
    }

    #[test]
    fn scan_nonexistent_file_returns_zero() {
        let count = TrajectoryRecorder::scan(Path::new("/tmp/nonexistent_traj.jsonl"), |_| {})
            .expect("should succeed");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn scan_skips_malformed_lines() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("mixed.jsonl");

        let record = build_record(TrajectoryInput {
            session_id: "sess-1",
            run_id: "run-1",
            outcome: Outcome::Success,
            goal_summary: "good goal",
            response_text: "ok",
            tool_executions: &[],
            input_tokens: 10,
            output_tokens: 5,
            cost_microdollars: 1,
            model: "test",
            latency_ms: 100,
        });
        let good_line = serde_json::to_string(&record).expect("serialize");

        std::fs::write(
            &path,
            format!("{good_line}\n{{\"bad\": true}}\n{good_line}\n"),
        )
        .expect("write mixed file");

        let mut records = Vec::new();
        let count =
            TrajectoryRecorder::scan(&path, |r| records.push(r)).expect("scan should succeed");
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn partial_outcome_goes_to_failure_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let recorder = TrajectoryRecorder::new(dir.path()).expect("create recorder");

        let record = build_record(TrajectoryInput {
            session_id: "sess-3",
            run_id: "run-3",
            outcome: Outcome::Partial {
                reason: "max iterations".to_string(),
            },
            goal_summary: "partial goal",
            response_text: "partial response",
            tool_executions: &[],
            input_tokens: 50,
            output_tokens: 25,
            cost_microdollars: 3,
            model: "test-model",
            latency_ms: 5000,
        });
        recorder
            .record(record)
            .await
            .expect("record should succeed");

        let content =
            std::fs::read_to_string(recorder.failure_path()).expect("should read failure file");
        assert!(content.contains("max iterations"));
    }

    #[tokio::test]
    async fn concurrent_writes_all_recorded() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let recorder =
            std::sync::Arc::new(TrajectoryRecorder::new(dir.path()).expect("create recorder"));

        let mut handles = Vec::new();
        for i in 0..20 {
            let rec = recorder.clone();
            handles.push(tokio::spawn(async move {
                let run_id = format!("run-{i}");
                let goal = format!("goal {i}");
                let record = build_record(TrajectoryInput {
                    session_id: "sess-c",
                    run_id: &run_id,
                    outcome: Outcome::Success,
                    goal_summary: &goal,
                    response_text: "ok",
                    tool_executions: &[],
                    input_tokens: 10,
                    output_tokens: 5,
                    cost_microdollars: 1,
                    model: "test",
                    latency_ms: 100,
                });
                rec.record(record).await.expect("concurrent record");
            }));
        }
        for h in handles {
            h.await.expect("task should complete");
        }

        let mut count = 0;
        TrajectoryRecorder::scan(recorder.success_path(), |_| count += 1)
            .expect("scan should succeed");
        assert_eq!(count, 20);
    }
}
