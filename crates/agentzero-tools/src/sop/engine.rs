use super::types::*;
use crate::skills::sop::SopPlan;
use std::collections::HashMap;
use std::path::Path;

/// Deterministic SOP execution engine.
///
/// Executes steps sequentially, piping the output of step N as the input
/// to step N+1. Checkpoints pause execution until approved.
pub struct SopEngine;

impl SopEngine {
    /// Start a new deterministic run for the given plan.
    pub fn start_deterministic_run(plan: &SopPlan) -> DeterministicRunState {
        let now = now_secs();
        DeterministicRunState {
            plan_id: plan.id.clone(),
            current_step: 0,
            status: SopRunStatus::Running,
            step_outputs: HashMap::new(),
            started_at: now,
            updated_at: now,
        }
    }

    /// Advance to the next step in a deterministic run.
    /// `step_output` is the output from the current step, piped as input to the next.
    /// Returns the new status after advancement.
    pub fn advance_deterministic_step(
        state: &mut DeterministicRunState,
        plan: &SopPlan,
        step_output: serde_json::Value,
        step_kinds: &[SopStepKind],
    ) -> anyhow::Result<SopRunStatus> {
        if state.current_step >= plan.steps.len() {
            anyhow::bail!("all steps already completed");
        }

        // Store the output of the current step
        state.step_outputs.insert(state.current_step, step_output);
        state.current_step += 1;
        state.updated_at = now_secs();

        // Check if we're done
        if state.current_step >= plan.steps.len() {
            state.status = SopRunStatus::Completed;
            return Ok(state.status.clone());
        }

        // Check if the next step is a checkpoint
        let next_kind = step_kinds
            .get(state.current_step)
            .unwrap_or(&SopStepKind::Execute);
        if *next_kind == SopStepKind::Checkpoint {
            state.status = SopRunStatus::PausedCheckpoint {
                step_index: state.current_step,
            };
            return Ok(state.status.clone());
        }

        state.status = SopRunStatus::Running;
        Ok(state.status.clone())
    }

    /// Resume a paused deterministic run (after checkpoint approval).
    pub fn resume_deterministic_run(
        state: &mut DeterministicRunState,
    ) -> anyhow::Result<SopRunStatus> {
        match &state.status {
            SopRunStatus::PausedCheckpoint { .. } => {
                state.status = SopRunStatus::Running;
                state.updated_at = now_secs();
                Ok(state.status.clone())
            }
            other => anyhow::bail!("cannot resume: run is {:?}", other),
        }
    }

    /// Persist deterministic run state to workspace directory.
    pub async fn persist_state(
        state: &DeterministicRunState,
        workspace_root: &str,
    ) -> anyhow::Result<()> {
        let dir = Path::new(workspace_root)
            .join(".agentzero")
            .join("sop_runs");
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(format!("{}.json", state.plan_id));
        let data = serde_json::to_string_pretty(state)?;
        tokio::fs::write(path, data).await?;
        Ok(())
    }

