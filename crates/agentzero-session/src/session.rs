use agentzero_audit::{AuditLogger, AuditSink, InMemorySink};
use agentzero_core::{
    AuditEvent, Capability, DataClassification, ExecutionId, PolicyDecision, RedactionResult,
    RuntimeTier, SessionId,
};
use agentzero_policy::PolicyEngine;
use agentzero_tracing::{info, warn};
use thiserror::Error;

use crate::provider::{ModelLocation, ModelProvider};
use crate::tool_exec::ToolExecutor;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session not initialized: {0}")]
    NotInitialized(String),
    #[error("session failed: {0}")]
    Failed(String),
    #[error("audit error: {0}")]
    AuditFailed(String),
}

/// Session operating mode per ADR 0002.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    /// Local models only. No network calls.
    LocalOnly,
    /// Prefer local but allow remote with policy checks.
    LocalPreferred,
}

/// Configuration for creating a session.
pub struct SessionConfig {
    pub mode: SessionMode,
    pub project_root: Option<String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            mode: SessionMode::LocalOnly,
            project_root: None,
        }
    }
}

/// A supervised agent session.
///
/// Ties together a model provider, tool executor, policy engine, and audit sink.
/// Every action passes through policy evaluation and emits audit events.
pub struct Session {
    id: SessionId,
    mode: SessionMode,
    policy: PolicyEngine,
    tool_executor: ToolExecutor,
    audit_sink: Box<dyn AuditSink>,
}

impl Session {
    /// Create a new session with the given configuration.
    pub fn new(config: SessionConfig, policy: PolicyEngine) -> Result<Self, SessionError> {
        let id = SessionId::new();
        info!(session_id = %id, mode = ?config.mode, "creating new session");

        let mut tool_executor = ToolExecutor::new(PolicyEngine::with_rules(vec![]));

        if let Some(ref root) = config.project_root {
            tool_executor = tool_executor.with_project_root(root.clone());
        }

        let session = Self {
            id,
            mode: config.mode,
            policy,
            tool_executor,
            audit_sink: Box::new(InMemorySink::new()),
        };

        // Audit: session start
        session.emit_lifecycle_event("session_start", "session created")?;

        Ok(session)
    }

    /// Create a session with a file-backed audit logger.
    pub fn with_audit_dir(mut self, audit_dir: &std::path::Path) -> Result<Self, SessionError> {
        let logger = AuditLogger::new(audit_dir, self.id.as_str())
            .map_err(|e| SessionError::AuditFailed(e.to_string()))?;
        self.audit_sink = Box::new(logger);
        Ok(self)
    }

    /// Override the tool executor (e.g., to inject custom policy rules).
    pub fn with_tool_executor(mut self, executor: ToolExecutor) -> Self {
        self.tool_executor = executor;
        self
    }

    /// Return the session ID.
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    /// Return the session mode.
    pub fn mode(&self) -> SessionMode {
        self.mode
    }

    /// Check whether a model provider is compatible with this session's mode.
    pub fn accepts_provider(&self, provider: &dyn ModelProvider) -> bool {
        match self.mode {
            SessionMode::LocalOnly => provider.location() == ModelLocation::Local,
            SessionMode::LocalPreferred => true,
        }
    }

