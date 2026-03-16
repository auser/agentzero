//! AgentZero FFI — Foreign function interface for Swift, Kotlin, Python, and TypeScript.
//!
//! This crate exposes the AgentZero agent runtime to non-Rust languages via:
//! - **UniFFI** (default feature) — generates Swift, Kotlin, and Python bindings
//! - **napi-rs** (`node` feature) — generates a native Node.js addon with TypeScript types

// UniFFI scaffolding must live in the crate root so the generated `UniFfiTag`
// type is visible to all derive macros in this crate.
#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

#[cfg(feature = "node")]
#[allow(dead_code)]
mod node_bindings;

#[cfg(feature = "privacy")]
pub mod privacy_types;

use agentzero_core::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;

// ── Global async runtime ───────────────────────────────────────────────────

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime")
    })
}

// ── FFI-safe types ─────────────────────────────────────────────────────────

/// Configuration for creating an AgentZero controller.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct AgentZeroConfig {
    pub config_path: String,
    pub workspace_root: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub profile: Option<String>,
}

/// Response from sending a message to the agent.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct AgentResponse {
    pub text: String,
    pub metrics_json: String,
}

/// A single message in the conversation history.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp_ms: i64,
}

/// Current status of the agent controller.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum AgentStatus {
    Idle,
    Running,
    Error { message: String },
}

/// Errors returned by the FFI layer.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum AgentZeroError {
    ConfigError { message: String },
    RuntimeError { message: String },
    ProviderError { message: String },
    TimeoutError { timeout_ms: u64 },
}

impl std::fmt::Display for AgentZeroError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigError { message } => write!(f, "config error: {message}"),
            Self::RuntimeError { message } => write!(f, "runtime error: {message}"),
            Self::ProviderError { message } => write!(f, "provider error: {message}"),
            Self::TimeoutError { timeout_ms } => {
                write!(f, "request timed out after {timeout_ms} ms")
            }
        }
    }
}

impl std::error::Error for AgentZeroError {}

// ── Tool callback interface ───────────────────────────────────────────────

/// Trait for foreign-language tool implementations.
///
/// Implement this trait in Swift, Kotlin, Python, or any other UniFFI-supported
/// language to register custom tools that run alongside native and WASM tools.
///
/// # Safety
///
/// FFI tools run in the host process (not sandboxed like WASM plugins).
/// They have full access to the process memory. This is by design — FFI
/// users are embedding the runtime in their own application and trust
/// their own code.
#[cfg_attr(feature = "uniffi", uniffi::export(callback_interface))]
pub trait ToolCallback: Send + Sync {
    /// Execute the tool with the given JSON input and workspace root.
    /// Returns a JSON string on success or an error message on failure.
    fn execute(&self, input: String, workspace_root: String) -> Result<String, String>;

    /// Optional: return a human-readable description of the tool.
    fn description(&self) -> String {
        String::new()
    }
}

/// A tool backed by a foreign callback (Swift, Kotlin, Python, etc.).
///
/// Wraps a `ToolCallback` implementor and bridges it to the `Tool` trait
/// used by the agent loop. Each `FfiTool` occupies one slot in the
/// agent's tool list, identical to native and WASM tools.
pub struct FfiTool {
    /// Leaked string for the `&'static str` requirement of `Tool::name()`.
    name: &'static str,
    /// Leaked description from the callback.
    description: &'static str,
    callback: Arc<dyn ToolCallback>,
}

impl FfiTool {
    /// Create an `FfiTool` from a name and callback.
    pub fn new(name: String, callback: Arc<dyn ToolCallback>) -> Self {
        let leaked_name: &'static str = Box::leak(name.into_boxed_str());
        let leaked_desc: &'static str = Box::leak(callback.description().into_boxed_str());
        Self {
            name: leaked_name,
            description: leaked_desc,
            callback,
        }
    }
}

impl std::fmt::Debug for FfiTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FfiTool").field("name", &self.name).finish()
    }
}

