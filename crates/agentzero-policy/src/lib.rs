//! Policy evaluation engine for AgentZero.
//!
//! Every meaningful action passes through policy evaluation before execution.
//! Unknown permissions fail closed as denied (ADR 0003).

mod loader;
mod rules;

use agentzero_core::{
    Capability, DataClassification, ModelRoutingDecision, PolicyDecision, RuntimeTier,
};

pub use loader::load_policy_file;
pub use rules::PolicyRule;

/// A policy request submitted for evaluation.
#[derive(Debug, Clone)]
pub struct PolicyRequest {
    pub capability: Capability,
    pub classification: DataClassification,
    pub runtime: RuntimeTier,
    pub context: String,
}

/// Policy engine that evaluates requests against loaded rules.
///
/// The default policy denies all requests (fail closed).
pub struct PolicyEngine {
    rules: Vec<PolicyRule>,
}

impl PolicyEngine {
    /// Create a new deny-by-default policy engine with no rules.
    pub fn deny_by_default() -> Self {
        Self { rules: vec![] }
    }

    /// Create a policy engine with the given rules.
    pub fn with_rules(rules: Vec<PolicyRule>) -> Self {
        Self { rules }
    }

    /// Add a rule to the engine.
    pub fn add_rule(&mut self, rule: PolicyRule) {
        self.rules.push(rule);
    }

    /// Evaluate a policy request. Returns a decision.
    ///
    /// Rules are evaluated in order. The first matching rule wins.
    /// If no rule matches, the request is denied (fail closed).
    pub fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision {
        for rule in &self.rules {
            if let Some(decision) = rule.evaluate(request) {
                return decision;
            }
        }

        // Fail closed: no rule matched
        PolicyDecision::Deny {
            reason: format!(
                "deny-by-default: {:?} with classification {:?}",
                request.capability, request.classification
            ),
        }
    }

    /// Determine model routing for a given classification and destination.
    pub fn route_model_call(
        &self,
        classification: DataClassification,
        destination_is_local: bool,
    ) -> ModelRoutingDecision {
        agentzero_core::route_for_classification(classification, destination_is_local)
    }

    /// Return the number of loaded rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_engine_denies_all() {
        let engine = PolicyEngine::deny_by_default();
        let request = PolicyRequest {
            capability: Capability::FileRead,
            classification: DataClassification::Private,
            runtime: RuntimeTier::HostReadonly,
            context: "read file".into(),
        };
        let decision = engine.evaluate(&request);
        assert!(!decision.is_allowed());
    }

    #[test]
    fn deny_includes_reason() {
        let engine = PolicyEngine::deny_by_default();
        let request = PolicyRequest {
            capability: Capability::ShellCommand,
            classification: DataClassification::Unknown,
            runtime: RuntimeTier::HostSupervised,
            context: "shell".into(),
        };
        match engine.evaluate(&request) {
            PolicyDecision::Deny { reason } => {
                assert!(reason.contains("deny-by-default"));
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn engine_with_allow_rule() {
        let engine = PolicyEngine::with_rules(vec![PolicyRule::allow(
            Capability::FileRead,
            DataClassification::Private,
        )]);
        let request = PolicyRequest {
            capability: Capability::FileRead,
            classification: DataClassification::Private,
            runtime: RuntimeTier::HostReadonly,
            context: "read file".into(),
        };
        assert!(engine.evaluate(&request).is_allowed());
    }

    #[test]
    fn engine_with_allow_rule_still_denies_unmatched() {
        let engine = PolicyEngine::with_rules(vec![PolicyRule::allow(
            Capability::FileRead,
            DataClassification::Private,
        )]);
        let request = PolicyRequest {
            capability: Capability::ShellCommand,
            classification: DataClassification::Private,
            runtime: RuntimeTier::HostSupervised,
            context: "shell command".into(),
        };
        assert!(!engine.evaluate(&request).is_allowed());
    }

    #[test]
    fn first_matching_rule_wins() {
        let engine = PolicyEngine::with_rules(vec![
            PolicyRule::deny(Capability::FileRead, DataClassification::Secret),
            PolicyRule::allow(Capability::FileRead, DataClassification::Private),
        ]);

        // Secret → denied by first rule
        let req_secret = PolicyRequest {
            capability: Capability::FileRead,
            classification: DataClassification::Secret,
            runtime: RuntimeTier::HostReadonly,
            context: "read secret".into(),
        };
        assert!(!engine.evaluate(&req_secret).is_allowed());

        // Private → allowed by second rule
        let req_private = PolicyRequest {
            capability: Capability::FileRead,
            classification: DataClassification::Private,
            runtime: RuntimeTier::HostReadonly,
            context: "read private".into(),
        };
        assert!(engine.evaluate(&req_private).is_allowed());
    }

    #[test]
    fn add_rule_works() {
        let mut engine = PolicyEngine::deny_by_default();
        assert_eq!(engine.rule_count(), 0);
        engine.add_rule(PolicyRule::allow(
            Capability::FileRead,
            DataClassification::Public,
        ));
        assert_eq!(engine.rule_count(), 1);

        let request = PolicyRequest {
            capability: Capability::FileRead,
            classification: DataClassification::Public,
            runtime: RuntimeTier::HostReadonly,
            context: "read public".into(),
        };
        assert!(engine.evaluate(&request).is_allowed());
    }

    #[test]
    fn model_routing_local_always_allowed() {
        let engine = PolicyEngine::deny_by_default();
        let decision = engine.route_model_call(DataClassification::Secret, true);
        assert!(decision.is_allowed());
    }

    #[test]
    fn model_routing_secret_remote_denied() {
        let engine = PolicyEngine::deny_by_default();
        let decision = engine.route_model_call(DataClassification::Secret, false);
        assert!(!decision.is_allowed());
    }

    #[test]
    fn model_routing_pii_remote_requires_redaction() {
        let engine = PolicyEngine::deny_by_default();
        let decision = engine.route_model_call(DataClassification::Pii, false);
        assert!(decision.is_allowed());
        assert!(decision.requires_redaction());
    }

    #[test]
    fn require_approval_rule() {
        let engine = PolicyEngine::with_rules(vec![PolicyRule::require_approval(
            Capability::ShellCommand,
            "shell commands require user approval",
        )]);
        let request = PolicyRequest {
            capability: Capability::ShellCommand,
            classification: DataClassification::Private,
            runtime: RuntimeTier::HostSupervised,
            context: "run shell".into(),
        };
        match engine.evaluate(&request) {
            PolicyDecision::RequiresApproval { reason } => {
                assert!(reason.contains("user approval"));
            }
            other => panic!("expected RequiresApproval, got {other:?}"),
        }
    }
}
