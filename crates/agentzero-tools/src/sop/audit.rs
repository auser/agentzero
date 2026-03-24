//! SOP audit logging — structured events for step transitions and checkpoint decisions.

use tracing::info;

/// Log a step transition in the SOP execution.
pub fn log_step_transition(plan_id: &str, step_index: usize, step_title: &str, status: &str) {
    info!(
        target: "sop_audit",
        plan_id = plan_id,
        step_index = step_index,
        step_title = step_title,
        status = status,
        "sop step transition"
    );
}

/// Log a checkpoint decision (approved or rejected).
pub fn log_checkpoint_decision(
    plan_id: &str,
    step_index: usize,
    decision: &str,
    reason: Option<&str>,
) {
    info!(
        target: "sop_audit",
        plan_id = plan_id,
        step_index = step_index,
        decision = decision,
        reason = reason.unwrap_or(""),
        "sop checkpoint decision"
    );
}

/// Log a deterministic run lifecycle event.
pub fn log_run_event(plan_id: &str, event: &str, detail: Option<&str>) {
    info!(
        target: "sop_audit",
        plan_id = plan_id,
        event = event,
        detail = detail.unwrap_or(""),
        "sop run event"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_step_transition_does_not_panic() {
        log_step_transition("plan-1", 0, "Build", "completed");
    }

    #[test]
    fn log_checkpoint_decision_does_not_panic() {
        log_checkpoint_decision("plan-1", 2, "approved", Some("looks good"));
        log_checkpoint_decision("plan-1", 3, "rejected", None);
    }

    #[test]
    fn log_run_event_does_not_panic() {
        log_run_event("plan-1", "started", None);
        log_run_event("plan-1", "completed", Some("all steps done"));
    }
}