#[async_trait]
impl Tool for FfiTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let callback = self.callback.clone();
        let input_owned = input.to_string();
        let workspace = ctx.workspace_root.clone();

        // Run the callback on a blocking thread since foreign callbacks
        // may perform I/O or other blocking operations.
        let result = tokio::task::spawn_blocking(move || callback.execute(input_owned, workspace))
            .await
            .map_err(|e| anyhow::anyhow!("FFI tool task panicked: {e}"))?;

        match result {
            Ok(output) => Ok(ToolResult { output }),
            Err(err) => Err(anyhow::anyhow!("FFI tool error: {err}")),
        }
    }
}

// ── Controller ─────────────────────────────────────────────────────────────

/// Main AgentZero controller exposed to foreign languages.
#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
pub struct AgentZeroController {
    config: Mutex<AgentZeroConfig>,
    status: Mutex<AgentStatus>,
    history: Mutex<Vec<ChatMessage>>,
    /// FFI-registered tools. These are cloned into each `RunAgentRequest`.
    registered_tools: Mutex<Vec<Arc<FfiTool>>>,
}

// UniFFI-exported public API.
//
// `uniffi::export` is a proc macro that cannot be applied via `cfg_attr`
// because inner attributes like `#[uniffi::constructor]` would not be
// expanded before the proc macro processes them. We therefore gate the
// entire impl block on the `uniffi` feature and use a mirror block below
// for the non-UniFFI case.
#[cfg(feature = "uniffi")]
#[uniffi::export]
impl AgentZeroController {
    #[uniffi::constructor]
    pub fn new(config: AgentZeroConfig) -> Arc<Self> {
        Self::init(config)
    }

    #[uniffi::constructor]
    pub fn with_defaults(config_path: String, workspace_root: String) -> Arc<Self> {
        Self::init(AgentZeroConfig {
            config_path,
            workspace_root,
            provider: None,
            model: None,
            profile: None,
        })
    }

    pub fn status(&self) -> AgentStatus {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or(AgentStatus::Error {
                message: "failed to acquire status lock".to_string(),
            })
    }

    pub fn send_message(&self, message: String) -> Result<AgentResponse, AgentZeroError> {
        self.send_message_impl(message)
    }

    pub fn get_history(&self) -> Vec<ChatMessage> {
        self.history.lock().map(|h| h.clone()).unwrap_or_default()
    }

    pub fn clear_history(&self) {
        if let Ok(mut h) = self.history.lock() {
            h.clear();
        }
    }

    pub fn get_config(&self) -> Result<AgentZeroConfig, AgentZeroError> {
        self.config
            .lock()
            .map(|c| c.clone())
            .map_err(|_| AgentZeroError::RuntimeError {
                message: "failed to acquire config lock".to_string(),
            })
    }

    pub fn update_config(&self, config: AgentZeroConfig) -> Result<(), AgentZeroError> {
        let mut current = self
            .config
            .lock()
            .map_err(|_| AgentZeroError::RuntimeError {
                message: "failed to acquire config lock".to_string(),
            })?;
        *current = config;
        Ok(())
    }

    pub fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    /// Register a custom tool implemented in a foreign language.
    ///
    /// The tool will be available in the agent's tool list alongside native
    /// and WASM tools. The callback's `execute` method will be invoked when
    /// the agent uses this tool.
    pub fn register_tool(
        &self,
        name: String,
        callback: Box<dyn ToolCallback>,
    ) -> Result<(), AgentZeroError> {
        self.register_tool_impl(name, Arc::from(callback))
    }

    /// List the names of all registered FFI tools.
    pub fn registered_tool_names(&self) -> Vec<String> {
        self.registered_tools
            .lock()
            .map(|tools| tools.iter().map(|t| t.name.to_string()).collect())
            .unwrap_or_default()
    }
}

// Non-UniFFI public API — identical signatures, no proc-macro annotations.
#[cfg(not(feature = "uniffi"))]
impl AgentZeroController {
    pub fn new(config: AgentZeroConfig) -> Arc<Self> {
        Self::init(config)
    }

    pub fn with_defaults(config_path: String, workspace_root: String) -> Arc<Self> {
        Self::init(AgentZeroConfig {
            config_path,
            workspace_root,
            provider: None,
            model: None,
            profile: None,
        })
    }

    pub fn status(&self) -> AgentStatus {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or(AgentStatus::Error {
                message: "failed to acquire status lock".to_string(),
            })
    }

    pub fn send_message(&self, message: String) -> Result<AgentResponse, AgentZeroError> {
        self.send_message_impl(message)
    }

