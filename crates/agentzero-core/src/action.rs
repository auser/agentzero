use serde::{Deserialize, Serialize};

/// Typed action kinds for audit events and policy evaluation.
///
/// Using a typed enum instead of raw strings ensures exhaustive matching
/// and prevents typo-based misclassification.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    FileRead,
    FileWrite,
    FileList,
    FileSearch,
    ShellCommand,
    ModelCallLocal,
    ModelCallRemote,
    SecretHandleResolve,
    SkillLoad,
    SkillExecute,
    PackageInstall,
    PolicyEvaluate,
    AuditRecord,
    SessionStart,
    SessionEnd,
    ApprovalRequest,
    ApprovalGrant,
    ApprovalDeny,
    RedactionApplied,
    SandboxCreate,
    SandboxDestroy,
}

impl std::fmt::Display for ActionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let json = serde_json::to_string(self).unwrap_or_else(|_| "unknown".into());
        // Strip the surrounding quotes from the JSON string
        f.write_str(json.trim_matches('"'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_kind_display() {
        assert_eq!(ActionKind::FileRead.to_string(), "file_read");
        assert_eq!(ActionKind::ModelCallRemote.to_string(), "model_call_remote");
    }

    #[test]
    fn action_kind_serializes() {
        let json =
            serde_json::to_string(&ActionKind::ShellCommand).expect("ActionKind should serialize");
        assert_eq!(json, "\"shell_command\"");
    }

    #[test]
    fn action_kind_deserializes() {
        let kind: ActionKind =
            serde_json::from_str("\"file_read\"").expect("ActionKind should deserialize");
        assert_eq!(kind, ActionKind::FileRead);
    }
}
