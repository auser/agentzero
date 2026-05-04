use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::id::{ExecutionId, SessionId, SkillId, ToolId};

/// Data classification levels per the security model.
///
/// Unknown content is treated as `Private` (fail closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClassification {
    Public,
    Internal,
    Private,
    Pii,
    Secret,
    Credential,
    Regulated,
    Unknown,
}

impl DataClassification {
    /// Whether this classification allows remote model calls without redaction.
    pub fn allows_remote_unredacted(&self) -> bool {
        matches!(self, Self::Public)
    }

    /// Whether remote model calls are unconditionally denied.
    pub fn denies_remote(&self) -> bool {
        matches!(self, Self::Secret | Self::Credential | Self::Unknown)
    }
}

/// Runtime isolation tier per ADR 0006.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTier {
    /// Instruction-only skills and prompt templates.
    None,
    /// Safe host tools that cannot mutate.
    HostReadonly,
    /// Host execution requiring user approval.
    HostSupervised,
    /// Low-risk portable tools with explicit host calls.
    WasmSandbox,
    /// High-risk tools, package installs, native execution.
    MvmMicrovm,
    /// Action not allowed.
    Deny,
}

/// Capability that a tool or skill may request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    FileRead,
    FileWrite,
    ShellCommand,
    NetworkRequest,
    ModelCall,
    SecretHandleUsage,
    PackageInstall,
    PackageExecution,
    SkillLoad,
    RuntimeLaunch,
    AcpContextRequest,
    MvmMount,
    WasmHostCall,
}

/// Result of a policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "effect")]
pub enum PolicyDecision {
    /// Action is allowed.
    Allow,
    /// Action is denied with a reason.
    Deny { reason: String },
    /// Action requires user approval before proceeding.
    RequiresApproval { reason: String },
    /// Action is allowed but content must be redacted first.
    AllowWithRedaction { reason: String },
}

impl PolicyDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow | Self::AllowWithRedaction { .. })
    }
}

/// Scope of a user approval per the security model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalScope {
    Once,
    Session,
    Project,
    Package,
    Never,
}

/// Structured audit event per ADR 0003.
///
/// Raw secrets must never appear in audit events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub execution_id: ExecutionId,
    pub session_id: SessionId,
    pub timestamp: DateTime<Utc>,
    pub action: String,
    pub capability: Capability,
    pub classification: DataClassification,
    pub decision: PolicyDecision,
    pub reason: String,
    pub runtime: RuntimeTier,
    pub skill_id: Option<SkillId>,
    pub tool_id: Option<ToolId>,
    pub redactions_applied: Vec<String>,
    pub approval_scope: Option<ApprovalScope>,
}

/// Sandbox profile describing execution constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxProfile {
    pub runtime: RuntimeTier,
    pub capabilities: Vec<Capability>,
    pub filesystem_paths: Vec<String>,
    pub network_allowed: bool,
    pub max_duration_secs: Option<u64>,
}

/// Skill manifest declaring metadata and requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: SkillId,
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Vec<Capability>,
    pub runtime: RuntimeTier,
}

/// Schema describing a tool's interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub id: ToolId,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<Capability>,
    pub parameters: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_classification_denies_remote() {
        assert!(DataClassification::Unknown.denies_remote());
    }

    #[test]
    fn secret_denies_remote() {
        assert!(DataClassification::Secret.denies_remote());
    }

    #[test]
    fn credential_denies_remote() {
        assert!(DataClassification::Credential.denies_remote());
    }

    #[test]
    fn public_allows_remote_unredacted() {
        assert!(DataClassification::Public.allows_remote_unredacted());
    }

    #[test]
    fn private_does_not_allow_remote_unredacted() {
        assert!(!DataClassification::Private.allows_remote_unredacted());
    }

    #[test]
    fn pii_does_not_allow_remote_unredacted() {
        assert!(!DataClassification::Pii.allows_remote_unredacted());
    }

    #[test]
    fn allow_decision_is_allowed() {
        assert!(PolicyDecision::Allow.is_allowed());
    }

    #[test]
    fn deny_decision_is_not_allowed() {
        let d = PolicyDecision::Deny {
            reason: "test".into(),
        };
        assert!(!d.is_allowed());
    }

    #[test]
    fn allow_with_redaction_is_allowed() {
        let d = PolicyDecision::AllowWithRedaction {
            reason: "pii present".into(),
        };
        assert!(d.is_allowed());
    }

    #[test]
    fn requires_approval_is_not_allowed() {
        let d = PolicyDecision::RequiresApproval {
            reason: "shell command".into(),
        };
        assert!(!d.is_allowed());
    }

    #[test]
    fn data_classification_serializes() {
        let json = serde_json::to_string(&DataClassification::Pii)
            .expect("DataClassification should serialize");
        assert_eq!(json, "\"pii\"");
    }

    #[test]
    fn runtime_tier_serializes() {
        let json =
            serde_json::to_string(&RuntimeTier::WasmSandbox).expect("RuntimeTier should serialize");
        assert_eq!(json, "\"wasm_sandbox\"");
    }
}
