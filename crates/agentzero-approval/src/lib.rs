use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub actor_id: String,
    pub action: String,
    pub risk: RiskLevel,
    pub requested_at_epoch_secs: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApprovalDecision {
    pub approved: bool,
    pub approver_id: String,
    pub reason: Option<String>,
    pub decided_at_epoch_secs: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub request: ApprovalRequest,
    pub outcome: ApprovalOutcome,
    pub recorded_at_epoch_secs: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalOutcome {
    AllowedNoApproval,
    AllowedWithApproval,
    BlockedApprovalRequired,
    BlockedDenied,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ApprovalError {
    #[error("approval required for high-risk action")]
    ApprovalRequired,
    #[error("action denied by approver")]
    Denied,
    #[error("invalid request: actor_id must not be empty")]
    EmptyActorId,
    #[error("invalid request: action must not be empty")]
    EmptyAction,
    #[error("invalid decision: approver_id must not be empty")]
    EmptyApproverId,
}

#[derive(Debug, Default)]
pub struct ApprovalEngine {
    audit_log: Vec<AuditEntry>,
}

impl ApprovalEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn evaluate(
        &mut self,
        request: ApprovalRequest,
        decision: Option<ApprovalDecision>,
    ) -> Result<ApprovalOutcome, ApprovalError> {
        validate_request(&request)?;

        let requires_approval = matches!(request.risk, RiskLevel::High | RiskLevel::Critical);
        if !requires_approval {
            let outcome = ApprovalOutcome::AllowedNoApproval;
            self.record(request, outcome);
            return Ok(outcome);
        }

        let Some(decision) = decision else {
            let outcome = ApprovalOutcome::BlockedApprovalRequired;
            self.record(request, outcome);
            return Err(ApprovalError::ApprovalRequired);
        };

        validate_decision(&decision)?;
        if decision.approved {
            let outcome = ApprovalOutcome::AllowedWithApproval;
            self.record(request, outcome);
            Ok(outcome)
        } else {
            let outcome = ApprovalOutcome::BlockedDenied;
            self.record(request, outcome);
            Err(ApprovalError::Denied)
        }
    }

    pub fn audit_log(&self) -> &[AuditEntry] {
        &self.audit_log
    }

    fn record(&mut self, request: ApprovalRequest, outcome: ApprovalOutcome) {
        self.audit_log.push(AuditEntry {
            request,
            outcome,
            recorded_at_epoch_secs: now_epoch_secs(),
        });
    }
}

impl ApprovalRequest {
    pub fn new(actor_id: &str, action: &str, risk: RiskLevel) -> Result<Self, ApprovalError> {
        let request = Self {
            actor_id: actor_id.trim().to_string(),
            action: action.trim().to_string(),
            risk,
            requested_at_epoch_secs: now_epoch_secs(),
        };
        validate_request(&request)?;
        Ok(request)
    }
}

impl ApprovalDecision {
    pub fn allow(approver_id: &str, reason: Option<&str>) -> Result<Self, ApprovalError> {
        let decision = Self {
            approved: true,
            approver_id: approver_id.trim().to_string(),
            reason: reason.map(str::to_string),
            decided_at_epoch_secs: now_epoch_secs(),
        };
        validate_decision(&decision)?;
        Ok(decision)
    }

    pub fn deny(approver_id: &str, reason: Option<&str>) -> Result<Self, ApprovalError> {
        let decision = Self {
            approved: false,
            approver_id: approver_id.trim().to_string(),
            reason: reason.map(str::to_string),
            decided_at_epoch_secs: now_epoch_secs(),
        };
        validate_decision(&decision)?;
        Ok(decision)
    }
}

fn validate_request(request: &ApprovalRequest) -> Result<(), ApprovalError> {
    if request.actor_id.trim().is_empty() {
        return Err(ApprovalError::EmptyActorId);
    }
    if request.action.trim().is_empty() {
        return Err(ApprovalError::EmptyAction);
    }
    Ok(())
}

fn validate_decision(decision: &ApprovalDecision) -> Result<(), ApprovalError> {
    if decision.approver_id.trim().is_empty() {
        return Err(ApprovalError::EmptyApproverId);
    }
    Ok(())
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_risk_action_allows_without_approval_success_path() {
        let mut engine = ApprovalEngine::new();
        let request = ApprovalRequest::new("operator-1", "read_status", RiskLevel::Low)
            .expect("request should be valid");

        let outcome = engine
            .evaluate(request, None)
            .expect("low risk should not require approval");

        assert_eq!(outcome, ApprovalOutcome::AllowedNoApproval);
        assert_eq!(engine.audit_log().len(), 1);
        assert_eq!(
            engine.audit_log()[0].outcome,
            ApprovalOutcome::AllowedNoApproval
        );
    }

    #[test]
    fn high_risk_requires_explicit_approval_negative_path() {
        let mut engine = ApprovalEngine::new();
        let request = ApprovalRequest::new("operator-1", "run_shell", RiskLevel::High)
            .expect("request should be valid");

        let err = engine
            .evaluate(request, None)
            .expect_err("high risk without approval should fail");

        assert_eq!(err, ApprovalError::ApprovalRequired);
        assert_eq!(engine.audit_log().len(), 1);
        assert_eq!(
            engine.audit_log()[0].outcome,
            ApprovalOutcome::BlockedApprovalRequired
        );
    }

    #[test]
    fn critical_action_with_approved_decision_allows_success_path() {
        let mut engine = ApprovalEngine::new();
        let request = ApprovalRequest::new("operator-1", "write_system_file", RiskLevel::Critical)
            .expect("request should be valid");
        let decision = ApprovalDecision::allow("approver-1", Some("maintenance window"))
            .expect("decision should be valid");

        let outcome = engine
            .evaluate(request, Some(decision))
            .expect("approved decision should allow execution");

        assert_eq!(outcome, ApprovalOutcome::AllowedWithApproval);
        assert_eq!(engine.audit_log().len(), 1);
        assert_eq!(
            engine.audit_log()[0].outcome,
            ApprovalOutcome::AllowedWithApproval
        );
    }

    #[test]
    fn denied_decision_blocks_high_risk_negative_path() {
        let mut engine = ApprovalEngine::new();
        let request = ApprovalRequest::new("operator-1", "wipe_data", RiskLevel::High)
            .expect("request should be valid");
        let decision = ApprovalDecision::deny("approver-1", Some("policy violation"))
            .expect("decision should be valid");

        let err = engine
            .evaluate(request, Some(decision))
            .expect_err("denied decision should block execution");

        assert_eq!(err, ApprovalError::Denied);
        assert_eq!(engine.audit_log().len(), 1);
        assert_eq!(
            engine.audit_log()[0].outcome,
            ApprovalOutcome::BlockedDenied
        );
    }

    #[test]
    fn empty_approver_is_rejected_negative_path() {
        let err =
            ApprovalDecision::allow("  ", None).expect_err("blank approver should be rejected");
        assert_eq!(err, ApprovalError::EmptyApproverId);
    }
}