    pub fn get_history(&self) -> Vec<ChatMessage> {
        self.history.lock().map(|h| h.clone()).unwrap_or_default()
    }

    pub fn clear_history(&self) {
        if let Ok(mut h) = self.history.lock() {
            h.clear();
        }
    }

    pub fn get_config(&self) -> Result<AgentZeroConfig, AgentZeroError> {
        self.config
            .lock()
            .map(|c| c.clone())
            .map_err(|_| AgentZeroError::RuntimeError {
                message: "failed to acquire config lock".to_string(),
            })
    }

    pub fn update_config(&self, config: AgentZeroConfig) -> Result<(), AgentZeroError> {
        let mut current = self
            .config
            .lock()
            .map_err(|_| AgentZeroError::RuntimeError {
                message: "failed to acquire config lock".to_string(),
            })?;
        *current = config;
        Ok(())
    }

    pub fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    pub fn register_tool(
        &self,
        name: String,
        callback: Box<dyn ToolCallback>,
    ) -> Result<(), AgentZeroError> {
        self.register_tool_impl(name, Arc::from(callback))
    }

    pub fn registered_tool_names(&self) -> Vec<String> {
        self.registered_tools
            .lock()
            .map(|tools| tools.iter().map(|t| t.name.to_string()).collect())
            .unwrap_or_default()
    }
}

// Private implementation shared by both API variants.
impl AgentZeroController {
    fn init(config: AgentZeroConfig) -> Arc<Self> {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("agentzero=info")
            .try_init();

        Arc::new(Self {
            config: Mutex::new(config),
            status: Mutex::new(AgentStatus::Idle),
            history: Mutex::new(Vec::new()),
            registered_tools: Mutex::new(Vec::new()),
        })
    }

