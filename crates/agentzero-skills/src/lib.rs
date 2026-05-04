//! Skill package manifest and loading contracts for AgentZero.
//!
//! Skills are first-class capability bundles (ADR 0004). They declare
//! metadata, required capabilities, runtime requirements, and version.
//! Skill execution is permissioned and auditable.

use agentzero_core::{Capability, RuntimeTier, SkillId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SkillError {
    #[error("skill not found: {0}")]
    NotFound(String),
    #[error("skill validation failed: {0}")]
    ValidationFailed(String),
}

/// Runtime environment a skill requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillRuntime {
    /// Instruction-only, no executable code.
    InstructionOnly,
    /// Requires WASM sandbox.
    Wasm,
    /// Requires MVM microVM.
    Mvm,
    /// Runs on the host with supervision.
    HostSupervised,
}

/// A permission a skill requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPermission {
    pub capability: Capability,
    pub reason: String,
}

/// Reference to a skill package source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SkillPackageRef {
    /// Local filesystem path.
    Local { path: String },
    /// Remote registry reference (future).
    Registry { name: String, version: String },
}

/// Full skill manifest per ADR 0004 and ADR 0005.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: SkillId,
    pub name: String,
    pub version: String,
    pub description: String,
    pub runtime: SkillRuntime,
    pub permissions: Vec<SkillPermission>,
    pub source: Option<SkillPackageRef>,
}

impl SkillManifest {
    /// Validate that the manifest has required fields.
    pub fn validate(&self) -> Result<(), SkillError> {
        if self.name.is_empty() {
            return Err(SkillError::ValidationFailed("name is empty".into()));
        }
        if self.version.is_empty() {
            return Err(SkillError::ValidationFailed("version is empty".into()));
        }
        Ok(())
    }

    /// Return the runtime tier this skill maps to.
    pub fn runtime_tier(&self) -> RuntimeTier {
        match self.runtime {
            SkillRuntime::InstructionOnly => RuntimeTier::None,
            SkillRuntime::Wasm => RuntimeTier::WasmSandbox,
            SkillRuntime::Mvm => RuntimeTier::MvmMicrovm,
            SkillRuntime::HostSupervised => RuntimeTier::HostSupervised,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> SkillManifest {
        SkillManifest {
            id: SkillId::from_string("repo-security-audit"),
            name: "repo-security-audit".into(),
            version: "0.1.0".into(),
            description: "Audit repo for secrets and PII".into(),
            runtime: SkillRuntime::InstructionOnly,
            permissions: vec![SkillPermission {
                capability: Capability::FileRead,
                reason: "needs to read repo files".into(),
            }],
            source: Some(SkillPackageRef::Local {
                path: "skills/repo-security-audit".into(),
            }),
        }
    }

    #[test]
    fn valid_manifest_passes_validation() {
        let manifest = sample_manifest();
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn empty_name_fails_validation() {
        let mut manifest = sample_manifest();
        manifest.name = String::new();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn empty_version_fails_validation() {
        let mut manifest = sample_manifest();
        manifest.version = String::new();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn instruction_only_maps_to_none_tier() {
        let manifest = sample_manifest();
        assert_eq!(manifest.runtime_tier(), RuntimeTier::None);
    }

    #[test]
    fn wasm_maps_to_wasm_sandbox_tier() {
        let mut manifest = sample_manifest();
        manifest.runtime = SkillRuntime::Wasm;
        assert_eq!(manifest.runtime_tier(), RuntimeTier::WasmSandbox);
    }

    #[test]
    fn mvm_maps_to_mvm_tier() {
        let mut manifest = sample_manifest();
        manifest.runtime = SkillRuntime::Mvm;
        assert_eq!(manifest.runtime_tier(), RuntimeTier::MvmMicrovm);
    }

    #[test]
    fn manifest_serializes() {
        let manifest = sample_manifest();
        let json = serde_json::to_string(&manifest).expect("manifest should serialize");
        assert!(json.contains("repo-security-audit"));
    }
}
