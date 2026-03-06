//! Core traits, types, and utilities for AgentZero.
//!
//! Defines the fundamental abstractions: `Tool`, `Provider`, `Agent`,
//! `MemoryStore`, and all message/result types. Also contains shared
//! utilities for URL policy enforcement, security, delegation, and routing.

pub mod agent;
pub mod common;
pub mod delegation;
pub mod event_bus;
pub mod metrics;
#[cfg(feature = "privacy")]
pub mod privacy;
pub mod routing;
pub mod security;
pub mod types;
pub mod validation;

/// Re-export `tracing` so downstream crates can use `agentzero_core::tracing`
/// instead of adding a separate `tracing` dependency.
pub use tracing;

pub use agent::Agent;
pub use event_bus::{Event, EventBus, EventSubscriber, InMemoryBus};
pub use metrics::{HistogramSnapshot, RuntimeMetrics, RuntimeMetricsSnapshot};
pub use types::{
    AgentConfig, AgentError, AssistantMessage, AuditEvent, AuditSink, ChatResult,
    ConversationMessage, HookEvent, HookFailureMode, HookPolicy, HookRiskTier, HookSink,
    MemoryEntry, MemoryStore, MetricsSink, Provider, ReasoningConfig, ResearchPolicy,
    ResearchTrigger, StopReason, StreamChunk, StreamSink, Tool, ToolCallDelta, ToolContext,
    ToolDefinition, ToolResult, ToolResultMessage, ToolUseRequest, UserMessage,
};
pub use validation::validate_json;
