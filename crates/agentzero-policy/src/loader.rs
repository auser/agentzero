//! Policy YAML/TOML loader.
//!
//! Parses `.agentzero/policy.yml` (actually TOML format despite the extension,
//! matching the init command output) into PolicyRule instances.

use std::path::Path;

use agentzero_core::{Capability, DataClassification};
use agentzero_tracing::info;
use serde::Deserialize;

use crate::PolicyRule;

/// Parsed policy file structure.
#[derive(Debug, Deserialize)]
struct PolicyFile {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default = "default_classification")]
    default_classification: String,
    #[serde(default)]
    model_routing: String,
    #[serde(default)]
    shell_commands: String,
    #[serde(default)]
    file_write: String,
    #[serde(default)]
    file_read: String,
    #[serde(default)]
    network: String,
}

fn default_version() -> u32 {
    1
}
fn default_classification() -> String {
    "private".into()
}

/// Load policy rules from a file.
///
/// The file format is TOML with keys matching the policy schema written by `init --private`.
/// Lines starting with `#` are comments.
pub fn load_policy_file(path: &Path) -> Result<Vec<PolicyRule>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read policy file {}: {e}", path.display()))?;

    // Strip comment lines for TOML parsing (TOML supports # comments natively,
    // but the "# AgentZero Policy" header line is fine)
    let parsed: PolicyFile = toml::from_str(&content)
        .map_err(|e| format!("failed to parse policy file {}: {e}", path.display()))?;

    info!(
        version = parsed.version,
        classification = %parsed.default_classification,
        "loaded policy file"
    );

    let default_class = parse_classification(&parsed.default_classification);
    let mut rules = Vec::new();

    // File read rules
    match parsed.file_read.as_str() {
        "allow" => {
            rules.push(PolicyRule::allow(Capability::FileRead, default_class));
        }
        "deny" => {
            rules.push(PolicyRule::deny(Capability::FileRead, default_class));
        }
        // Default: allow file reads for private classification
        _ => {
            rules.push(PolicyRule::allow(
                Capability::FileRead,
                DataClassification::Private,
            ));
            rules.push(PolicyRule::allow(
                Capability::FileRead,
                DataClassification::Public,
            ));
            rules.push(PolicyRule::allow(
                Capability::FileRead,
                DataClassification::Internal,
            ));
        }
    }

    // File write rules
    match parsed.file_write.as_str() {
        "allow" => {
            rules.push(PolicyRule::allow(Capability::FileWrite, default_class));
        }
        "deny" => {
            rules.push(PolicyRule::deny(Capability::FileWrite, default_class));
        }
        _ => {
            rules.push(PolicyRule::require_approval(
                Capability::FileWrite,
                "file writes require approval per policy",
            ));
        }
    }

    // Shell command rules
    match parsed.shell_commands.as_str() {
        "allow" => {
            rules.push(PolicyRule::allow(Capability::ShellCommand, default_class));
        }
        "deny" => {
            rules.push(PolicyRule::deny(Capability::ShellCommand, default_class));
        }
        _ => {
            rules.push(PolicyRule::require_approval(
                Capability::ShellCommand,
                "shell commands require approval per policy",
            ));
        }
    }

    // Network rules
    match parsed.network.as_str() {
        "allow" => {
            rules.push(PolicyRule::allow(Capability::NetworkRequest, default_class));
        }
        "require_approval" => {
            rules.push(PolicyRule::require_approval(
                Capability::NetworkRequest,
                "network requests require approval per policy",
            ));
        }
        _ => {
            rules.push(PolicyRule::deny(Capability::NetworkRequest, default_class));
        }
    }

    // Model routing rules
    match parsed.model_routing.as_str() {
        "local_only" => {
            // Deny all remote model calls
            rules.push(PolicyRule::deny(
                Capability::ModelCall,
                DataClassification::Secret,
            ));
            rules.push(PolicyRule::deny(
                Capability::ModelCall,
                DataClassification::Credential,
            ));
            rules.push(PolicyRule::deny(
                Capability::ModelCall,
                DataClassification::Pii,
            ));
            rules.push(PolicyRule::deny(
                Capability::ModelCall,
                DataClassification::Private,
            ));
        }
        "local_preferred" => {
            // Deny secrets/credentials, require redaction for PII, allow others
            rules.push(PolicyRule::deny(
                Capability::ModelCall,
                DataClassification::Secret,
            ));
            rules.push(PolicyRule::deny(
                Capability::ModelCall,
                DataClassification::Credential,
            ));
            rules.push(PolicyRule::allow_with_redaction(
                Capability::ModelCall,
                DataClassification::Pii,
                "PII must be redacted before remote model call",
            ));
        }
        _ => {}
    }

    info!(rules = rules.len(), "policy rules loaded");
    Ok(rules)
}

