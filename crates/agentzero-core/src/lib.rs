//! Core traits, types, and utilities for AgentZero.
//!
//! Defines the fundamental abstractions: `Tool`, `Provider`, `Agent`,
//! `MemoryStore`, and all message/result types. Also contains shared
//! utilities for URL policy enforcement, security, delegation, and routing.

pub mod a2a_types;
pub mod agent;
pub mod agent_store;
pub mod canvas;
pub mod common;
pub mod complexity;
pub mod context_compression;
pub mod delegation;
pub mod device;
pub mod embedding;
pub mod event_bus;
pub mod loop_detection;
pub mod metrics;
#[cfg(feature = "privacy")]
pub mod privacy;
pub mod regression;
pub mod regression_bus;
pub mod routing;
pub mod search;
pub mod security;
pub mod tool_middleware;
pub mod types;
pub mod validation;

/// Re-export `tracing` so downstream crates can use `agentzero_core::tracing`
/// instead of adding a separate `tracing` dependency.
pub use tracing;

pub use agent::{Agent, ToolSource};
pub use canvas::{Canvas, CanvasFrame, CanvasStore, CanvasSummary};
pub use event_bus::{
    Event, EventBus, EventFilter, EventSubscriber, FileBackedBus, InMemoryBus, PublishResult,
    TypedSubscriber, TypedTopic,
};
pub use loop_detection::{LoopDetectionConfig, ToolLoopDetector};
pub use metrics::{HistogramSnapshot, RuntimeMetrics, RuntimeMetricsSnapshot};
pub use types::{
    AgentConfig, AgentEndpoint, AgentError, AgentId, AnnounceMessage, AssistantMessage,
    AuditDetail, AuditEvent, AuditSink, ChannelEndpoint, ChatResult, ContentPart,
    ConversationMessage, DepthPolicy, DepthRule, EphemeralMemory, HookEvent, HookFailureMode,
    HookPolicy, HookRiskTier, HookSink, JobStatus, Lane, LoopAction, MemoryEntry, MemoryStore,
    MergeStrategy, MetricsSink, Provider, QueueMode, ReasoningConfig, ResearchPolicy,
    ResearchTrigger, RunId, SessionId, StopReason, StreamChunk, StreamSink,
    StreamToolCallAccumulator, SummarizationConfig, Tool, ToolCallDelta, ToolContext,
    ToolDefinition, ToolExecutionRecord, ToolResult, ToolResultMessage, ToolSelectionMode,
    ToolSelector, ToolSummary, ToolUseRequest, UserMessage,
};
pub use types::{
    ConversationNode, ConversationTree, SkillActivation, SkillBundle, SkillBundleMeta, SkillLoader,
    SkillToolDef, SkillTrigger,
};
pub use validation::validate_json;
