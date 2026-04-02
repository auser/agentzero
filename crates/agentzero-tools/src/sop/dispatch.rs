//! SOP step dispatch — routes step execution based on `SopStepKind`.
//!
//! - `Execute` steps pipe output from the previous step as input, no LLM needed.
//! - `Checkpoint` steps pause execution until human approval arrives.
//! - `Supervised` fallback delegates to the existing LLM agent loop.

use super::types::{SopRunStatus, SopStepKind};
use crate::skills::sop::SopStep;

/// Action to take for a given step in a deterministic run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepAction {
    /// Pipe previous step's output directly — no LLM round-trip.
    DeterministicPipe {
        /// Output from the previous step (used as input for this step).
        previous_output: Option<serde_json::Value>,
    },
    /// Pause for human approval. The run enters `PausedCheckpoint`.
    CheckpointWait { step_index: usize },
    /// Fall back to the supervised (LLM-mediated) path.
    Supervised,
}

/// Determine the dispatch action for a step based on its kind and the execution context.
pub fn dispatch_step(
    step: &SopStep,
    step_index: usize,
    previous_output: Option<serde_json::Value>,
    is_deterministic: bool,
) -> StepAction {
    if !is_deterministic {
        return StepAction::Supervised;
    }

    match step.kind {
        SopStepKind::Execute => StepAction::DeterministicPipe { previous_output },
        SopStepKind::Checkpoint => StepAction::CheckpointWait { step_index },
    }
}

/// Check whether a checkpoint's approval arrived within the timeout window.
pub fn is_checkpoint_expired(paused_at: u64, timeout_secs: u64) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_sub(paused_at) > timeout_secs
}

/// Map a `StepAction` to the resulting `SopRunStatus` after dispatch.
pub fn status_after_dispatch(action: &StepAction) -> Option<SopRunStatus> {
    match action {
        StepAction::CheckpointWait { step_index } => Some(SopRunStatus::PausedCheckpoint {
            step_index: *step_index,
        }),
        StepAction::DeterministicPipe { .. } => Some(SopRunStatus::Running),
        StepAction::Supervised => None, // supervised path handles its own status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn execute_step(title: &str) -> SopStep {
        SopStep {
            title: title.to_string(),
            completed: false,
            kind: SopStepKind::Execute,
            input_schema: None,
            output_schema: None,
            output: None,
        }
    }

    fn checkpoint_step(title: &str) -> SopStep {
        SopStep {
            title: title.to_string(),
            completed: false,
            kind: SopStepKind::Checkpoint,
            input_schema: None,
            output_schema: None,
            output: None,
        }
    }

    #[test]
    fn dispatch_deterministic_execute_returns_pipe() {
        let step = execute_step("build");
        let prev = Some(serde_json::json!({"data": "from previous"}));
        let action = dispatch_step(&step, 1, prev.clone(), true);
        assert_eq!(
            action,
            StepAction::DeterministicPipe {
                previous_output: prev
            }
        );
    }

    #[test]
    fn dispatch_deterministic_checkpoint_returns_wait() {
        let step = checkpoint_step("review");
        let action = dispatch_step(&step, 2, None, true);
        assert_eq!(action, StepAction::CheckpointWait { step_index: 2 });
    }

    #[test]
    fn dispatch_supervised_always_returns_supervised() {
        let step = execute_step("build");
        let action = dispatch_step(&step, 0, None, false);
        assert_eq!(action, StepAction::Supervised);

        let cp = checkpoint_step("review");
        let action = dispatch_step(&cp, 1, None, false);
        assert_eq!(action, StepAction::Supervised);
    }

    #[test]
    fn checkpoint_not_expired_within_window() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_secs();
        assert!(!is_checkpoint_expired(now, 300));
    }

    #[test]
    fn checkpoint_expired_after_window() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_secs();
        // Paused 600 seconds ago, timeout is 300 seconds.
        assert!(is_checkpoint_expired(now.saturating_sub(600), 300));
    }

    #[test]
    fn status_after_dispatch_maps_correctly() {
        assert_eq!(
            status_after_dispatch(&StepAction::CheckpointWait { step_index: 3 }),
            Some(SopRunStatus::PausedCheckpoint { step_index: 3 })
        );
        assert_eq!(
            status_after_dispatch(&StepAction::DeterministicPipe {
                previous_output: None
            }),
            Some(SopRunStatus::Running)
        );
        assert_eq!(status_after_dispatch(&StepAction::Supervised), None);
    }
}
