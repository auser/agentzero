//! Session engine for AgentZero.
//!
//! Orchestrates model calls, tool invocation, and policy enforcement
//! within a single supervised session (ADR 0001).

pub mod ollama;
pub mod openai_compat;
mod provider;
mod session;
mod tool_exec;

pub use ollama::{
    ChatMessage, ChatResult, OllamaConfig, OllamaProvider, ToolCall, ToolCallFunction,
    ToolDefinition, ToolFunctionDef,
};
pub use openai_compat::{OpenAICompatConfig, OpenAICompatProvider};
pub use provider::{LocalStubProvider, ModelLocation, ModelProvider, ModelProviderError};
pub use session::{Session, SessionConfig, SessionError, SessionMode};
pub use tool_exec::{ToolExecutor, ToolExecutorError, ToolResult};