fn parse_classification(s: &str) -> DataClassification {
    match s {
        "public" => DataClassification::Public,
        "internal" => DataClassification::Internal,
        "private" => DataClassification::Private,
        "pii" => DataClassification::Pii,
        "secret" => DataClassification::Secret,
        "credential" => DataClassification::Credential,
        _ => DataClassification::Private, // fail closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agentzero-policy-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be after epoch")
                .as_nanos()
        ))
    }

    #[test]
    fn loads_private_policy() {
        let dir = temp_dir("private-policy");
        fs::create_dir_all(&dir).expect("should create dir");
        let policy_path = dir.join("policy.yml");
        fs::write(
            &policy_path,
            concat!(
                "version = 1\n",
                "default_classification = \"private\"\n",
                "model_routing = \"local_only\"\n",
                "shell_commands = \"require_approval\"\n",
                "file_write = \"require_approval\"\n",
                "network = \"deny\"\n",
            ),
        )
        .expect("should write");

        let rules = load_policy_file(&policy_path).expect("should load");
        assert!(!rules.is_empty());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn loads_default_policy() {
        let dir = temp_dir("default-policy");
        fs::create_dir_all(&dir).expect("should create dir");
        let policy_path = dir.join("policy.yml");
        fs::write(
            &policy_path,
            concat!(
                "version = 1\n",
                "default_classification = \"private\"\n",
                "model_routing = \"local_preferred\"\n",
                "shell_commands = \"require_approval\"\n",
                "file_write = \"require_approval\"\n",
                "network = \"require_approval\"\n",
            ),
        )
        .expect("should write");

        let rules = load_policy_file(&policy_path).expect("should load");
        assert!(!rules.is_empty());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn private_policy_denies_network() {
        let dir = temp_dir("deny-network");
        fs::create_dir_all(&dir).expect("should create dir");
        let policy_path = dir.join("policy.yml");
        fs::write(
            &policy_path,
            "version = 1\ndefault_classification = \"private\"\nnetwork = \"deny\"\n",
        )
        .expect("should write");

        let rules = load_policy_file(&policy_path).expect("should load");
        let engine = crate::PolicyEngine::with_rules(rules);

        let request = crate::PolicyRequest {
            capability: Capability::NetworkRequest,
            classification: DataClassification::Private,
            runtime: agentzero_core::RuntimeTier::HostSupervised,
            context: "test network".into(),
        };
        assert!(!engine.evaluate(&request).is_allowed());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn private_policy_requires_approval_for_shell() {
        let dir = temp_dir("shell-approval");
        fs::create_dir_all(&dir).expect("should create dir");
        let policy_path = dir.join("policy.yml");
        fs::write(
            &policy_path,
            "version = 1\ndefault_classification = \"private\"\nshell_commands = \"require_approval\"\n",
        )
        .expect("should write");

        let rules = load_policy_file(&policy_path).expect("should load");
        let engine = crate::PolicyEngine::with_rules(rules);

        let request = crate::PolicyRequest {
            capability: Capability::ShellCommand,
            classification: DataClassification::Private,
            runtime: agentzero_core::RuntimeTier::HostSupervised,
            context: "test shell".into(),
        };
        match engine.evaluate(&request) {
            agentzero_core::PolicyDecision::RequiresApproval { .. } => {}
            other => panic!("expected RequiresApproval, got {other:?}"),
        }

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn handles_comments_in_policy() {
        let dir = temp_dir("comments");
        fs::create_dir_all(&dir).expect("should create dir");
        let policy_path = dir.join("policy.yml");
        fs::write(
            &policy_path,
            "# AgentZero Policy (private-by-default)\nversion = 1\ndefault_classification = \"private\"\n",
        )
        .expect("should write");

        let rules = load_policy_file(&policy_path);
        assert!(rules.is_ok());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_returns_error() {
        let result = load_policy_file(Path::new("/nonexistent/policy.yml"));
        assert!(result.is_err());
    }
}
