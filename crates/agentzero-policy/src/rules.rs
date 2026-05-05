use agentzero_core::{Capability, DataClassification, PolicyDecision, RuntimeTier};

use crate::PolicyRequest;

/// A single policy rule that matches requests and produces decisions.
#[derive(Debug, Clone)]
pub struct PolicyRule {
    matcher: RuleMatcher,
    effect: RuleEffect,
}

#[derive(Debug, Clone)]
enum RuleMatcher {
    /// Match a specific capability and classification.
    CapabilityAndClassification {
        capability: Capability,
        classification: DataClassification,
    },
    /// Match any request with the given capability.
    Capability { capability: Capability },
    /// Match a capability at a specific runtime tier.
    CapabilityAndRuntime {
        capability: Capability,
        runtime: RuntimeTier,
    },
}

#[derive(Debug, Clone)]
enum RuleEffect {
    Allow,
    Deny { reason: String },
    RequireApproval { reason: String },
    AllowWithRedaction { reason: String },
}

impl PolicyRule {
    /// Create an allow rule for a specific capability + classification.
    pub fn allow(capability: Capability, classification: DataClassification) -> Self {
        Self {
            matcher: RuleMatcher::CapabilityAndClassification {
                capability,
                classification,
            },
            effect: RuleEffect::Allow,
        }
    }

    /// Create a deny rule for a specific capability + classification.
    pub fn deny(capability: Capability, classification: DataClassification) -> Self {
        let reason = format!(
            "denied: {:?} with {:?} classification",
            capability, classification
        );
        Self {
            matcher: RuleMatcher::CapabilityAndClassification {
                capability,
                classification,
            },
            effect: RuleEffect::Deny { reason },
        }
    }

    /// Create a require-approval rule for any request with the given capability.
    pub fn require_approval(capability: Capability, reason: &str) -> Self {
        Self {
            matcher: RuleMatcher::Capability { capability },
            effect: RuleEffect::RequireApproval {
                reason: reason.to_string(),
            },
        }
    }

    /// Create an allow rule for a capability at a specific runtime tier.
    pub fn allow_runtime(capability: Capability, runtime: RuntimeTier) -> Self {
        Self {
            matcher: RuleMatcher::CapabilityAndRuntime {
                capability,
                runtime,
            },
            effect: RuleEffect::Allow,
        }
    }

    /// Create a deny rule for a capability at a specific runtime tier.
    pub fn deny_runtime(capability: Capability, runtime: RuntimeTier, reason: &str) -> Self {
        Self {
            matcher: RuleMatcher::CapabilityAndRuntime {
                capability,
                runtime,
            },
            effect: RuleEffect::Deny {
                reason: reason.to_string(),
            },
        }
    }

    /// Create an allow-with-redaction rule for a specific capability + classification.
    pub fn allow_with_redaction(
        capability: Capability,
        classification: DataClassification,
        reason: &str,
    ) -> Self {
        Self {
            matcher: RuleMatcher::CapabilityAndClassification {
                capability,
                classification,
            },
            effect: RuleEffect::AllowWithRedaction {
                reason: reason.to_string(),
            },
        }
    }

    /// Evaluate this rule against a request. Returns `Some(decision)` if the rule
    /// matches, `None` if it doesn't apply.
    pub fn evaluate(&self, request: &PolicyRequest) -> Option<PolicyDecision> {
        if !self.matches(request) {
            return None;
        }

        Some(match &self.effect {
            RuleEffect::Allow => PolicyDecision::Allow,
            RuleEffect::Deny { reason } => PolicyDecision::Deny {
                reason: reason.clone(),
            },
            RuleEffect::RequireApproval { reason } => PolicyDecision::RequiresApproval {
                reason: reason.clone(),
            },
            RuleEffect::AllowWithRedaction { reason } => PolicyDecision::AllowWithRedaction {
                reason: reason.clone(),
            },
        })
    }

