//! napi-rs bindings for TypeScript / Node.js.
//!
//! This module is only compiled when the `node` feature is enabled.
//! It wraps the core [`AgentZeroController`] in napi-rs types that produce
//! a native Node.js addon with auto-generated TypeScript definitions.

use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::{
    AgentResponse as CoreResponse, AgentStatus as CoreStatus, AgentZeroConfig as CoreConfig,
    AgentZeroController as CoreController, AgentZeroError as CoreError, ChatMessage as CoreMessage,
    FfiTool, ToolCallback,
};

#[napi(object)]
pub struct NodeAgentZeroConfig {
    pub config_path: String,
    pub workspace_root: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub profile: Option<String>,
}

impl From<NodeAgentZeroConfig> for CoreConfig {
    fn from(c: NodeAgentZeroConfig) -> Self {
        CoreConfig {
            config_path: c.config_path,
            workspace_root: c.workspace_root,
            provider: c.provider,
            model: c.model,
            profile: c.profile,
        }
    }
}

impl From<CoreConfig> for NodeAgentZeroConfig {
    fn from(c: CoreConfig) -> Self {
        Self {
            config_path: c.config_path,
            workspace_root: c.workspace_root,
            provider: c.provider,
            model: c.model,
            profile: c.profile,
        }
    }
}

#[napi(object)]
pub struct NodeAgentResponse {
    pub text: String,
    pub metrics_json: String,
}

impl From<CoreResponse> for NodeAgentResponse {
    fn from(r: CoreResponse) -> Self {
        Self {
            text: r.text,
            metrics_json: r.metrics_json,
        }
    }
}

#[napi(object)]
pub struct NodeChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp_ms: i64,
}

impl From<CoreMessage> for NodeChatMessage {
    fn from(m: CoreMessage) -> Self {
        Self {
            role: m.role,
            content: m.content,
            timestamp_ms: m.timestamp_ms,
        }
    }
}

fn core_error_to_napi(e: CoreError) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}

#[napi]
pub struct AgentZeroController {
    inner: Arc<CoreController>,
}

#[napi]
impl AgentZeroController {
    #[napi(constructor)]
    pub fn new(config: NodeAgentZeroConfig) -> Self {
        Self {
            inner: CoreController::new(config.into()),
        }
    }

    #[napi(factory)]
    pub fn with_defaults(config_path: String, workspace_root: String) -> Self {
        Self {
            inner: CoreController::with_defaults(config_path, workspace_root),
        }
    }

    #[napi]
    pub fn status(&self) -> String {
        match self.inner.status() {
            CoreStatus::Idle => "idle".to_string(),
            CoreStatus::Running => "running".to_string(),
            CoreStatus::Error { message } => format!("error: {message}"),
        }
    }

    #[napi]
    pub fn send_message(&self, message: String) -> napi::Result<NodeAgentResponse> {
        self.inner
            .send_message(message)
            .map(NodeAgentResponse::from)
            .map_err(core_error_to_napi)
    }

    #[napi]
    pub fn get_history(&self) -> Vec<NodeChatMessage> {
        self.inner
            .get_history()
            .into_iter()
            .map(NodeChatMessage::from)
            .collect()
    }

    #[napi]
    pub fn clear_history(&self) {
        self.inner.clear_history();
    }

    #[napi]
    pub fn version(&self) -> String {
        self.inner.version()
    }

    #[napi]
    pub fn get_config(&self) -> napi::Result<NodeAgentZeroConfig> {
        self.inner
            .get_config()
            .map(NodeAgentZeroConfig::from)
            .map_err(core_error_to_napi)
    }

    #[napi]
    pub fn update_config(&self, config: NodeAgentZeroConfig) -> napi::Result<()> {
        self.inner
            .update_config(config.into())
            .map_err(core_error_to_napi)
    }

    #[napi]
    pub fn register_tool(&self, name: String, description: String) -> napi::Result<()> {
        let callback: Box<dyn ToolCallback> = Box::new(NodeStubCallback { desc: description });
        self.inner
            .register_tool_impl(name, Arc::from(callback))
            .map_err(core_error_to_napi)
    }

    #[napi]
    pub fn registered_tool_names(&self) -> Vec<String> {
        self.inner.registered_tool_names()
    }

    /// Non-blocking version of `send_message` that returns a Promise.
    /// Avoids blocking the Node.js event loop.
    #[napi]
    pub async fn send_message_async(&self, message: String) -> napi::Result<NodeAgentResponse> {
        let inner = Arc::clone(&self.inner);
        // Run the blocking send_message on a separate thread so the event loop stays free.
        let result = tokio::task::spawn_blocking(move || inner.send_message(message))
            .await
            .map_err(|e| napi::Error::from_reason(format!("task panicked: {e}")))?;
        result
            .map(NodeAgentResponse::from)
            .map_err(core_error_to_napi)
    }
}

/// Stub callback for tools registered from Node.js via `register_tool`.
///
/// Provides the tool name and description to the agent's tool list.
/// Full JS callback execution requires `send_message_async` to avoid
/// deadlocking the Node.js event loop.
struct NodeStubCallback {
    desc: String,
}

impl ToolCallback for NodeStubCallback {
    fn execute(&self, _input: String, _workspace_root: String) -> Result<String, String> {
        Err(
            "Node.js tool callback not yet connected — register a full callback via the JS SDK"
                .to_string(),
        )
    }

    fn description(&self) -> String {
        self.desc.clone()
    }
}
