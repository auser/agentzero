//! Session engine for AgentZero.
//!
//! Orchestrates model calls, tool invocation, and policy enforcement
//! within a single supervised session (ADR 0001).

mod provider;
mod session;
mod tool_exec;

pub use provider::{LocalStubProvider, ModelProvider, ModelProviderError};
pub use session::{Session, SessionConfig, SessionError, SessionMode};
pub use tool_exec::{ToolExecutor, ToolExecutorError, ToolResult};
