use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Execution mode for an SOP.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SopExecutionMode {
    /// LLM-mediated step transitions (default, existing behavior).
    #[default]
    Supervised,
    /// Deterministic execution -- pipe output of step N to input of step N+1, no LLM.
    Deterministic,
}

/// Kind of step in an SOP.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SopStepKind {
    /// Normal execution step.
    #[default]
    Execute,
    /// Checkpoint requiring human approval before proceeding.
    Checkpoint,
}

/// JSON Schema fragment for step input/output validation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepSchema {
    /// JSON Schema as a Value (e.g., {"type": "object", "properties": {...}})
    pub schema: serde_json::Value,
}

/// Status of an SOP run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SopRunStatus {
    Running,
    PausedCheckpoint { step_index: usize },
    Completed,
    Failed { error: String },
}

/// Persisted state for a deterministic SOP run (for resume capability).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterministicRunState {
    pub plan_id: String,
    pub current_step: usize,
    pub status: SopRunStatus,
    pub step_outputs: HashMap<usize, serde_json::Value>,
    pub started_at: u64,
    pub updated_at: u64,
}

/// Tracks cost savings from deterministic execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeterministicSavings {
    pub llm_calls_saved: u64,
    pub steps_executed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_mode_default_is_supervised() {
        assert_eq!(SopExecutionMode::default(), SopExecutionMode::Supervised);
    }

    #[test]
    fn step_kind_default_is_execute() {
        assert_eq!(SopStepKind::default(), SopStepKind::Execute);
    }

    #[test]
    fn run_status_serializes_correctly() {
        let running = SopRunStatus::Running;
        let json = serde_json::to_string(&running).expect("serialize Running");
        assert_eq!(json, r#""running""#);

        let paused = SopRunStatus::PausedCheckpoint { step_index: 3 };
        let json = serde_json::to_string(&paused).expect("serialize PausedCheckpoint");
        assert!(json.contains("paused_checkpoint"));
        assert!(json.contains("3"));

        let completed = SopRunStatus::Completed;
        let json = serde_json::to_string(&completed).expect("serialize Completed");
        assert_eq!(json, r#""completed""#);

        let failed = SopRunStatus::Failed {
            error: "boom".to_string(),
        };
        let json = serde_json::to_string(&failed).expect("serialize Failed");
        assert!(json.contains("failed"));
        assert!(json.contains("boom"));
    }

    #[test]
    fn deterministic_savings_default_is_zero() {
        let savings = DeterministicSavings::default();
        assert_eq!(savings.llm_calls_saved, 0);
        assert_eq!(savings.steps_executed, 0);
    }

    #[test]
    fn execution_mode_serializes_roundtrip() {
        let modes = [
            SopExecutionMode::Supervised,
            SopExecutionMode::Deterministic,
        ];
        for mode in &modes {
            let json = serde_json::to_string(mode).expect("serialize mode");
            let back: SopExecutionMode = serde_json::from_str(&json).expect("deserialize mode");
            assert_eq!(&back, mode);
        }
    }
}