    /// Check policy and redact content before sending to a model provider.
    ///
    /// Returns the (possibly redacted) content and the list of redactions applied.
    /// Returns Err if the content is denied for this provider.
    pub fn prepare_for_model(
        &self,
        content: &str,
        classification: DataClassification,
        provider: &dyn ModelProvider,
    ) -> Result<(String, Vec<String>), SessionError> {
        // Local providers accept everything
        if provider.location() == ModelLocation::Local {
            self.emit_audit(
                "model_call_local",
                Capability::ModelCall,
                classification,
                PolicyDecision::Allow,
                "local provider — no redaction needed",
                &[],
            )?;
            return Ok((content.to_string(), vec![]));
        }

        // Remote providers: check policy
        let decision = self.check_policy(Capability::ModelCall, classification);

        match decision.clone() {
            PolicyDecision::Allow => {
                self.emit_audit(
                    "model_call_remote",
                    Capability::ModelCall,
                    classification,
                    PolicyDecision::Allow,
                    "remote model call allowed by policy",
                    &[],
                )?;
                Ok((content.to_string(), vec![]))
            }
            PolicyDecision::AllowWithRedaction { reason } => {
                info!(
                    session_id = %self.id,
                    reason = %reason,
                    "redacting content before remote model call"
                );

                let redacted = self.redact_content(content);
                let redaction_labels: Vec<String> = redacted
                    .redactions
                    .iter()
                    .map(|r| format!("{:?}@{}:{}", r.classification, r.start, r.end))
                    .collect();

                self.emit_audit(
                    "model_call_remote_redacted",
                    Capability::ModelCall,
                    classification,
                    PolicyDecision::AllowWithRedaction {
                        reason: reason.clone(),
                    },
                    "content redacted before remote model call",
                    &redaction_labels,
                )?;

                Ok((redacted.apply(content), redaction_labels))
            }
            PolicyDecision::Deny { reason } => {
                warn!(
                    session_id = %self.id,
                    reason = %reason,
                    "remote model call denied"
                );
                self.emit_audit(
                    "model_call_denied",
                    Capability::ModelCall,
                    classification,
                    PolicyDecision::Deny {
                        reason: reason.clone(),
                    },
                    &reason,
                    &[],
                )?;
                Err(SessionError::Failed(format!("model call denied: {reason}")))
            }
            PolicyDecision::RequiresApproval { reason } => {
                self.emit_audit(
                    "model_call_requires_approval",
                    Capability::ModelCall,
                    classification,
                    PolicyDecision::RequiresApproval {
                        reason: reason.clone(),
                    },
                    &reason,
                    &[],
                )?;
                Err(SessionError::Failed(format!(
                    "model call requires approval: {reason}"
                )))
            }
        }
    }

