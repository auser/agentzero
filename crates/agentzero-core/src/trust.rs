use serde::{Deserialize, Serialize};

/// Trust source label for content per ADR 0008.
///
/// AgentZero labels content by trust source and never lets untrusted
/// content become trusted instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustSource {
    /// Trusted user instructions (typed by the user).
    UserInstruction,
    /// Trusted project policy (`.agentzero/policy.yml`).
    ProjectPolicy,
    /// Trusted accepted ADRs.
    AcceptedAdr,
    /// Trusted AgentZero core code.
    CoreCode,
    /// Compatible but untrusted skill instructions.
    SkillInstruction,
    /// Untrusted document content (files, READMEs, etc.).
    DocumentContent,
    /// Untrusted tool output.
    ToolOutput,
    /// Untrusted network content.
    NetworkContent,
    /// Untrusted package code.
    PackageCode,
    /// Untrusted runtime guest output.
    RuntimeGuestOutput,
}

impl TrustSource {
    /// Whether this source is trusted for use as agent instructions.
    pub fn is_trusted(&self) -> bool {
        matches!(
            self,
            Self::UserInstruction | Self::ProjectPolicy | Self::AcceptedAdr | Self::CoreCode
        )
    }

    /// Whether content from this source should be treated as data only,
    /// never as instructions to the agent.
    pub fn is_data_only(&self) -> bool {
        !self.is_trusted()
    }
}

/// Content with an attached trust label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabeledContent {
    pub source: TrustSource,
    pub content: String,
}

impl LabeledContent {
    pub fn trusted(source: TrustSource, content: impl Into<String>) -> Self {
        debug_assert!(source.is_trusted(), "use untrusted() for untrusted sources");
        Self {
            source,
            content: content.into(),
        }
    }

    pub fn untrusted(source: TrustSource, content: impl Into<String>) -> Self {
        debug_assert!(source.is_data_only(), "use trusted() for trusted sources");
        Self {
            source,
            content: content.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_instruction_is_trusted() {
        assert!(TrustSource::UserInstruction.is_trusted());
        assert!(!TrustSource::UserInstruction.is_data_only());
    }

    #[test]
    fn project_policy_is_trusted() {
        assert!(TrustSource::ProjectPolicy.is_trusted());
    }

    #[test]
    fn document_content_is_data_only() {
        assert!(TrustSource::DocumentContent.is_data_only());
        assert!(!TrustSource::DocumentContent.is_trusted());
    }

    #[test]
    fn tool_output_is_data_only() {
        assert!(TrustSource::ToolOutput.is_data_only());
    }

    #[test]
    fn network_content_is_data_only() {
        assert!(TrustSource::NetworkContent.is_data_only());
    }

    #[test]
    fn package_code_is_data_only() {
        assert!(TrustSource::PackageCode.is_data_only());
    }

    #[test]
    fn skill_instruction_is_data_only() {
        assert!(TrustSource::SkillInstruction.is_data_only());
    }

    #[test]
    fn labeled_content_trusted() {
        let lc = LabeledContent::trusted(TrustSource::UserInstruction, "do this");
        assert!(lc.source.is_trusted());
        assert_eq!(lc.content, "do this");
    }

    #[test]
    fn labeled_content_untrusted() {
        let lc = LabeledContent::untrusted(TrustSource::DocumentContent, "# README");
        assert!(lc.source.is_data_only());
    }

    #[test]
    fn trust_source_serializes() {
        let json =
            serde_json::to_string(&TrustSource::ToolOutput).expect("TrustSource should serialize");
        assert_eq!(json, "\"tool_output\"");
    }
}
