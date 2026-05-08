use agentzero_audit::{AuditLogger, AuditSink, InMemorySink};
use agentzero_core::{
    AuditEvent, Capability, DataClassification, ExecutionId, PolicyDecision, RedactionResult,
    RuntimeTier, SessionId, SkillId,
};
use agentzero_policy::PolicyEngine;
use agentzero_sandbox::SandboxProfile;
use agentzero_skills::{SkillManifest, SkillRuntime};
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

/// Parameters for emitting an audit event.
struct AuditParams<'a> {
    action: &'a str,
    capability: Capability,
    classification: DataClassification,
    decision: PolicyDecision,
    reason: &'a str,
    redactions: &'a [String],
    runtime: RuntimeTier,
    skill_id: Option<SkillId>,
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
    pub async fn execute_tool(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<String, SessionError> {
        info!(session_id = %self.id, tool = tool_name, "executing tool");

        let capability = match tool_name {
            "read" | "list" | "search" | "query" => Capability::FileRead,
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
                "query" => {
                    let question =
                        args.get("question")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                SessionError::Failed("query: missing 'question' argument".into())
                            })?;
                    let ollama_url = args
                        .get("ollama_url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("http://localhost:11434");
                    let embed_model = args
                        .get("embed_model")
                        .and_then(|v| v.as_str())
                        .unwrap_or("nomic-embed-text");
                    self.tool_executor
                        .query_index(question, ollama_url, embed_model)
                        .await
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

    /// Execute a skill inside its declared runtime sandbox.
    ///
    /// For WASM skills: resolves the module bytes, builds a `SandboxProfile`
    /// from the manifest, checks policy, and delegates to `ToolExecutor::execute_wasm`.
    /// For `InstructionOnly` skills: returns the skill instructions as output.
    /// Other runtime tiers return an error (not yet implemented).
    pub fn execute_skill(
        &self,
        manifest: &SkillManifest,
        wasm_bytes: Option<&[u8]>,
    ) -> Result<String, SessionError> {
        info!(
            session_id = %self.id,
            skill = %manifest.name,
            runtime = ?manifest.runtime,
            "executing skill"
        );

        manifest
            .validate()
            .map_err(|e| SessionError::Failed(e.to_string()))?;

        let tier = manifest.runtime_tier();

        // Policy check: SkillLoad at the skill's runtime tier
        let decision =
            self.check_policy_with_tier(Capability::SkillLoad, DataClassification::Private, tier);
        if !decision.is_allowed() {
            self.emit_audit_event(AuditParams {
                action: &format!("skill_denied:{}", manifest.name),
                capability: Capability::SkillLoad,
                classification: DataClassification::Private,
                decision: decision.clone(),
                reason: &format!("skill {} denied by policy", manifest.name),
                redactions: &[],
                runtime: tier,
                skill_id: Some(manifest.id.clone()),
            })?;
            return Err(SessionError::Failed(format!(
                "skill {} denied: {decision:?}",
                manifest.name
            )));
        }

        match manifest.runtime {
            SkillRuntime::InstructionOnly => {
                self.emit_audit_event(AuditParams {
                    action: &format!("skill:instruction:{}", manifest.name),
                    capability: Capability::SkillLoad,
                    classification: DataClassification::Private,
                    decision: PolicyDecision::Allow,
                    reason: "instruction-only skill loaded",
                    redactions: &[],
                    runtime: RuntimeTier::None,
                    skill_id: Some(manifest.id.clone()),
                })?;
                Ok(format!("skill {} loaded (instruction-only)", manifest.name))
            }
            SkillRuntime::Wasm => {
                let bytes = wasm_bytes.ok_or_else(|| {
                    SessionError::Failed(format!(
                        "skill {} requires WASM module bytes but none provided",
                        manifest.name
                    ))
                })?;

                let profile = self.build_wasm_profile(manifest);
                let result = self
                    .tool_executor
                    .execute_wasm(bytes, &profile)
                    .map_err(|e| SessionError::Failed(e.to_string()))?;

                self.emit_audit_event(AuditParams {
                    action: &format!("skill:wasm:{}", manifest.name),
                    capability: Capability::RuntimeLaunch,
                    classification: DataClassification::Private,
                    decision: PolicyDecision::Allow,
                    reason: &format!("WASM skill {} executed", manifest.name),
                    redactions: &[],
                    runtime: RuntimeTier::WasmSandbox,
                    skill_id: Some(manifest.id.clone()),
                })?;

                Ok(result.output)
            }
            SkillRuntime::HostSupervised => {
                // Resolve entrypoint: manifest field, or run.sh in skill dir
                let entrypoint = manifest.entrypoint.clone().unwrap_or_else(|| {
                    manifest
                        .source
                        .as_ref()
                        .map(|s| match s {
                            agentzero_skills::SkillPackageRef::Local { path } => {
                                format!("{path}/run.sh")
                            }
                            agentzero_skills::SkillPackageRef::GitHub { repo, .. }
                            | agentzero_skills::SkillPackageRef::Registry { name: repo, .. } => {
                                format!("skills/{repo}/run.sh")
                            }
                        })
                        .unwrap_or_else(|| format!("skills/{}/run.sh", manifest.name))
                });

                // Check ShellCommand capability at HostSupervised tier
                let shell_decision = self.check_policy_with_tier(
                    Capability::ShellCommand,
                    DataClassification::Private,
                    RuntimeTier::HostSupervised,
                );
                if !shell_decision.is_allowed() {
                    self.emit_audit_event(AuditParams {
                        action: &format!("skill_shell_denied:{}", manifest.name),
                        capability: Capability::ShellCommand,
                        classification: DataClassification::Private,
                        decision: shell_decision.clone(),
                        reason: &format!(
                            "host-supervised skill {} shell denied by policy",
                            manifest.name
                        ),
                        redactions: &[],
                        runtime: RuntimeTier::HostSupervised,
                        skill_id: Some(manifest.id.clone()),
                    })?;
                    return Err(SessionError::Failed(format!(
                        "skill {} shell execution denied: {shell_decision:?}",
                        manifest.name
                    )));
                }

                info!(
                    session_id = %self.id,
                    skill = %manifest.name,
                    entrypoint = %entrypoint,
                    "executing host-supervised skill"
                );

                let result = self
                    .tool_executor
                    .shell_command(&entrypoint)
                    .map_err(|e| SessionError::Failed(e.to_string()))?;

                self.emit_audit_event(AuditParams {
                    action: &format!("skill:host_supervised:{}", manifest.name),
                    capability: Capability::ShellCommand,
                    classification: DataClassification::Private,
                    decision: PolicyDecision::Allow,
                    reason: &format!(
                        "host-supervised skill {} executed: {}",
                        manifest.name, entrypoint
                    ),
                    redactions: &[],
                    runtime: RuntimeTier::HostSupervised,
                    skill_id: Some(manifest.id.clone()),
                })?;

                Ok(result.output)
            }
            SkillRuntime::Mvm => Err(SessionError::Failed(
                "runtime Mvm not yet supported".to_string(),
            )),
        }
    }

    /// Evaluate a policy request with a specific runtime tier.
    fn check_policy_with_tier(
        &self,
        capability: Capability,
        classification: DataClassification,
        runtime: RuntimeTier,
    ) -> PolicyDecision {
        let request = agentzero_policy::PolicyRequest {
            capability,
            classification,
            runtime,
            context: format!("session:{}", self.id),
        };
        self.policy.evaluate(&request)
    }

    /// Build a WASM sandbox profile from a skill manifest.
    fn build_wasm_profile(&self, manifest: &SkillManifest) -> SandboxProfile {
        use agentzero_sandbox::{SandboxLimit, SandboxNetworkPolicy};

        let capabilities = manifest
            .permissions
            .iter()
            .map(|p| p.capability.clone())
            .collect();

        SandboxProfile {
            runtime: RuntimeTier::WasmSandbox,
            capabilities,
            mounts: vec![], // WASM modules get no filesystem mounts
            network: SandboxNetworkPolicy::Deny,
            limits: SandboxLimit {
                max_duration_secs: 30,
                max_memory_bytes: Some(64 * 1024 * 1024),
                max_cpu_secs: None,
            },
        }
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
        self.emit_audit_event(AuditParams {
            action,
            capability,
            classification,
            decision,
            reason,
            redactions,
            runtime: RuntimeTier::HostReadonly,
            skill_id: None,
        })
    }

    fn emit_audit_event(&self, params: AuditParams<'_>) -> Result<(), SessionError> {
        let event = AuditEvent {
            execution_id: ExecutionId::new(),
            session_id: self.id.clone(),
            timestamp: chrono::Utc::now(),
            action: params.action.to_string(),
            capability: params.capability,
            classification: params.classification,
            decision: params.decision,
            reason: params.reason.to_string(),
            runtime: params.runtime,
            skill_id: params.skill_id,
            tool_id: None,
            redactions_applied: params.redactions.to_vec(),
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

    #[tokio::test]
    async fn execute_read_tool() {
        let session = session_with_read_allowed();
        let args = serde_json::json!({"path": "Cargo.toml"});
        let result = session.execute_tool("read", &args).await;
        assert!(result.is_ok());
        assert!(result.expect("should succeed").contains("[package]"));
    }

    #[tokio::test]
    async fn execute_list_tool() {
        let session = session_with_read_allowed();
        let args = serde_json::json!({"path": "."});
        let result = session.execute_tool("list", &args).await;
        assert!(result.is_ok());
        assert!(result.expect("should succeed").contains("Cargo.toml"));
    }

    #[tokio::test]
    async fn execute_unknown_tool_fails() {
        let session = session_with_read_allowed();
        let args = serde_json::json!({});
        let result = session.execute_tool("delete_everything", &args).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn execute_propose_edit() {
        let session = session_with_read_allowed();
        let args = serde_json::json!({
            "path": "src/lib.rs",
            "description": "Add new facade re-export"
        });
        let result = session.execute_tool("propose_edit", &args).await;
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

    #[test]
    fn execute_instruction_only_skill() {
        use agentzero_skills::{SkillManifest, SkillPermission, SkillRuntime};

        let policy = PolicyEngine::with_rules(vec![PolicyRule::allow(
            Capability::SkillLoad,
            DataClassification::Private,
        )]);
        let session = Session::new(SessionConfig::default(), policy).expect("should create");

        let manifest = SkillManifest {
            id: agentzero_core::SkillId::from_string("test-skill"),
            name: "test-skill".into(),
            version: "0.1.0".into(),
            description: "A test instruction-only skill".into(),
            runtime: SkillRuntime::InstructionOnly,
            permissions: vec![SkillPermission {
                capability: Capability::FileRead,
                reason: "needs file access".into(),
            }],
            source: None,
            entrypoint: None,
        };

        let result = session.execute_skill(&manifest, None);
        assert!(result.is_ok());
        assert!(result.expect("should succeed").contains("instruction-only"));
    }

    #[test]
    fn execute_skill_denied_by_policy() {
        use agentzero_skills::{SkillManifest, SkillRuntime};

        let policy = PolicyEngine::deny_by_default();
        let session = Session::new(SessionConfig::default(), policy).expect("should create");

        let manifest = SkillManifest {
            id: agentzero_core::SkillId::from_string("denied-skill"),
            name: "denied-skill".into(),
            version: "0.1.0".into(),
            description: "Should be denied".into(),
            runtime: SkillRuntime::Wasm,
            permissions: vec![],
            source: None,
            entrypoint: None,
        };

        let result = session.execute_skill(&manifest, Some(&[]));
        assert!(result.is_err());
        let err = result.expect_err("should fail");
        assert!(err.to_string().contains("denied"));
    }

    #[test]
    fn execute_wasm_skill_without_bytes_fails() {
        use agentzero_skills::{SkillManifest, SkillRuntime};

        let policy = PolicyEngine::with_rules(vec![
            PolicyRule::allow(Capability::SkillLoad, DataClassification::Private),
            PolicyRule::allow_runtime(Capability::SkillLoad, RuntimeTier::WasmSandbox),
        ]);
        let session = Session::new(SessionConfig::default(), policy).expect("should create");

        let manifest = SkillManifest {
            id: agentzero_core::SkillId::from_string("wasm-skill"),
            name: "wasm-skill".into(),
            version: "0.1.0".into(),
            description: "WASM skill without bytes".into(),
            runtime: SkillRuntime::Wasm,
            permissions: vec![],
            source: None,
            entrypoint: None,
        };

        let result = session.execute_skill(&manifest, None);
        assert!(result.is_err());
        assert!(result
            .expect_err("should fail")
            .to_string()
            .contains("WASM module bytes"));
    }

    #[test]
    fn execute_unsupported_runtime_fails() {
        use agentzero_skills::{SkillManifest, SkillRuntime};

        let policy = PolicyEngine::with_rules(vec![PolicyRule::allow(
            Capability::SkillLoad,
            DataClassification::Private,
        )]);
        let session = Session::new(SessionConfig::default(), policy).expect("should create");

        let manifest = SkillManifest {
            id: agentzero_core::SkillId::from_string("mvm-skill"),
            name: "mvm-skill".into(),
            version: "0.1.0".into(),
            description: "MVM skill".into(),
            runtime: SkillRuntime::Mvm,
            permissions: vec![],
            source: None,
            entrypoint: None,
        };

        let result = session.execute_skill(&manifest, None);
        assert!(result.is_err());
        assert!(result
            .expect_err("should fail")
            .to_string()
            .contains("not yet supported"));
    }

    #[test]
    fn execute_host_supervised_skill() {
        use agentzero_skills::{SkillManifest, SkillPermission, SkillRuntime};

        let session_policy = PolicyEngine::with_rules(vec![
            PolicyRule::allow(Capability::SkillLoad, DataClassification::Private),
            PolicyRule::allow(Capability::ShellCommand, DataClassification::Private),
        ]);
        // Tool executor needs to actually allow shell
        let tool_policy = PolicyEngine::with_rules(vec![PolicyRule::allow(
            Capability::ShellCommand,
            DataClassification::Private,
        )]);
        let session = Session::new(SessionConfig::default(), session_policy)
            .expect("session should create")
            .with_tool_executor(ToolExecutor::new(tool_policy));

        let manifest = SkillManifest {
            id: agentzero_core::SkillId::from_string("echo-skill"),
            name: "echo-skill".into(),
            version: "0.1.0".into(),
            description: "Simple echo test".into(),
            runtime: SkillRuntime::HostSupervised,
            permissions: vec![SkillPermission {
                capability: Capability::ShellCommand,
                reason: "needs shell".into(),
            }],
            source: None,
            entrypoint: Some("echo hello-from-skill".into()),
        };

        let result = session.execute_skill(&manifest, None);
        assert!(result.is_ok(), "host-supervised should succeed: {result:?}");
        let output = result.expect("should succeed");
        assert!(
            output.contains("hello-from-skill"),
            "should contain echo output: {output}"
        );
    }

    #[test]
    fn execute_host_supervised_denied_by_policy() {
        use agentzero_skills::{SkillManifest, SkillRuntime};

        // Session allows skill loading but tool executor denies shell
        let session_policy = PolicyEngine::with_rules(vec![PolicyRule::allow(
            Capability::SkillLoad,
            DataClassification::Private,
        )]);
        let tool_policy = PolicyEngine::deny_by_default();
        let session = Session::new(SessionConfig::default(), session_policy)
            .expect("session should create")
            .with_tool_executor(ToolExecutor::new(tool_policy));

        let manifest = SkillManifest {
            id: agentzero_core::SkillId::from_string("denied-shell"),
            name: "denied-shell".into(),
            version: "0.1.0".into(),
            description: "Should be denied".into(),
            runtime: SkillRuntime::HostSupervised,
            permissions: vec![],
            source: None,
            entrypoint: Some("echo should-not-run".into()),
        };

        let result = session.execute_skill(&manifest, None);
        assert!(result.is_err());
        assert!(result
            .expect_err("should fail")
            .to_string()
            .contains("denied"));
    }

    /// Minimal WASM module: exports `main() -> i32` returning 42.
    #[cfg(feature = "wasm")]
    fn minimal_wasm_module() -> Vec<u8> {
        vec![
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
            0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7F, // type section
            0x03, 0x02, 0x01, 0x00, // function section
            0x05, 0x03, 0x01, 0x00, 0x01, // memory section
            0x07, 0x11, 0x02, 0x04, 0x6D, 0x61, 0x69, 0x6E, 0x00, 0x00, 0x06, 0x6D, 0x65, 0x6D,
            0x6F, 0x72, 0x79, 0x02, 0x00, // export section
            0x0A, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2A, 0x0B, // code section
        ]
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn execute_wasm_skill_end_to_end() {
        use agentzero_skills::{SkillManifest, SkillPermission, SkillRuntime};

        // Policy: allow skill loading and WASM runtime launch
        let session_policy = PolicyEngine::with_rules(vec![
            PolicyRule::allow(Capability::SkillLoad, DataClassification::Private),
            PolicyRule::allow_runtime(Capability::SkillLoad, RuntimeTier::WasmSandbox),
        ]);
        let tool_policy = PolicyEngine::with_rules(vec![PolicyRule::allow_runtime(
            Capability::RuntimeLaunch,
            RuntimeTier::WasmSandbox,
        )]);
        let session = Session::new(SessionConfig::default(), session_policy)
            .expect("session should create")
            .with_tool_executor(ToolExecutor::new(tool_policy));

        let manifest = SkillManifest {
            id: agentzero_core::SkillId::from_string("wasm-test"),
            name: "wasm-test".into(),
            version: "0.1.0".into(),
            description: "Integration test WASM skill".into(),
            runtime: SkillRuntime::Wasm,
            permissions: vec![SkillPermission {
                capability: Capability::FileRead,
                reason: "test permission".into(),
            }],
            source: None,
            entrypoint: None,
        };

        let wasm_bytes = minimal_wasm_module();
        let result = session.execute_skill(&manifest, Some(&wasm_bytes));
        assert!(result.is_ok(), "WASM skill should succeed: {result:?}");
        let output = result.expect("should succeed");
        assert!(
            output.contains("42"),
            "output should contain exit code 42: {output}"
        );
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn execute_wasm_skill_denied_without_runtime_policy() {
        use agentzero_skills::{SkillManifest, SkillRuntime};

        // Allow skill loading but NOT runtime launch
        let session_policy = PolicyEngine::with_rules(vec![
            PolicyRule::allow(Capability::SkillLoad, DataClassification::Private),
            PolicyRule::allow_runtime(Capability::SkillLoad, RuntimeTier::WasmSandbox),
        ]);
        let tool_policy = PolicyEngine::deny_by_default(); // no RuntimeLaunch rule
        let session = Session::new(SessionConfig::default(), session_policy)
            .expect("session should create")
            .with_tool_executor(ToolExecutor::new(tool_policy));

        let manifest = SkillManifest {
            id: agentzero_core::SkillId::from_string("wasm-denied"),
            name: "wasm-denied".into(),
            version: "0.1.0".into(),
            description: "Should fail at runtime launch".into(),
            runtime: SkillRuntime::Wasm,
            permissions: vec![],
            source: None,
            entrypoint: None,
        };

        let wasm_bytes = minimal_wasm_module();
        let result = session.execute_skill(&manifest, Some(&wasm_bytes));
        assert!(result.is_err(), "should be denied");
        assert!(result
            .expect_err("should fail")
            .to_string()
            .contains("denied"));
    }
}
