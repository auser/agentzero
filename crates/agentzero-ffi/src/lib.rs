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

// ── Controller ─────────────────────────────────────────────────────────────

/// Main AgentZero controller exposed to foreign languages.
#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
pub struct AgentZeroController {
    config: Mutex<AgentZeroConfig>,
    status: Mutex<AgentStatus>,
    history: Mutex<Vec<ChatMessage>>,
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

        let req = agentzero_runtime::RunAgentRequest {
            workspace_root: config.workspace_root.into(),
            config_path: config.config_path.into(),
            message: message.clone(),
            provider_override: config.provider.clone(),
            model_override: config.model.clone(),
            profile_override: config.profile.clone(),
        };

        let result = runtime().block_on(agentzero_runtime::run_agent_once(req));

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

    fn test_config() -> AgentZeroConfig {
        AgentZeroConfig {
            config_path: "/tmp/agentzero-ffi-test/agentzero.toml".to_string(),
            workspace_root: "/tmp/agentzero-ffi-test".to_string(),
            provider: None,
            model: None,
            profile: None,
        }
    }

    #[test]
    fn controller_creation_success_path() {
        let controller = AgentZeroController::new(test_config());
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
        let controller = AgentZeroController::new(test_config());
        let version = controller.version();
        assert!(!version.is_empty());
        assert!(version.contains('.'));
    }

    #[test]
    fn history_starts_empty() {
        let controller = AgentZeroController::new(test_config());
        assert!(controller.get_history().is_empty());
    }

    #[test]
    fn clear_history_empties_messages() {
        let controller = AgentZeroController::new(test_config());
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
        let controller = AgentZeroController::new(test_config());
        let mut cfg = controller.get_config().unwrap();
        assert!(cfg.provider.is_none());

        cfg.provider = Some("anthropic".to_string());
        controller.update_config(cfg).unwrap();

        let updated = controller.get_config().unwrap();
        assert_eq!(updated.provider, Some("anthropic".to_string()));
    }

    #[test]
    fn send_message_returns_error_for_invalid_setup() {
        let controller = AgentZeroController::new(test_config());
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
            ),
            "expected config, runtime, or provider error, got: {err}"
        );
    }

    #[test]
    fn status_transitions_through_send_message() {
        let controller = AgentZeroController::new(test_config());
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
}
