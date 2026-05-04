//! Sandbox profile and execution contracts for AgentZero.
//!
//! Defines execution constraints per ADR 0006 (runtime isolation tiers).
//! No actual process execution happens here — this crate models the
//! contracts that sandbox implementations must satisfy.

use agentzero_core::{Capability, ExecutionId, RuntimeTier};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("sandbox creation denied: {0}")]
    Denied(String),
    #[error("sandbox limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("sandbox execution failed: {0}")]
    ExecutionFailed(String),
}

/// Resource limit for sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxLimit {
    pub max_duration_secs: u64,
    pub max_memory_bytes: Option<u64>,
    pub max_cpu_secs: Option<u64>,
}

impl Default for SandboxLimit {
    fn default() -> Self {
        Self {
            max_duration_secs: 60,
            max_memory_bytes: None,
            max_cpu_secs: None,
        }
    }
}

/// A filesystem mount visible inside the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxMount {
    pub host_path: String,
    pub guest_path: String,
    pub readonly: bool,
}

/// Network policy for sandbox execution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxNetworkPolicy {
    /// No network access.
    #[default]
    Deny,
    /// Allow specific egress only.
    AllowEgress,
}

/// Complete sandbox profile for an execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxProfile {
    pub runtime: RuntimeTier,
    pub capabilities: Vec<Capability>,
    pub mounts: Vec<SandboxMount>,
    pub network: SandboxNetworkPolicy,
    pub limits: SandboxLimit,
}

impl SandboxProfile {
    /// Create a minimal read-only host profile with no network.
    pub fn host_readonly(paths: Vec<SandboxMount>) -> Self {
        Self {
            runtime: RuntimeTier::HostReadonly,
            capabilities: vec![Capability::FileRead],
            mounts: paths,
            network: SandboxNetworkPolicy::Deny,
            limits: SandboxLimit::default(),
        }
    }

    /// Create a deny-all profile.
    pub fn deny() -> Self {
        Self {
            runtime: RuntimeTier::Deny,
            capabilities: vec![],
            mounts: vec![],
            network: SandboxNetworkPolicy::Deny,
            limits: SandboxLimit::default(),
        }
    }
}

/// A request to execute something inside a sandbox.
#[derive(Debug, Clone)]
pub struct SandboxExecutionRequest {
    pub execution_id: ExecutionId,
    pub profile: SandboxProfile,
    pub command: String,
    pub args: Vec<String>,
}

/// Result of a sandbox execution.
#[derive(Debug, Clone)]
pub struct SandboxExecutionResult {
    pub execution_id: ExecutionId,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_readonly_profile_has_file_read_only() {
        let profile = SandboxProfile::host_readonly(vec![SandboxMount {
            host_path: "/project".into(),
            guest_path: "/project".into(),
            readonly: true,
        }]);
        assert_eq!(profile.runtime, RuntimeTier::HostReadonly);
        assert_eq!(profile.capabilities, vec![Capability::FileRead]);
        assert_eq!(profile.network, SandboxNetworkPolicy::Deny);
        assert_eq!(profile.mounts.len(), 1);
        assert!(profile.mounts[0].readonly);
    }

    #[test]
    fn deny_profile_has_no_capabilities() {
        let profile = SandboxProfile::deny();
        assert_eq!(profile.runtime, RuntimeTier::Deny);
        assert!(profile.capabilities.is_empty());
        assert!(profile.mounts.is_empty());
        assert_eq!(profile.network, SandboxNetworkPolicy::Deny);
    }

    #[test]
    fn default_limits_are_reasonable() {
        let limits = SandboxLimit::default();
        assert_eq!(limits.max_duration_secs, 60);
        assert!(limits.max_memory_bytes.is_none());
    }

    #[test]
    fn default_network_is_deny() {
        assert_eq!(SandboxNetworkPolicy::default(), SandboxNetworkPolicy::Deny);
    }

    #[test]
    fn sandbox_profile_serializes() {
        let profile = SandboxProfile::host_readonly(vec![]);
        let json = serde_json::to_string(&profile).expect("profile should serialize");
        assert!(json.contains("host_readonly"));
    }
}
