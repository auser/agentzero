pub mod agent;
pub mod common;
pub mod delegation;
pub mod metrics;
pub mod routing;
pub mod security;
pub mod types;
pub mod validation;

/// Re-export `tracing` so downstream crates can use `agentzero_core::tracing`
/// instead of adding a separate `tracing` dependency.
pub use tracing;

pub use agent::Agent;
pub use metrics::{HistogramSnapshot, RuntimeMetrics, RuntimeMetricsSnapshot};
pub use types::{
    AgentConfig, AgentError, AssistantMessage, AuditEvent, AuditSink, ChatResult,
    ConversationMessage, HookEvent, HookFailureMode, HookPolicy, HookRiskTier, HookSink,
    MemoryEntry, MemoryStore, MetricsSink, Provider, ReasoningConfig, ResearchPolicy,
    ResearchTrigger, StopReason, StreamChunk, Tool, ToolCallDelta, ToolContext, ToolDefinition,
    ToolResult, ToolResultMessage, ToolUseRequest, UserMessage,
};
