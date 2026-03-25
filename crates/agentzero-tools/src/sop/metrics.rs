//! SOP metrics — tracks deterministic execution savings and run durations.

use super::types::DeterministicSavings;
use std::time::Duration;

/// Extended metrics for a completed deterministic SOP run.
#[derive(Debug, Clone, Default)]
pub struct SopRunMetrics {
    /// Savings from deterministic execution (LLM calls avoided).
    pub savings: DeterministicSavings,
    /// Total wall-clock duration of the run.
    pub duration: Duration,
    /// Number of steps that were checkpoints.
    pub checkpoint_count: u64,
    /// Number of checkpoint approvals that arrived.
    pub approvals_received: u64,
}

impl SopRunMetrics {
    /// Record a completed step, incrementing the appropriate counters.
    pub fn record_step(&mut self, is_deterministic: bool) {
        self.savings.steps_executed += 1;
        if is_deterministic {
            self.savings.llm_calls_saved += 1;
        }
    }

    /// Record a checkpoint approval.
    pub fn record_approval(&mut self) {
        self.approvals_received += 1;
    }

    /// Set the total duration from start to finish.
    pub fn set_duration(&mut self, start_epoch: u64, end_epoch: u64) {
        self.duration = Duration::from_secs(end_epoch.saturating_sub(start_epoch));
    }

    /// Format a human-readable summary.
    pub fn summary(&self) -> String {
        format!(
            "{} steps, {} LLM calls saved, {} checkpoints ({} approved), {:.1}s",
            self.savings.steps_executed,
            self.savings.llm_calls_saved,
            self.checkpoint_count,
            self.approvals_received,
            self.duration.as_secs_f64()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_step_increments_counters() {
        let mut m = SopRunMetrics::default();
        m.record_step(true);
        m.record_step(true);
        m.record_step(false);
        assert_eq!(m.savings.steps_executed, 3);
        assert_eq!(m.savings.llm_calls_saved, 2);
    }

    #[test]
    fn record_approval_increments() {
        let mut m = SopRunMetrics::default();
        m.record_approval();
        m.record_approval();
        assert_eq!(m.approvals_received, 2);
    }

    #[test]
    fn set_duration_computes_correctly() {
        let mut m = SopRunMetrics::default();
        m.set_duration(1000, 1045);
        assert_eq!(m.duration, Duration::from_secs(45));
    }

    #[test]
    fn summary_formats_correctly() {
        let mut m = SopRunMetrics::default();
        m.record_step(true);
        m.record_step(true);
        m.checkpoint_count = 1;
        m.record_approval();
        m.set_duration(0, 10);
        let s = m.summary();
        assert!(s.contains("2 steps"));
        assert!(s.contains("2 LLM calls saved"));
        assert!(s.contains("1 checkpoints"));
        assert!(s.contains("1 approved"));
    }
}
