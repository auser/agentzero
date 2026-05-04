//! Session engine for AgentZero.
//!
//! Orchestrates model calls, tool invocation, and policy enforcement
//! within a single supervised session (ADR 0001).

pub mod ollama;
mod provider;
mod session;
mod tool_exec;

pub use ollama::{ChatMessage, OllamaConfig, OllamaProvider};
pub use provider::{LocalStubProvider, ModelLocation, ModelProvider, ModelProviderError};
pub use session::{Session, SessionConfig, SessionError, SessionMode};
pub use tool_exec::{ToolExecutor, ToolExecutorError, ToolResult};