    fn send_message_impl(&self, message: String) -> Result<AgentResponse, AgentZeroError> {
        // Mark as running
        if let Ok(mut s) = self.status.lock() {
            *s = AgentStatus::Running;
        }

        let config =
            self.config
                .lock()
                .map(|c| c.clone())
                .map_err(|_| AgentZeroError::RuntimeError {
                    message: "failed to acquire config lock".to_string(),
                })?;

        // Record user message
        let now_ms = current_timestamp_ms();
        if let Ok(mut h) = self.history.lock() {
            h.push(ChatMessage {
                role: "user".to_string(),
                content: message.clone(),
                timestamp_ms: now_ms,
            });
        }

        // Collect registered FFI tools as Box<dyn Tool> for the request.
        let extra_tools: Vec<Box<dyn Tool>> = self
            .registered_tools
            .lock()
            .map(|tools| {
                tools
                    .iter()
                    .map(|t| -> Box<dyn Tool> {
                        Box::new(FfiTool::new(t.name.to_string(), t.callback.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let req = agentzero_infra::runtime::RunAgentRequest {
            workspace_root: config.workspace_root.into(),
            config_path: config.config_path.into(),
            message: message.clone(),
            provider_override: config.provider.clone(),
            model_override: config.model.clone(),
            profile_override: config.profile.clone(),
            extra_tools,
            conversation_id: None,
            agent_store: None,
        };

        let result = runtime().block_on(agentzero_infra::runtime::run_agent_once(req));

        // Back to idle
        if let Ok(mut s) = self.status.lock() {
            *s = AgentStatus::Idle;
        }

        match result {
            Ok(output) => {
                // Record assistant message
                if let Ok(mut h) = self.history.lock() {
                    h.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: output.response_text.clone(),
                        timestamp_ms: current_timestamp_ms(),
                    });
                }

                Ok(AgentResponse {
                    text: output.response_text,
                    metrics_json: output.metrics_snapshot.to_string(),
                })
            }
            Err(e) => {
                let err_msg = format!("{e:#}");
                if let Ok(mut s) = self.status.lock() {
                    *s = AgentStatus::Error {
                        message: err_msg.clone(),
                    };
                }
                Err(classify_error(e))
            }
        }
    }

    fn register_tool_impl(
        &self,
        name: String,
        callback: Arc<dyn ToolCallback>,
    ) -> Result<(), AgentZeroError> {
        let mut tools = self
            .registered_tools
            .lock()
            .map_err(|_| AgentZeroError::RuntimeError {
                message: "failed to acquire registered_tools lock".to_string(),
            })?;

        // Reject duplicate names
        if tools.iter().any(|t| t.name == name.as_str()) {
            return Err(AgentZeroError::RuntimeError {
                message: format!("tool '{}' is already registered", name),
            });
        }

        tools.push(Arc::new(FfiTool::new(name, callback)));
        Ok(())
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn current_timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn classify_error(e: anyhow::Error) -> AgentZeroError {
    let msg = format!("{e:#}");
    let lower = msg.to_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        AgentZeroError::TimeoutError { timeout_ms: 0 }
    } else if lower.contains("provider") || lower.contains("api key") || lower.contains("api_key") {
        AgentZeroError::ProviderError { message: msg }
    } else if lower.contains("config") || lower.contains("toml") || lower.contains("not found") {
        AgentZeroError::ConfigError { message: msg }
    } else {
        AgentZeroError::RuntimeError { message: msg }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test config with a non-existent workspace root inside a temp
    /// directory. The `TempDir` guard must be held alive for the test's
    /// duration; it is automatically cleaned up when dropped.
    ///
    /// The workspace points to a non-existent subdirectory so that the config
    /// loader's `canonicalize(allowed_root)` always fails — regardless of env
    /// vars like `ANTHROPIC_API_KEY` that would otherwise let defaults succeed.
    fn test_config() -> (AgentZeroConfig, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let workspace = dir.path().join("nonexistent").to_string_lossy().to_string();
        let config = AgentZeroConfig {
            config_path: format!("{workspace}/agentzero.toml"),
            workspace_root: workspace,
            provider: None,
            model: None,
            profile: None,
        };
        (config, dir)
    }

    #[test]
    fn controller_creation_success_path() {
        let (config, _dir) = test_config();
        let controller = AgentZeroController::new(config);
        assert!(matches!(controller.status(), AgentStatus::Idle));
    }

    #[test]
    fn controller_with_defaults_success_path() {
        let controller = AgentZeroController::with_defaults(
            "/tmp/test/agentzero.toml".to_string(),
            "/tmp/test".to_string(),
        );
        assert!(matches!(controller.status(), AgentStatus::Idle));
    }

    #[test]
    fn version_returns_crate_version() {
        let (config, _dir) = test_config();
        let controller = AgentZeroController::new(config);
        let version = controller.version();
        assert!(!version.is_empty());
        assert!(version.contains('.'));
    }

    #[test]
    fn history_starts_empty() {
        let (config, _dir) = test_config();
        let controller = AgentZeroController::new(config);
        assert!(controller.get_history().is_empty());
    }

    #[test]
    fn clear_history_empties_messages() {
        let (config, _dir) = test_config();
        let controller = AgentZeroController::new(config);
        if let Ok(mut h) = controller.history.lock() {
            h.push(ChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
                timestamp_ms: 0,
            });
        }
        assert_eq!(controller.get_history().len(), 1);
        controller.clear_history();
        assert!(controller.get_history().is_empty());
    }

    #[test]
    fn get_and_update_config_success_path() {
        let (config, _dir) = test_config();
        let controller = AgentZeroController::new(config);
        let mut cfg = controller.get_config().unwrap();
        assert!(cfg.provider.is_none());

        cfg.provider = Some("anthropic".to_string());
        controller.update_config(cfg).unwrap();

        let updated = controller.get_config().unwrap();
        assert_eq!(updated.provider, Some("anthropic".to_string()));
    }

    #[test]
    fn send_message_returns_error_for_invalid_setup() {
        let (config, _dir) = test_config();
        let controller = AgentZeroController::new(config);
        let result = controller.send_message("hello".to_string());
        assert!(result.is_err());
        let err = result.unwrap_err();
        // May fail at config, provider, or runtime stage depending on environment.
        assert!(
            matches!(
                err,
                AgentZeroError::ConfigError { .. }
                    | AgentZeroError::RuntimeError { .. }
                    | AgentZeroError::ProviderError { .. }
                    | AgentZeroError::TimeoutError { .. }
            ),
            "expected config, runtime, provider, or timeout error, got: {err}"
        );
    }

    #[test]
    fn status_transitions_through_send_message() {
        let (config, _dir) = test_config();
        let controller = AgentZeroController::new(config);
        assert!(matches!(controller.status(), AgentStatus::Idle));

        let _ = controller.send_message("test".to_string());
        let status = controller.status();
        assert!(
            matches!(status, AgentStatus::Error { .. } | AgentStatus::Idle),
            "expected Error or Idle after failed send, got: {status:?}"
        );
    }

    #[test]
    fn classify_error_detects_timeout() {
        let e = anyhow::anyhow!("request timed out after 30000 ms");
        assert!(matches!(
            classify_error(e),
            AgentZeroError::TimeoutError { .. }
        ));
    }

    #[test]
    fn classify_error_detects_provider() {
        let e = anyhow::anyhow!("provider failure: missing api key");
        assert!(matches!(
            classify_error(e),
            AgentZeroError::ProviderError { .. }
        ));
    }

    #[test]
    fn classify_error_detects_config() {
        let e = anyhow::anyhow!("config file not found at /tmp/foo.toml");
        assert!(matches!(
            classify_error(e),
            AgentZeroError::ConfigError { .. }
        ));
    }

    #[test]
    fn classify_error_falls_back_to_runtime() {
        let e = anyhow::anyhow!("something unexpected happened");
        assert!(matches!(
            classify_error(e),
            AgentZeroError::RuntimeError { .. }
        ));
    }

    // ── FFI Tool Registration tests ──────────────────────────────────────

    struct EchoCallback;

    impl ToolCallback for EchoCallback {
        fn execute(&self, input: String, _workspace_root: String) -> Result<String, String> {
            Ok(format!("echo: {input}"))
        }

        fn description(&self) -> String {
            "Echoes the input back".to_string()
        }
    }

    struct FailCallback;

    impl ToolCallback for FailCallback {
        fn execute(&self, _input: String, _workspace_root: String) -> Result<String, String> {
            Err("intentional failure".to_string())
        }
    }

    #[test]
    fn register_tool_success() {
        let (config, _dir) = test_config();
        let controller = AgentZeroController::new(config);
        controller
            .register_tool("echo_tool".to_string(), Box::new(EchoCallback))
            .expect("registration should succeed");

        let names = controller.registered_tool_names();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0], "echo_tool");
    }

    #[test]
    fn register_multiple_tools() {
        let (config, _dir) = test_config();
        let controller = AgentZeroController::new(config);
        controller
            .register_tool("tool_a".to_string(), Box::new(EchoCallback))
            .unwrap();
        controller
            .register_tool("tool_b".to_string(), Box::new(EchoCallback))
            .unwrap();

        let names = controller.registered_tool_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"tool_a".to_string()));
        assert!(names.contains(&"tool_b".to_string()));
    }

    #[test]
    fn register_duplicate_name_fails() {
        let (config, _dir) = test_config();
        let controller = AgentZeroController::new(config);
        controller
            .register_tool("dup".to_string(), Box::new(EchoCallback))
            .unwrap();

        let err = controller
            .register_tool("dup".to_string(), Box::new(EchoCallback))
            .expect_err("duplicate should fail");
        assert!(
            matches!(err, AgentZeroError::RuntimeError { ref message } if message.contains("already registered")),
            "expected already registered error, got: {err}"
        );
    }

    #[test]
    fn ffi_tool_implements_tool_trait() {
        let tool = FfiTool::new("test_tool".to_string(), Arc::new(EchoCallback));
        assert_eq!(tool.name(), "test_tool");
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn ffi_tool_execute_success() {
        let tool = FfiTool::new("echo".to_string(), Arc::new(EchoCallback));
        let ctx = ToolContext::new("/tmp/test".to_string());
        let result = tool.execute("hello world", &ctx).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().output, "echo: hello world");
    }

    #[tokio::test]
    async fn ffi_tool_execute_error_propagation() {
        let tool = FfiTool::new("fail".to_string(), Arc::new(FailCallback));
        let ctx = ToolContext::new("/tmp/test".to_string());
        let result = tool.execute("anything", &ctx).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("intentional failure"),
            "error should contain callback message: {err}"
        );
    }

    #[test]
    fn registered_tools_starts_empty() {
        let (config, _dir) = test_config();
        let controller = AgentZeroController::new(config);
        assert!(controller.registered_tool_names().is_empty());
    }
}
