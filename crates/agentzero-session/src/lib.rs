//! Session engine for AgentZero.
//!
//! Orchestrates model calls, tool invocation, and policy enforcement
//! within a single supervised session (ADR 0001).

pub mod agent_loop;
pub mod anthropic;
pub mod checkpoint;
pub mod context;
pub mod dynamic_tools;
pub mod models_config;
pub mod ollama;
pub mod openai_compat;
mod provider;
pub mod retry;
pub mod router;
mod session;
mod tool_exec;
pub mod wasm_host;

pub use agent_loop::{
    AgentLoop, AgentLoopConfig, AgentLoopError, AgentResponse, ApprovalDecision, ApprovalHandler,
    AutoApprove, NoopProgress, ProgressHandler, ToolCallRecord,
};
pub use checkpoint::{
    AgentCheckpoint, CaptureParams, CheckpointConfig, CheckpointError, WakeTrigger,
};
pub use models_config::{ModelsConfig, ProviderConfig, ProviderType};
pub use ollama::{
    ChatMessage, ChatResult, OllamaConfig, OllamaProvider, ToolCall, ToolCallFunction,
    ToolDefinition, ToolFunctionDef,
};
pub use openai_compat::{OpenAICompatConfig, OpenAICompatProvider};
pub use provider::{LocalStubProvider, ModelLocation, ModelProvider, ModelProviderError};
pub use session::{Session, SessionConfig, SessionError, SessionMode};
pub use tool_exec::{ToolExecutor, ToolExecutorError, ToolResult};
