pub mod agent;
pub mod common;
pub mod delegation;
pub mod metrics;
pub mod routing;
pub mod security;
pub mod types;

/// Re-export `tracing` so downstream crates can use `agentzero_core::tracing`
/// instead of adding a separate `tracing` dependency.
pub use tracing;

pub use agent::Agent;
pub use metrics::{HistogramSnapshot, RuntimeMetrics, RuntimeMetricsSnapshot};
pub use types::{
    AgentConfig, AgentError, AssistantMessage, AuditEvent, AuditSink, ChatResult, HookEvent,
    HookFailureMode, HookPolicy, HookRiskTier, HookSink, MemoryEntry, MemoryStore, MetricsSink,
    Provider, ReasoningConfig, ResearchPolicy, ResearchTrigger, StreamChunk, Tool, ToolContext,
    ToolResult, UserMessage,
};
