use serde::{Deserialize, Serialize};

use crate::DataClassification;

/// Model routing decision per ADR 0002.
///
/// AgentZero defaults to local models. Remote model calls require
/// policy evaluation, data classification, redaction checks, and audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum ModelRoutingDecision {
    /// Route to a local model. Always safe.
    Local,
    /// Route to a remote model. Content has been classified and cleared.
    RemoteAllowed { reason: String },
    /// Route to a remote model but content must be redacted first.
    RemoteWithRedaction { reason: String },
    /// Remote routing denied. Content classification prohibits it.
    RemoteDenied { reason: String },
}

impl ModelRoutingDecision {
    /// Whether this decision allows the model call to proceed.
    pub fn is_allowed(&self) -> bool {
        matches!(
            self,
            Self::Local | Self::RemoteAllowed { .. } | Self::RemoteWithRedaction { .. }
        )
    }

    /// Whether redaction is required before the call.
    pub fn requires_redaction(&self) -> bool {
        matches!(self, Self::RemoteWithRedaction { .. })
    }
}

/// Determine the routing decision for a given classification and destination.
pub fn route_for_classification(
    classification: DataClassification,
    destination_is_local: bool,
) -> ModelRoutingDecision {
    if destination_is_local {
        return ModelRoutingDecision::Local;
    }

    match classification {
        DataClassification::Public => ModelRoutingDecision::RemoteAllowed {
            reason: "public content may be sent to remote models".into(),
        },
        DataClassification::Internal => ModelRoutingDecision::RemoteAllowed {
            reason: "internal content allowed with explicit policy".into(),
        },
        DataClassification::Private => ModelRoutingDecision::RemoteWithRedaction {
            reason: "private content requires redaction before remote model call".into(),
        },
        DataClassification::Pii => ModelRoutingDecision::RemoteWithRedaction {
            reason: "PII must be redacted before remote model call".into(),
        },
        DataClassification::Regulated => ModelRoutingDecision::RemoteDenied {
            reason: "regulated content denied for remote models".into(),
        },
        DataClassification::Secret => ModelRoutingDecision::RemoteDenied {
            reason: "secrets are never sent to remote models".into(),
        },
        DataClassification::Credential => ModelRoutingDecision::RemoteDenied {
            reason: "credentials are never sent to remote models".into(),
        },
        DataClassification::Unknown => ModelRoutingDecision::RemoteDenied {
            reason: "unknown classification fails closed: denied for remote models".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_always_allowed() {
        let d = route_for_classification(DataClassification::Secret, true);
        assert_eq!(d, ModelRoutingDecision::Local);
        assert!(d.is_allowed());
        assert!(!d.requires_redaction());
    }

    #[test]
    fn public_remote_allowed() {
        let d = route_for_classification(DataClassification::Public, false);
        assert!(d.is_allowed());
        assert!(!d.requires_redaction());
    }

    #[test]
    fn private_remote_requires_redaction() {
        let d = route_for_classification(DataClassification::Private, false);
        assert!(d.is_allowed());
        assert!(d.requires_redaction());
    }

    #[test]
    fn pii_remote_requires_redaction() {
        let d = route_for_classification(DataClassification::Pii, false);
        assert!(d.is_allowed());
        assert!(d.requires_redaction());
    }

    #[test]
    fn secret_remote_denied() {
        let d = route_for_classification(DataClassification::Secret, false);
        assert!(!d.is_allowed());
    }

    #[test]
    fn credential_remote_denied() {
        let d = route_for_classification(DataClassification::Credential, false);
        assert!(!d.is_allowed());
    }

    #[test]
    fn unknown_remote_denied() {
        let d = route_for_classification(DataClassification::Unknown, false);
        assert!(!d.is_allowed());
    }

    #[test]
    fn regulated_remote_denied() {
        let d = route_for_classification(DataClassification::Regulated, false);
        assert!(!d.is_allowed());
    }

    #[test]
    fn routing_decision_serializes() {
        let d = ModelRoutingDecision::RemoteDenied {
            reason: "test".into(),
        };
        let json = serde_json::to_string(&d).expect("should serialize");
        assert!(json.contains("remote_denied"));
    }
}