    fn matches(&self, request: &PolicyRequest) -> bool {
        match &self.matcher {
            RuleMatcher::CapabilityAndClassification {
                capability,
                classification,
            } => request.capability == *capability && request.classification == *classification,
            RuleMatcher::Capability { capability } => request.capability == *capability,
            RuleMatcher::CapabilityAndRuntime {
                capability,
                runtime,
            } => request.capability == *capability && request.runtime == *runtime,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::RuntimeTier;

    fn request(capability: Capability, classification: DataClassification) -> PolicyRequest {
        PolicyRequest {
            capability,
            classification,
            runtime: RuntimeTier::HostReadonly,
            context: "test".into(),
        }
    }

    #[test]
    fn allow_rule_matches() {
        let rule = PolicyRule::allow(Capability::FileRead, DataClassification::Private);
        let decision = rule.evaluate(&request(Capability::FileRead, DataClassification::Private));
        assert_eq!(decision, Some(PolicyDecision::Allow));
    }

    #[test]
    fn allow_rule_does_not_match_different_capability() {
        let rule = PolicyRule::allow(Capability::FileRead, DataClassification::Private);
        let decision = rule.evaluate(&request(Capability::FileWrite, DataClassification::Private));
        assert_eq!(decision, None);
    }

    #[test]
    fn allow_rule_does_not_match_different_classification() {
        let rule = PolicyRule::allow(Capability::FileRead, DataClassification::Private);
        let decision = rule.evaluate(&request(Capability::FileRead, DataClassification::Secret));
        assert_eq!(decision, None);
    }

    #[test]
    fn deny_rule_matches() {
        let rule = PolicyRule::deny(Capability::ShellCommand, DataClassification::Secret);
        let decision = rule.evaluate(&request(
            Capability::ShellCommand,
            DataClassification::Secret,
        ));
        assert!(decision.is_some());
        assert!(!decision.as_ref().expect("should be Some").is_allowed());
    }

    #[test]
    fn require_approval_matches_any_classification() {
        let rule = PolicyRule::require_approval(Capability::ShellCommand, "needs approval");
        // Should match regardless of classification
        let d1 = rule.evaluate(&request(
            Capability::ShellCommand,
            DataClassification::Private,
        ));
        let d2 = rule.evaluate(&request(
            Capability::ShellCommand,
            DataClassification::Public,
        ));
        assert!(d1.is_some());
        assert!(d2.is_some());
    }

    #[test]
    fn allow_runtime_rule_matches_tier() {
        let rule = PolicyRule::allow_runtime(Capability::RuntimeLaunch, RuntimeTier::WasmSandbox);
        let req = PolicyRequest {
            capability: Capability::RuntimeLaunch,
            classification: DataClassification::Private,
            runtime: RuntimeTier::WasmSandbox,
            context: "test".into(),
        };
        assert_eq!(rule.evaluate(&req), Some(PolicyDecision::Allow));
    }

    #[test]
    fn allow_runtime_rule_does_not_match_different_tier() {
        let rule = PolicyRule::allow_runtime(Capability::RuntimeLaunch, RuntimeTier::WasmSandbox);
        let req = PolicyRequest {
            capability: Capability::RuntimeLaunch,
            classification: DataClassification::Private,
            runtime: RuntimeTier::HostSupervised,
            context: "test".into(),
        };
        assert_eq!(rule.evaluate(&req), None);
    }

    #[test]
    fn deny_runtime_rule_matches() {
        let rule = PolicyRule::deny_runtime(
            Capability::RuntimeLaunch,
            RuntimeTier::MvmMicrovm,
            "MVM not supported",
        );
        let req = PolicyRequest {
            capability: Capability::RuntimeLaunch,
            classification: DataClassification::Private,
            runtime: RuntimeTier::MvmMicrovm,
            context: "test".into(),
        };
        let decision = rule.evaluate(&req);
        assert!(decision.is_some());
        assert!(!decision.as_ref().expect("should be Some").is_allowed());
    }

    #[test]
    fn allow_with_redaction_rule() {
        let rule = PolicyRule::allow_with_redaction(
            Capability::ModelCall,
            DataClassification::Pii,
            "PII must be redacted",
        );
        let decision = rule.evaluate(&request(Capability::ModelCall, DataClassification::Pii));
        match decision {
            Some(PolicyDecision::AllowWithRedaction { reason }) => {
                assert!(reason.contains("redacted"));
            }
            other => panic!("expected AllowWithRedaction, got {other:?}"),
        }
    }
}
