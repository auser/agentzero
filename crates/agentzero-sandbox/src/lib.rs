//! Sandbox profile and execution contracts for AgentZero.
//!
//! Defines execution constraints per ADR 0006 (runtime isolation tiers).
//! Includes WASM sandbox runtime (behind `wasm` feature flag).

pub mod codegen;
pub mod wasm;

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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxNetworkPolicy {
    /// No network access.
    #[default]
    Deny,
    /// Allow all outbound egress (unrestricted).
    AllowEgress,
    /// Allow outbound egress only to specific hosts.
    AllowEgressFiltered {
        /// Allowed hostnames (e.g. `["slack.com", "api.telegram.org"]`).
        allowed_hosts: Vec<String>,
    },
}

impl SandboxNetworkPolicy {
    /// Check whether the given URL is allowed by this policy.
    pub fn allows_url(&self, url: &str) -> bool {
        match self {
            Self::Deny => false,
            Self::AllowEgress => true,
            Self::AllowEgressFiltered { allowed_hosts } => {
                // Extract host from URL
                let host = url
                    .strip_prefix("https://")
                    .or_else(|| url.strip_prefix("http://"))
                    .unwrap_or(url)
                    .split('/')
                    .next()
                    .unwrap_or("")
                    .split(':')
                    .next()
                    .unwrap_or("");
                allowed_hosts
                    .iter()
                    .any(|h| host == h || host.ends_with(&format!(".{h}")))
            }
        }
    }
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

    #[test]
    fn deny_policy_blocks_all_urls() {
        let policy = SandboxNetworkPolicy::Deny;
        assert!(!policy.allows_url("https://slack.com/api/chat.postMessage"));
    }

    #[test]
    fn allow_egress_allows_all_urls() {
        let policy = SandboxNetworkPolicy::AllowEgress;
        assert!(policy.allows_url("https://slack.com/api/chat.postMessage"));
        assert!(policy.allows_url("http://localhost:8080/test"));
    }

    #[test]
    fn filtered_egress_checks_host() {
        let policy = SandboxNetworkPolicy::AllowEgressFiltered {
            allowed_hosts: vec!["slack.com".into(), "api.telegram.org".into()],
        };
        assert!(policy.allows_url("https://slack.com/api/chat.postMessage"));
        assert!(policy.allows_url("https://api.telegram.org/bot123/sendMessage"));
        assert!(!policy.allows_url("https://evil.com/steal-data"));
        assert!(!policy.allows_url("https://notslack.com/api"));
    }

    #[test]
    fn filtered_egress_allows_subdomains() {
        let policy = SandboxNetworkPolicy::AllowEgressFiltered {
            allowed_hosts: vec!["slack.com".into()],
        };
        assert!(policy.allows_url("https://files.slack.com/upload"));
    }

    #[test]
    fn filtered_egress_serializes() {
        let policy = SandboxNetworkPolicy::AllowEgressFiltered {
            allowed_hosts: vec!["slack.com".into()],
        };
        let json = serde_json::to_string(&policy).expect("should serialize");
        let loaded: SandboxNetworkPolicy = serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(loaded, policy);
    }
}
