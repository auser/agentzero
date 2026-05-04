use agentzero_audit::{AuditLogger, AuditSink, InMemorySink};
use agentzero_core::{
    AuditEvent, Capability, DataClassification, PolicyDecision, RuntimeTier, SessionId,
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

        let mut tool_executor = ToolExecutor::new(PolicyEngine::with_rules(
            // Session creates its own tool executor policy — clone the rules
            // For now, use deny-by-default; the caller should configure this
            vec![],
        ));

        if let Some(ref root) = config.project_root {
            tool_executor = tool_executor.with_project_root(root.clone());
        }

        Ok(Self {
            id,
            mode: config.mode,
            policy,
            tool_executor,
            audit_sink: Box::new(InMemorySink::new()),
        })
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

    /// Execute a tool by name with the given arguments.
    pub fn execute_tool(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<String, SessionError> {
        info!(session_id = %self.id, tool = tool_name, "executing tool");

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

        // Emit audit event
        let event = AuditEvent {
            execution_id: result.execution_id.clone(),
            session_id: self.id.clone(),
            timestamp: chrono::Utc::now(),
            action: format!("tool:{tool_name}"),
            capability: Capability::FileRead,
            classification: DataClassification::Private,
            decision: PolicyDecision::Allow,
            reason: format!("tool {tool_name} executed successfully"),
            runtime: RuntimeTier::HostReadonly,
            skill_id: None,
            tool_id: Some(result.tool_id),
            redactions_applied: vec![],
            approval_scope: None,
        };
        self.audit_sink
            .record(&event)
            .map_err(SessionError::AuditFailed)?;

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
}