    /// Execute a tool by name with the given arguments.
    pub fn execute_tool(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<String, SessionError> {
        info!(session_id = %self.id, tool = tool_name, "executing tool");

        let capability = match tool_name {
            "read" | "list" | "search" => Capability::FileRead,
            "write" | "propose_edit" => Capability::FileWrite,
            "shell" => Capability::ShellCommand,
            _ => Capability::FileRead,
        };

        let result =
            match tool_name {
                "read" => {
                    let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                        SessionError::Failed("read: missing 'path' argument".into())
                    })?;
                    self.tool_executor
                        .read_file(path)
                        .map_err(|e| SessionError::Failed(e.to_string()))?
                }
                "list" => {
                    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                    self.tool_executor
                        .list_dir(path)
                        .map_err(|e| SessionError::Failed(e.to_string()))?
                }
                "search" => {
                    let pattern =
                        args.get("pattern")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                SessionError::Failed("search: missing 'pattern' argument".into())
                            })?;
                    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                    self.tool_executor
                        .search_files(path, pattern)
                        .map_err(|e| SessionError::Failed(e.to_string()))?
                }
                "propose_edit" => {
                    let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                        SessionError::Failed("propose_edit: missing 'path' argument".into())
                    })?;
                    let description = args
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(no description)");
                    self.tool_executor
                        .propose_edit(path, description)
                        .map_err(|e| SessionError::Failed(e.to_string()))?
                }
                "write" => {
                    let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                        SessionError::Failed("write: missing 'path' argument".into())
                    })?;
                    let content =
                        args.get("content")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                SessionError::Failed("write: missing 'content' argument".into())
                            })?;
                    self.tool_executor
                        .write_file(path, content)
                        .map_err(|e| SessionError::Failed(e.to_string()))?
                }
                "shell" => {
                    let command =
                        args.get("command")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                SessionError::Failed("shell: missing 'command' argument".into())
                            })?;
                    self.tool_executor
                        .shell_command(command)
                        .map_err(|e| SessionError::Failed(e.to_string()))?
                }
                other => {
                    warn!(session_id = %self.id, tool = other, "unknown tool");
                    return Err(SessionError::Failed(format!("unknown tool: {other}")));
                }
            };

        // Audit: tool execution
        self.emit_audit(
            &format!("tool:{tool_name}"),
            capability,
            DataClassification::Private,
            PolicyDecision::Allow,
            &format!("tool {tool_name} executed successfully"),
            &[],
        )?;

        Ok(result.output)
    }

    /// Evaluate a policy request through the session's engine.
    pub fn check_policy(
        &self,
        capability: Capability,
        classification: DataClassification,
    ) -> PolicyDecision {
        let request = agentzero_policy::PolicyRequest {
            capability,
            classification,
            runtime: RuntimeTier::HostReadonly,
            context: format!("session:{}", self.id),
        };
        self.policy.evaluate(&request)
    }

    /// Signal session end and emit audit event.
    pub fn end(&self) -> Result<(), SessionError> {
        self.emit_lifecycle_event("session_end", "session ended by user")
    }

    /// Scan content for sensitive patterns and return redaction result.
    fn redact_content(&self, content: &str) -> RedactionResult {
        let mut redactions = Vec::new();
        let lower = content.to_lowercase();

        // Simple pattern-based redaction for common PII/secret patterns
        let patterns: &[(&str, DataClassification)] = &[
            ("@gmail.com", DataClassification::Pii),
            ("@yahoo.com", DataClassification::Pii),
            ("@hotmail.com", DataClassification::Pii),
            ("@outlook.com", DataClassification::Pii),
            ("ghp_", DataClassification::Secret),
            ("gho_", DataClassification::Secret),
            ("sk-", DataClassification::Secret),
            ("AKIA", DataClassification::Secret),
        ];

        for (pattern, classification) in patterns {
            let pattern_lower = pattern.to_lowercase();
            let mut search_from = 0;
            while let Some(pos) = lower[search_from..].find(&pattern_lower) {
                let abs_pos = search_from + pos;
                // Find word boundary (extend to whitespace or end)
                let end = content[abs_pos..]
                    .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',')
                    .map_or(content.len(), |e| abs_pos + e);

                let idx = redactions.len();
                redactions.push(agentzero_core::Redaction {
                    start: abs_pos,
                    end,
                    classification: *classification,
                    placeholder: agentzero_core::placeholder_for(*classification, idx),
                });
                search_from = end;
            }
        }

        redactions.sort_by_key(|r| r.start);
        RedactionResult { redactions }
    }

    fn emit_lifecycle_event(&self, action: &str, reason: &str) -> Result<(), SessionError> {
        self.emit_audit(
            action,
            Capability::SkillLoad, // neutral capability for lifecycle
            DataClassification::Private,
            PolicyDecision::Allow,
            reason,
            &[],
        )
    }

    fn emit_audit(
        &self,
        action: &str,
        capability: Capability,
        classification: DataClassification,
        decision: PolicyDecision,
        reason: &str,
        redactions: &[String],
    ) -> Result<(), SessionError> {
        let event = AuditEvent {
            execution_id: ExecutionId::new(),
            session_id: self.id.clone(),
            timestamp: chrono::Utc::now(),
            action: action.to_string(),
            capability,
            classification,
            decision,
            reason: reason.to_string(),
            runtime: RuntimeTier::HostReadonly,
            skill_id: None,
            tool_id: None,
            redactions_applied: redactions.to_vec(),
            approval_scope: None,
        };
        self.audit_sink
            .record(&event)
            .map_err(SessionError::AuditFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::LocalStubProvider;
    use agentzero_policy::PolicyRule;

    fn session_with_read_allowed() -> Session {
        let policy = PolicyEngine::with_rules(vec![PolicyRule::allow(
            Capability::FileRead,
            DataClassification::Private,
        )]);
        let tool_policy = PolicyEngine::with_rules(vec![PolicyRule::allow(
            Capability::FileRead,
            DataClassification::Private,
        )]);
        Session::new(SessionConfig::default(), policy)
            .expect("session should initialize")
            .with_tool_executor(ToolExecutor::new(tool_policy))
    }

    #[test]
    fn session_creates_with_unique_id() {
        let s1 = Session::new(SessionConfig::default(), PolicyEngine::deny_by_default())
            .expect("should create");
        let s2 = Session::new(SessionConfig::default(), PolicyEngine::deny_by_default())
            .expect("should create");
        assert_ne!(s1.id().as_str(), s2.id().as_str());
    }

    #[test]
    fn session_default_mode_is_local_only() {
        let session = Session::new(SessionConfig::default(), PolicyEngine::deny_by_default())
            .expect("should create");
        assert_eq!(session.mode(), SessionMode::LocalOnly);
    }

    #[test]
    fn local_only_rejects_remote_provider() {
        let session = Session::new(SessionConfig::default(), PolicyEngine::deny_by_default())
            .expect("should create");
        let local = LocalStubProvider;
        assert!(session.accepts_provider(&local));
    }

    #[test]
    fn execute_read_tool() {
        let session = session_with_read_allowed();
        let args = serde_json::json!({"path": "Cargo.toml"});
        let result = session.execute_tool("read", &args);
        assert!(result.is_ok());
        assert!(result.expect("should succeed").contains("[package]"));
    }

    #[test]
    fn execute_list_tool() {
        let session = session_with_read_allowed();
        let args = serde_json::json!({"path": "."});
        let result = session.execute_tool("list", &args);
        assert!(result.is_ok());
        assert!(result.expect("should succeed").contains("Cargo.toml"));
    }

    #[test]
    fn execute_unknown_tool_fails() {
        let session = session_with_read_allowed();
        let args = serde_json::json!({});
        let result = session.execute_tool("delete_everything", &args);
        assert!(result.is_err());
    }

    #[test]
    fn execute_propose_edit() {
        let session = session_with_read_allowed();
        let args = serde_json::json!({
            "path": "src/lib.rs",
            "description": "Add new facade re-export"
        });
        let result = session.execute_tool("propose_edit", &args);
        assert!(result.is_ok());
        let output = result.expect("should succeed");
        assert!(output.contains("PROPOSED EDIT"));
        assert!(output.contains("requires approval"));
    }

    #[test]
    fn policy_check_through_session() {
        let policy = PolicyEngine::with_rules(vec![PolicyRule::require_approval(
            Capability::ShellCommand,
            "shell requires approval",
        )]);
        let session = Session::new(SessionConfig::default(), policy).expect("should create");
        let decision = session.check_policy(Capability::ShellCommand, DataClassification::Private);
        match decision {
            PolicyDecision::RequiresApproval { .. } => {}
            other => panic!("expected RequiresApproval, got {other:?}"),
        }
    }

    #[test]
    fn prepare_for_local_model_passes_everything() {
        let session = Session::new(SessionConfig::default(), PolicyEngine::deny_by_default())
            .expect("should create");
        let local = LocalStubProvider;
        let result = session.prepare_for_model(
            "secret AKIA1234567890123456",
            DataClassification::Secret,
            &local,
        );
        assert!(result.is_ok());
        let (content, redactions) = result.expect("should succeed");
        // Local provider: no redaction
        assert!(content.contains("AKIA"));
        assert!(redactions.is_empty());
    }

    #[test]
    fn redact_content_finds_pii() {
        let session = Session::new(SessionConfig::default(), PolicyEngine::deny_by_default())
            .expect("should create");
        let result = session.redact_content("contact me at user@gmail.com please");
        assert!(!result.is_clean());
        let redacted = result.apply("contact me at user@gmail.com please");
        assert!(!redacted.contains("@gmail.com"));
        assert!(redacted.contains("[PII_"));
    }

    #[test]
    fn redact_content_finds_secrets() {
        let session = Session::new(SessionConfig::default(), PolicyEngine::deny_by_default())
            .expect("should create");
        let result = session.redact_content("token is ghp_ABCDabcd1234567890abcdef1234567890");
        assert!(!result.is_clean());
        let redacted = result.apply("token is ghp_ABCDabcd1234567890abcdef1234567890");
        assert!(!redacted.contains("ghp_"));
        assert!(redacted.contains("[SECRET_"));
    }

    #[test]
    fn session_end_emits_event() {
        let session = Session::new(SessionConfig::default(), PolicyEngine::deny_by_default())
            .expect("should create");
        assert!(session.end().is_ok());
    }
}