    /// Load persisted deterministic run state.
    pub async fn load_state(
        plan_id: &str,
        workspace_root: &str,
    ) -> anyhow::Result<Option<DeterministicRunState>> {
        let path = Path::new(workspace_root)
            .join(".agentzero")
            .join("sop_runs")
            .join(format!("{plan_id}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let data = tokio::fs::read_to_string(path).await?;
        let state: DeterministicRunState = serde_json::from_str(&data)?;
        Ok(Some(state))
    }

    /// Calculate savings from a deterministic run.
    pub fn calculate_savings(state: &DeterministicRunState) -> DeterministicSavings {
        // Each step that ran deterministically saved one LLM transition call
        let steps_completed = state.step_outputs.len() as u64;
        DeterministicSavings {
            llm_calls_saved: steps_completed.saturating_sub(1), // First step doesn't save an LLM call
            steps_executed: steps_completed,
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::sop;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-sop-engine-{}-{nanos}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn make_plan(steps: &[&str]) -> SopPlan {
        sop::create_plan("test-plan", steps).expect("plan creation should succeed")
    }

    #[test]
    fn start_deterministic_run_initializes_correctly() {
        let plan = make_plan(&["a", "b", "c"]);
        let state = SopEngine::start_deterministic_run(&plan);

        assert_eq!(state.plan_id, "test-plan");
        assert_eq!(state.current_step, 0);
        assert_eq!(state.status, SopRunStatus::Running);
        assert!(state.step_outputs.is_empty());
        assert!(state.started_at > 0);
        assert_eq!(state.started_at, state.updated_at);
    }

    #[test]
    fn advance_step_stores_output_and_increments() {
        let plan = make_plan(&["a", "b", "c"]);
        let mut state = SopEngine::start_deterministic_run(&plan);
        let kinds = vec![SopStepKind::Execute; 3];

        let status = SopEngine::advance_deterministic_step(
            &mut state,
            &plan,
            serde_json::json!({"result": "done_a"}),
            &kinds,
        )
        .expect("advance should succeed");

        assert_eq!(status, SopRunStatus::Running);
        assert_eq!(state.current_step, 1);
        assert_eq!(
            state.step_outputs.get(&0),
            Some(&serde_json::json!({"result": "done_a"}))
        );
    }

    #[test]
    fn advance_past_last_step_completes() {
        let plan = make_plan(&["only"]);
        let mut state = SopEngine::start_deterministic_run(&plan);
        let kinds = vec![SopStepKind::Execute];

        let status = SopEngine::advance_deterministic_step(
            &mut state,
            &plan,
            serde_json::json!("output"),
            &kinds,
        )
        .expect("advance should succeed");

        assert_eq!(status, SopRunStatus::Completed);
        assert_eq!(state.current_step, 1);
    }

    #[test]
    fn checkpoint_pauses_execution() {
        let plan = make_plan(&["build", "approve", "deploy"]);
        let mut state = SopEngine::start_deterministic_run(&plan);
        let kinds = vec![
            SopStepKind::Execute,
            SopStepKind::Checkpoint,
            SopStepKind::Execute,
        ];

        // Advance past step 0 -> next step is checkpoint
        let status = SopEngine::advance_deterministic_step(
            &mut state,
            &plan,
            serde_json::json!("built"),
            &kinds,
        )
        .expect("advance should succeed");

        assert_eq!(status, SopRunStatus::PausedCheckpoint { step_index: 1 });
    }

    #[test]
    fn resume_from_checkpoint() {
        let plan = make_plan(&["build", "approve", "deploy"]);
        let mut state = SopEngine::start_deterministic_run(&plan);
        let kinds = vec![
            SopStepKind::Execute,
            SopStepKind::Checkpoint,
            SopStepKind::Execute,
        ];

        // Advance to checkpoint
        SopEngine::advance_deterministic_step(
            &mut state,
            &plan,
            serde_json::json!("built"),
            &kinds,
        )
        .expect("advance should succeed");

        // Resume
        let status =
            SopEngine::resume_deterministic_run(&mut state).expect("resume should succeed");
        assert_eq!(status, SopRunStatus::Running);
    }

    #[test]
    fn resume_non_paused_fails() {
        let plan = make_plan(&["a"]);
        let mut state = SopEngine::start_deterministic_run(&plan);

        let err = SopEngine::resume_deterministic_run(&mut state)
            .expect_err("resume running should fail");
        assert!(err.to_string().contains("cannot resume"));
    }

    #[test]
    fn advance_completed_run_fails() {
        let plan = make_plan(&["only"]);
        let mut state = SopEngine::start_deterministic_run(&plan);
        let kinds = vec![SopStepKind::Execute];

        // Complete the run
        SopEngine::advance_deterministic_step(&mut state, &plan, serde_json::json!("done"), &kinds)
            .expect("advance should succeed");

        // Try to advance again
        let err = SopEngine::advance_deterministic_step(
            &mut state,
            &plan,
            serde_json::json!("extra"),
            &kinds,
        )
        .expect_err("advance past end should fail");
        assert!(err.to_string().contains("already completed"));
    }

    #[test]
    fn calculate_savings_correct() {
        let plan = make_plan(&["a", "b", "c", "d"]);
        let mut state = SopEngine::start_deterministic_run(&plan);
        let kinds = vec![SopStepKind::Execute; 4];

        // Execute 3 steps
        for i in 0..3 {
            SopEngine::advance_deterministic_step(
                &mut state,
                &plan,
                serde_json::json!(format!("output_{i}")),
                &kinds,
            )
            .expect("advance should succeed");
        }

        let savings = SopEngine::calculate_savings(&state);
        assert_eq!(savings.steps_executed, 3);
        // First step doesn't save an LLM call, so 3 - 1 = 2
        assert_eq!(savings.llm_calls_saved, 2);
    }

    #[tokio::test]
    async fn persist_and_load_roundtrip() {
        let dir = temp_dir();
        let workspace = dir.to_string_lossy().to_string();

        let plan = make_plan(&["a", "b"]);
        let mut state = SopEngine::start_deterministic_run(&plan);
        let kinds = vec![SopStepKind::Execute; 2];

        SopEngine::advance_deterministic_step(
            &mut state,
            &plan,
            serde_json::json!({"key": "value"}),
            &kinds,
        )
        .expect("advance should succeed");

        // Persist
        SopEngine::persist_state(&state, &workspace)
            .await
            .expect("persist should succeed");

        // Load
        let loaded = SopEngine::load_state("test-plan", &workspace)
            .await
            .expect("load should succeed")
            .expect("state should exist");

        assert_eq!(loaded.plan_id, state.plan_id);
        assert_eq!(loaded.current_step, state.current_step);
        assert_eq!(loaded.status, state.status);
        assert_eq!(loaded.step_outputs.len(), state.step_outputs.len());
        assert_eq!(
            loaded.step_outputs.get(&0),
            Some(&serde_json::json!({"key": "value"}))
        );

        // Load non-existent
        let missing = SopEngine::load_state("no-such-plan", &workspace)
            .await
            .expect("load should succeed");
        assert!(missing.is_none());

        std::fs::remove_dir_all(dir).ok();
    }
}
