use serde::{Deserialize, Serialize};

use crate::config::AutopilotConfig;
use crate::store::AutopilotStore;
use crate::types::Proposal;

/// Result of a cap gate check.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum CapGateResult {
    Approved,
    Rejected { reason: String },
}

impl CapGateResult {
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }
}

/// Resource constraint enforcer that rejects proposals at entry when limits
/// are violated, preventing queue buildup.
#[derive(Debug, Clone)]
pub struct CapGate {
    pub max_daily_spend_microdollars: u64,
    pub max_concurrent_missions: usize,
    pub max_proposals_per_hour: usize,
    pub max_missions_per_agent_per_day: usize,
}

impl CapGate {
    pub fn from_config(config: &AutopilotConfig) -> Self {
        Self {
            max_daily_spend_microdollars: config.max_daily_spend_cents * 10_000,
            max_concurrent_missions: config.max_concurrent_missions,
            max_proposals_per_hour: config.max_proposals_per_hour,
            max_missions_per_agent_per_day: config.max_missions_per_agent_per_day,
        }
    }

    /// Check whether a proposal should be approved based on current resource usage.
    pub async fn check(
        &self,
        proposal: &Proposal,
        client: &dyn AutopilotStore,
    ) -> anyhow::Result<CapGateResult> {
        // Check daily spend
        let daily_spend = client.get_daily_spend().await?;
        let projected = daily_spend + proposal.estimated_cost_microdollars;
        if projected > self.max_daily_spend_microdollars {
            return Ok(CapGateResult::Rejected {
                reason: format!(
                    "daily spend would be {} microdollars (limit: {})",
                    projected, self.max_daily_spend_microdollars
                ),
            });
        }

        // Check concurrent missions
        let concurrent = client.get_concurrent_mission_count().await?;
        if concurrent >= self.max_concurrent_missions {
            return Ok(CapGateResult::Rejected {
                reason: format!(
                    "concurrent missions at capacity: {} (limit: {})",
                    concurrent, self.max_concurrent_missions
                ),
            });
        }

        Ok(CapGateResult::Approved)
    }

    /// Offline check that does not require Supabase. Useful for tests and
    /// quick validation against known state.
    pub fn check_offline(
        &self,
        proposal: &Proposal,
        current_daily_spend: u64,
        current_concurrent_missions: usize,
    ) -> CapGateResult {
        let projected = current_daily_spend + proposal.estimated_cost_microdollars;
        if projected > self.max_daily_spend_microdollars {
            return CapGateResult::Rejected {
                reason: format!(
                    "daily spend would be {} microdollars (limit: {})",
                    projected, self.max_daily_spend_microdollars
                ),
            };
        }

        if current_concurrent_missions >= self.max_concurrent_missions {
            return CapGateResult::Rejected {
                reason: format!(
                    "concurrent missions at capacity: {} (limit: {})",
                    current_concurrent_missions, self.max_concurrent_missions
                ),
            };
        }

        CapGateResult::Approved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Priority, ProposalType};

    fn test_gate() -> CapGate {
        CapGate {
            max_daily_spend_microdollars: 5_000_000, // $5.00
            max_concurrent_missions: 3,
            max_proposals_per_hour: 10,
            max_missions_per_agent_per_day: 5,
        }
    }

    fn test_proposal(cost: u64) -> Proposal {
        Proposal::new(
            "editor",
            "Test Proposal",
            "desc",
            ProposalType::ContentIdea,
            Priority::Medium,
            cost,
        )
    }

    #[test]
    fn approve_under_limits() {
        let gate = test_gate();
        let proposal = test_proposal(100_000); // $0.10
        let result = gate.check_offline(&proposal, 1_000_000, 1);
        assert!(result.is_approved());
    }

    #[test]
    fn reject_over_daily_spend() {
        let gate = test_gate();
        let proposal = test_proposal(100_000);
        let result = gate.check_offline(&proposal, 4_950_000, 0);
        assert!(!result.is_approved());
        match result {
            CapGateResult::Rejected { reason } => {
                assert!(reason.contains("daily spend"));
            }
            _ => panic!("expected rejection"),
        }
    }

    #[test]
    fn reject_at_mission_capacity() {
        let gate = test_gate();
        let proposal = test_proposal(1000);
        let result = gate.check_offline(&proposal, 0, 3);
        assert!(!result.is_approved());
        match result {
            CapGateResult::Rejected { reason } => {
                assert!(reason.contains("concurrent missions"));
            }
            _ => panic!("expected rejection"),
        }
    }

    #[test]
    fn approve_at_exact_boundary() {
        let gate = test_gate();
        // Exactly at the limit — should still approve (projected == max)
        let proposal = test_proposal(0);
        let result = gate.check_offline(&proposal, 5_000_000, 2);
        assert!(result.is_approved());
    }

    #[test]
    fn reject_one_over_boundary() {
        let gate = test_gate();
        let proposal = test_proposal(1);
        let result = gate.check_offline(&proposal, 5_000_000, 0);
        assert!(!result.is_approved());
    }

    #[test]
    fn from_config() {
        let config = AutopilotConfig {
            max_daily_spend_cents: 100,
            max_concurrent_missions: 7,
            max_proposals_per_hour: 15,
            max_missions_per_agent_per_day: 8,
            ..Default::default()
        };
        let gate = CapGate::from_config(&config);
        assert_eq!(gate.max_daily_spend_microdollars, 1_000_000);
        assert_eq!(gate.max_concurrent_missions, 7);
    }
}
