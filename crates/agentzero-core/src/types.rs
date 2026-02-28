use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub max_tool_iterations: usize,
    pub request_timeout_ms: u64,
    pub memory_window_size: usize,
    pub max_prompt_chars: usize,
    pub hooks: HookPolicy,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_tool_iterations: 4,
            request_timeout_ms: 30_000,
            memory_window_size: 8,
            max_prompt_chars: 8_000,
            hooks: HookPolicy::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HookPolicy {
    pub enabled: bool,
    pub timeout_ms: u64,
    pub fail_closed: bool,
}

impl Default for HookPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_ms: 250,
            fail_closed: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResult {
    pub output_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    pub workspace_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub stage: String,
    pub detail: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookEvent {
    pub stage: String,
    pub detail: Value,
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent request timed out after {timeout_ms} ms")]
    Timeout { timeout_ms: u64 },
    #[error("provider failure: {source}")]
    Provider {
        #[source]
        source: anyhow::Error,
    },
    #[error("memory failure: {source}")]
    Memory {
        #[source]
        source: anyhow::Error,
    },
    #[error("tool failure ({tool}): {source}")]
    Tool {
        tool: String,
        #[source]
        source: anyhow::Error,
    },
    #[error("hook failure ({stage}): {source}")]
    Hook {
        stage: String,
        #[source]
        source: anyhow::Error,
    },
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult>;
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()>;
    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>>;
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult>;
}

#[async_trait]
pub trait AuditSink: Send + Sync {
    async fn record(&self, event: AuditEvent) -> anyhow::Result<()>;
}

#[async_trait]
pub trait HookSink: Send + Sync {
    async fn record(&self, event: HookEvent) -> anyhow::Result<()>;
}

pub trait MetricsSink: Send + Sync {
    fn increment_counter(&self, name: &'static str, value: u64);
    fn observe_histogram(&self, _name: &'static str, _value: f64) {}
}
