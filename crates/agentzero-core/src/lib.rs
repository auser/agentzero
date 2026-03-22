//! Core traits, types, and utilities for AgentZero.
//!
//! Defines the fundamental abstractions: `Tool`, `Provider`, `Agent`,
//! `MemoryStore`, and all message/result types. Also contains shared
//! utilities for URL policy enforcement, security, delegation, and routing.

pub mod a2a_types;
pub mod agent;
pub mod agent_store;
pub mod common;
pub mod delegation;
pub mod embedding;
pub mod event_bus;
pub mod loop_detection;
pub mod metrics;
#[cfg(feature = "privacy")]
pub mod privacy;
pub mod regression;
pub mod regression_bus;
pub mod routing;
pub mod security;
pub mod types;
pub mod validation;

/// Re-export `tracing` so downstream crates can use `agentzero_core::tracing`
/// instead of adding a separate `tracing` dependency.
pub use tracing;

pub use agent::Agent;
pub use event_bus::{Event, EventBus, EventSubscriber, FileBackedBus, InMemoryBus};
pub use loop_detection::{LoopDetectionConfig, ToolLoopDetector};
pub use metrics::{HistogramSnapshot, RuntimeMetrics, RuntimeMetricsSnapshot};
pub use types::{
    AgentConfig, AgentEndpoint, AgentError, AnnounceMessage, AssistantMessage, AuditEvent,
    AuditSink, ChannelEndpoint, ChatResult, ContentPart, ConversationMessage, DepthPolicy,
    DepthRule, HookEvent, HookFailureMode, HookPolicy, HookRiskTier, HookSink, JobStatus, Lane,
    LoopAction, MemoryEntry, MemoryStore, MergeStrategy, MetricsSink, Provider, QueueMode,
    ReasoningConfig, ResearchPolicy, ResearchTrigger, RunId, StopReason, StreamChunk, StreamSink,
    StreamToolCallAccumulator, SummarizationConfig, Tool, ToolCallDelta, ToolContext,
    ToolDefinition, ToolResult, ToolResultMessage, ToolSelectionMode, ToolSelector, ToolSummary,
    ToolUseRequest, UserMessage,
};
pub use validation::validate_json;
