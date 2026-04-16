use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;

use crate::event_bus::EventBus;

// ---------------------------------------------------------------------------
// Multi-agent orchestration types
// ---------------------------------------------------------------------------

/// Unique identifier for an async job / agent run.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunId(pub String);

impl RunId {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(format!("run-{ts:x}-{seq}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Unique identifier for a session (groups related runs/events for replay).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(format!("ses-{ts}-{seq}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Unique identifier for an agent in the swarm.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub String);

impl AgentId {
    pub fn new(name: &str) -> Self {
        Self(name.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for AgentId {
    fn default() -> Self {
        Self("default".to_string())
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Status of an async job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Completed { result: String },
    Failed { error: String },
    Cancelled,
}

impl JobStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobStatus::Completed { .. } | JobStatus::Failed { .. } | JobStatus::Cancelled
        )
    }
}

/// Processing lane for work items. Separate lanes prevent session collisions
/// and allow independent concurrency control.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "lane", rename_all = "snake_case")]
#[derive(Default)]
pub enum Lane {
    /// Interactive user requests (serialized, one at a time).
    #[default]
    Main,
    /// Scheduled/cron jobs (parallel up to configured limit).
    Cron,
    /// Sub-agent work spawned by a parent agent.
    SubAgent { parent_run_id: RunId, depth: u8 },
}

/// How a message should be routed within a lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
#[derive(Default)]
pub enum QueueMode {
    /// Route message to a single agent based on AI router classification (default).
    #[default]
    Steer,
    /// Append to an existing run's conversation rather than starting a new one.
    Followup { run_id: RunId },
    /// Fan-out to all agents in the lane, collect all responses, merge into a single result.
    Collect,
    /// Preempt the currently running agent in the lane, cancelling its in-flight work.
    Interrupt,
}

/// Action returned by a tool-loop detector after inspecting a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopAction {
    /// No loop detected — continue normally.
    Continue,
    /// Inject a system message telling the agent to try a different approach.
    InjectMessage(String),
    /// Remove the named tools for the next iteration.
    RestrictTools(Vec<String>),
    /// Force-complete the run with the given error message.
    ForceComplete(String),
}

/// Message published when a sub-agent completes, announcing its result
/// back to the parent agent's channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnounceMessage {
    pub run_id: RunId,
    pub agent_id: String,
    pub parent_run_id: Option<RunId>,
    pub summary: String,
    pub status: JobStatus,
    pub depth: u8,
}

/// Rule controlling which tools are available at a given nesting depth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepthRule {
    /// Maximum depth this rule applies to (inclusive).
    pub max_depth: u8,
    /// If non-empty, ONLY these tools are available (allowlist).
    #[serde(default)]
    pub allowed_tools: HashSet<String>,
    /// These tools are removed regardless of the allowlist (denylist).
    #[serde(default)]
    pub denied_tools: HashSet<String>,
}

/// Policy controlling tool availability based on sub-agent nesting depth.
/// Rules are evaluated in order; the first rule where `depth <= max_depth` applies.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DepthPolicy {
    pub rules: Vec<DepthRule>,
}

impl DepthPolicy {
    /// Filter a list of tool names based on the current depth.
    /// Returns the names that should remain available.
    pub fn filter_tools(&self, depth: u8, tool_names: &[&str]) -> Vec<String> {
        let rule = self.rules.iter().find(|r| depth <= r.max_depth);
        match rule {
            None => tool_names.iter().map(|s| s.to_string()).collect(),
            Some(rule) => {
                let mut result: Vec<String> = if rule.allowed_tools.is_empty() {
                    // No allowlist → start with all tools.
                    tool_names.iter().map(|s| s.to_string()).collect()
                } else {
                    // Allowlist mode → only include listed tools.
                    tool_names
                        .iter()
                        .filter(|name| rule.allowed_tools.contains(**name))
                        .map(|s| s.to_string())
                        .collect()
                };
                // Always apply denylist.
                result.retain(|name| !rule.denied_tools.contains(name));
                result
            }
        }
    }
}

/// Merge strategy for fan-out steps where multiple agents run in parallel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum MergeStrategy {
    /// Wait for all agents to complete before proceeding.
    #[default]
    WaitAll,
    /// Proceed as soon as any one agent completes.
    WaitAny,
    /// Proceed once at least `min` agents have completed.
    WaitQuorum { min: usize },
}

#[derive(Clone)]
pub struct AgentConfig {
    pub max_tool_iterations: usize,
    pub request_timeout_ms: u64,
    pub memory_window_size: usize,
    pub max_prompt_chars: usize,
    pub hooks: HookPolicy,
    pub parallel_tools: bool,
    /// Tools that require approval before execution (from autonomy.always_ask).
    /// When parallel_tools is enabled, any batch containing a gated tool falls
    /// back to sequential execution to preserve the approval flow.
    pub gated_tools: HashSet<String>,
    pub loop_detection_no_progress_threshold: usize,
    pub loop_detection_ping_pong_cycles: usize,
    pub loop_detection_failure_streak: usize,
    pub research: ResearchPolicy,
    pub reasoning: ReasoningConfig,
    /// Whether the current model supports tool use (function calling).
    pub model_supports_tool_use: bool,
    /// Whether the current model supports vision (image content blocks).
    pub model_supports_vision: bool,
    /// Optional system prompt sent to the LLM at the start of each conversation.
    pub system_prompt: Option<String>,
    /// Effective privacy boundary for this agent: "local_only", "encrypted_only", "any", or empty (inherit).
    pub privacy_boundary: String,
    /// Per-tool privacy boundaries. Keys are tool names, values are boundary
    /// strings that override the agent-level boundary for that tool.
    pub tool_boundaries: HashMap<String, String>,
    /// Optional function to compute cost in microdollars from (input_tokens, output_tokens).
    /// Injected at construction time so the core crate doesn't depend on providers.
    pub cost_calculator: Option<Arc<dyn Fn(u64, u64) -> u64 + Send + Sync>>,
    /// Per-tool execution timeout in milliseconds (0 = no timeout). Default: 120_000 (2 min).
    pub tool_timeout_ms: u64,
    /// Tool selection strategy: "all" (default), "keyword", or "ai".
    pub tool_selection: ToolSelectionMode,
    /// Optional override model for AI-based tool selection (cheaper/faster model).
    pub tool_selection_model: Option<String>,
    /// Context summarization config. When enabled, older conversation entries
    /// are summarized by the LLM instead of being hard-truncated.
    pub summarization: SummarizationConfig,
    /// Prompt fragments from active skills, injected after the system prompt.
    /// Populated at runtime by the skill loader; empty by default.
    pub skill_prompt_fragments: Vec<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_tool_iterations: 20,
            request_timeout_ms: 30_000,
            memory_window_size: 50,
            max_prompt_chars: 8_000,
            hooks: HookPolicy::default(),
            parallel_tools: false,
            gated_tools: HashSet::new(),
            loop_detection_no_progress_threshold: 3,
            loop_detection_ping_pong_cycles: 2,
            loop_detection_failure_streak: 3,
            research: ResearchPolicy::default(),
            reasoning: ReasoningConfig::default(),
            model_supports_tool_use: true,
            model_supports_vision: false,
            system_prompt: None,
            privacy_boundary: String::new(),
            tool_boundaries: HashMap::new(),
            cost_calculator: None,
            tool_timeout_ms: 120_000,
            tool_selection: ToolSelectionMode::All,
            tool_selection_model: None,
            summarization: SummarizationConfig::default(),
            skill_prompt_fragments: Vec::new(),
        }
    }
}

impl std::fmt::Debug for AgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentConfig")
            .field("max_tool_iterations", &self.max_tool_iterations)
            .field("request_timeout_ms", &self.request_timeout_ms)
            .field("memory_window_size", &self.memory_window_size)
            .field("max_prompt_chars", &self.max_prompt_chars)
            .field("parallel_tools", &self.parallel_tools)
            .field("model_supports_tool_use", &self.model_supports_tool_use)
            .field("model_supports_vision", &self.model_supports_vision)
            .field("system_prompt", &self.system_prompt.is_some())
            .field("privacy_boundary", &self.privacy_boundary)
            .field(
                "cost_calculator",
                &self.cost_calculator.as_ref().map(|_| "..."),
            )
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct HookPolicy {
    pub enabled: bool,
    pub timeout_ms: u64,
    pub fail_closed: bool,
    pub default_mode: HookFailureMode,
    pub low_tier_mode: HookFailureMode,
    pub medium_tier_mode: HookFailureMode,
    pub high_tier_mode: HookFailureMode,
}

impl Default for HookPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_ms: 250,
            fail_closed: false,
            default_mode: HookFailureMode::Warn,
            low_tier_mode: HookFailureMode::Ignore,
            medium_tier_mode: HookFailureMode::Warn,
            high_tier_mode: HookFailureMode::Block,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookFailureMode {
    Block,
    Warn,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookRiskTier {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone)]
pub struct ResearchPolicy {
    pub enabled: bool,
    pub trigger: ResearchTrigger,
    pub keywords: Vec<String>,
    pub min_message_length: usize,
    pub max_iterations: usize,
    pub show_progress: bool,
}

impl Default for ResearchPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            trigger: ResearchTrigger::Never,
            keywords: Vec::new(),
            min_message_length: 50,
            max_iterations: 5,
            show_progress: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResearchTrigger {
    Never,
    Always,
    Keywords,
    Length,
    Question,
}

#[derive(Debug, Clone, Default)]
pub struct ReasoningConfig {
    pub enabled: Option<bool>,
    pub level: Option<String>,
    /// When true, reasoning effort is adjusted dynamically based on query
    /// complexity. Simple queries get low/no reasoning; complex queries get deep.
    pub adaptive: bool,
}

impl ReasoningConfig {
    /// Adjust reasoning level based on query complexity.
    /// Returns a new config with the level set to match the complexity tier.
    pub fn adapt_to_complexity(&self, tier: crate::complexity::ComplexityTier) -> ReasoningConfig {
        if !self.adaptive {
            return self.clone();
        }
        let (enabled, level) = match tier {
            crate::complexity::ComplexityTier::Simple => (Some(false), None),
            crate::complexity::ComplexityTier::Medium => (Some(true), Some("medium".to_string())),
            crate::complexity::ComplexityTier::Complex => (Some(true), Some("high".to_string())),
        };
        ReasoningConfig {
            enabled,
            level,
            adaptive: self.adaptive,
        }
    }
}

/// Configuration for intelligent context summarization.
/// When enabled, older conversation entries are summarized by the LLM
/// instead of being hard-truncated, preserving key context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummarizationConfig {
    pub enabled: bool,
    /// Number of recent entries to keep verbatim (rest get summarized).
    pub keep_recent: usize,
    /// Minimum total entries before summarization triggers.
    pub min_entries_for_summarization: usize,
    /// Max characters for the generated summary.
    pub max_summary_chars: usize,

    // ── Advanced context compression (4-phase pipeline) ─────────────
    /// Enable 4-phase context compression on the tool-use message list.
    /// When enabled, tool results are pruned, boundaries are protected,
    /// and the middle section is summarized instead of hard-truncated.
    #[serde(default)]
    pub compression_enabled: bool,
    /// Maximum characters for a single tool result before truncation (Phase 1).
    #[serde(default = "default_max_tool_result_chars")]
    pub max_tool_result_chars: usize,
    /// Number of messages to protect at the start of conversation (Phase 2).
    #[serde(default = "default_protect_head")]
    pub protect_head: usize,
    /// Number of messages to protect at the tail of conversation (Phase 2).
    #[serde(default = "default_protect_tail")]
    pub protect_tail: usize,
}

fn default_max_tool_result_chars() -> usize {
    4000
}
fn default_protect_head() -> usize {
    3
}
fn default_protect_tail() -> usize {
    10
}

impl Default for SummarizationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            keep_recent: 10,
            min_entries_for_summarization: 20,
            max_summary_chars: 2000,
            compression_enabled: false,
            max_tool_result_chars: 4000,
            protect_head: 3,
            protect_tail: 10,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatResult {
    pub output_text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolUseRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    /// Number of input tokens consumed by this provider call (0 if unknown).
    #[serde(default)]
    pub input_tokens: u64,
    /// Number of output tokens produced by this provider call (0 if unknown).
    #[serde(default)]
    pub output_tokens: u64,
}

/// Incremental tool call data emitted during streaming tool use.
#[derive(Debug, Clone)]
pub struct ToolCallDelta {
    /// Index of the tool call in the response (for multi-call streaming).
    pub index: usize,
    /// Tool call ID (sent on first chunk for this index).
    pub id: Option<String>,
    /// Tool name (sent on first chunk for this index).
    pub name: Option<String>,
    /// Incremental JSON arguments string.
    pub arguments_delta: String,
}

/// A single chunk emitted during streaming completion.
#[derive(Debug, Clone)]
pub struct StreamChunk {
    /// Incremental text delta for this chunk.
    pub delta: String,
    /// True when the stream is complete (final chunk).
    pub done: bool,
    /// Incremental tool call data (for streaming tool use).
    pub tool_call_delta: Option<ToolCallDelta>,
}

/// Convenience alias for the sender half of a streaming channel.
pub type StreamSink = tokio::sync::mpsc::UnboundedSender<StreamChunk>;

/// Accumulates incremental tool call deltas from streaming responses into
/// complete `ToolUseRequest` objects. Shared across provider implementations
/// to avoid duplicating accumulation logic.
#[derive(Debug, Default)]
pub struct StreamToolCallAccumulator {
    /// In-flight tool calls: `(index, id, name, arguments_json_buffer)`.
    pending: Vec<(usize, String, String, String)>,
}

impl StreamToolCallAccumulator {
    /// Create a new empty accumulator.
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    /// Process a streaming tool call delta. Call this for each
    /// `ToolCallDelta` received during streaming.
    pub fn process_delta(&mut self, delta: &ToolCallDelta) {
        // Find or create the accumulator entry for this index.
        if let Some(entry) = self
            .pending
            .iter_mut()
            .find(|(idx, _, _, _)| *idx == delta.index)
        {
            // Update id/name if provided (first chunk for this index).
            if let Some(ref id) = delta.id {
                entry.1 = id.clone();
            }
            if let Some(ref name) = delta.name {
                entry.2 = name.clone();
            }
            entry.3.push_str(&delta.arguments_delta);
        } else {
            self.pending.push((
                delta.index,
                delta.id.clone().unwrap_or_default(),
                delta.name.clone().unwrap_or_default(),
                delta.arguments_delta.clone(),
            ));
        }
    }

    /// Consume the accumulator and produce finished tool use requests.
    pub fn finish(mut self) -> Vec<ToolUseRequest> {
        self.pending.sort_by_key(|(idx, _, _, _)| *idx);
        self.pending
            .into_iter()
            .map(|(_, id, name, args)| {
                let input = serde_json::from_str(&args)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                ToolUseRequest { id, name, input }
            })
            .collect()
    }

    /// Whether any tool calls have been accumulated.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolContext {
    pub workspace_root: String,
    #[serde(default)]
    pub allow_sensitive_file_reads: bool,
    #[serde(default)]
    pub allow_sensitive_file_writes: bool,
    /// Effective privacy boundary for this execution context.
    /// Empty string or "inherit" means no restriction.
    #[serde(default)]
    pub privacy_boundary: String,
    /// Source channel that initiated this request (for channel-specific boundaries).
    #[serde(default)]
    pub source_channel: Option<String>,
    /// Event bus for inter-agent communication. Available when swarm mode is active.
    #[serde(skip)]
    pub event_bus: Option<Arc<dyn EventBus>>,
    /// Identifier for the agent executing this tool (set in swarm mode).
    #[serde(skip)]
    pub agent_id: Option<String>,
    /// Active conversation identifier. When set, memory queries are scoped to this conversation.
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// Current nesting depth (0 = top-level agent, 1 = first sub-agent, etc.).
    #[serde(default)]
    pub depth: u8,
    /// Run identifier for this execution (for async job tracking).
    #[serde(default)]
    pub run_id: Option<RunId>,
    /// Parent run that spawned this sub-agent (for announce-back pattern).
    #[serde(default)]
    pub parent_run_id: Option<RunId>,
    /// Processing lane this execution belongs to.
    #[serde(default)]
    pub lane: Option<Lane>,
    /// Cancellation flag. When set to `true`, the agent should stop executing
    /// tool calls and return as soon as possible.
    #[serde(skip)]
    pub cancelled: Arc<std::sync::atomic::AtomicBool>,
    /// Accumulated token usage for this execution (input + output tokens).
    /// Shared via `Arc` so child delegations can aggregate back to the parent.
    #[serde(skip)]
    pub tokens_used: Arc<std::sync::atomic::AtomicU64>,
    /// Accumulated cost in micro-dollars for this execution.
    /// Shared via `Arc` so child delegations can aggregate back to the parent.
    #[serde(skip)]
    pub cost_microdollars: Arc<std::sync::atomic::AtomicU64>,
    /// Maximum token budget for this execution (0 = unlimited).
    #[serde(default)]
    pub max_tokens: u64,
    /// Maximum cost budget in micro-dollars (0 = unlimited).
    #[serde(default)]
    pub max_cost_microdollars: u64,
    /// Path to the agentzero.toml config file (for self-configuration tools).
    #[serde(default)]
    pub config_path: Option<String>,
    /// Sender identity for per-sender rate limiting (e.g., Telegram user ID, Discord channel).
    #[serde(default)]
    pub sender_id: Option<String>,
    /// Cancellation token for structured cancellation cascade.
    /// Coexists with the legacy `cancelled: Arc<AtomicBool>` flag.
    /// When the token is cancelled, background tasks and sub-agents should stop.
    #[serde(skip)]
    pub cancellation_token: Option<tokio_util::sync::CancellationToken>,
    /// Task identifier when this context is executing as a background task.
    /// Set by `TaskManager::spawn_background()` so tools can identify their own task.
    #[serde(default)]
    pub task_id: Option<String>,
    /// Shared collector for tool execution records. Populated during agent runs
    /// and consumed by the runtime for quality tracking and persistence.
    #[serde(skip)]
    pub tool_executions: Arc<std::sync::Mutex<Vec<ToolExecutionRecord>>>,
    /// Effective capability set for this execution (Sprint 90 — Phase J/K).
    ///
    /// When non-empty, memory tools enforce namespace access and delegate tools
    /// apply `Delegate { max_capabilities }` ceilings.
    /// Set by the runtime from `tool_policy.capability_set`.
    /// Empty (default) = unrestricted / backward-compatible.
    #[serde(skip, default)]
    pub capability_set: crate::security::CapabilitySet,
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("workspace_root", &self.workspace_root)
            .field(
                "allow_sensitive_file_reads",
                &self.allow_sensitive_file_reads,
            )
            .field(
                "allow_sensitive_file_writes",
                &self.allow_sensitive_file_writes,
            )
            .field("privacy_boundary", &self.privacy_boundary)
            .field("source_channel", &self.source_channel)
            .field("event_bus", &self.event_bus.as_ref().map(|_| "..."))
            .field("agent_id", &self.agent_id)
            .field("conversation_id", &self.conversation_id)
            .field("depth", &self.depth)
            .field("run_id", &self.run_id)
            .field("parent_run_id", &self.parent_run_id)
            .field("lane", &self.lane)
            .field(
                "cancelled",
                &self.cancelled.load(std::sync::atomic::Ordering::Relaxed),
            )
            .field(
                "tokens_used",
                &self.tokens_used.load(std::sync::atomic::Ordering::Relaxed),
            )
            .field(
                "cost_microdollars",
                &self
                    .cost_microdollars
                    .load(std::sync::atomic::Ordering::Relaxed),
            )
            .field("max_tokens", &self.max_tokens)
            .field("max_cost_microdollars", &self.max_cost_microdollars)
            .field("sender_id", &self.sender_id)
            .field(
                "tool_executions_count",
                &self.tool_executions.lock().map(|v| v.len()).unwrap_or(0),
            )
            .finish()
    }
}

impl ToolContext {
    pub fn new(workspace_root: String) -> Self {
        Self {
            workspace_root,
            allow_sensitive_file_reads: false,
            allow_sensitive_file_writes: false,
            privacy_boundary: String::new(),
            source_channel: None,
            event_bus: None,
            agent_id: None,
            conversation_id: None,
            config_path: None,
            depth: 0,
            run_id: None,
            parent_run_id: None,
            lane: None,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tokens_used: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            cost_microdollars: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            max_tokens: 0,
            max_cost_microdollars: 0,
            sender_id: None,
            cancellation_token: None,
            task_id: None,
            tool_executions: Arc::new(std::sync::Mutex::new(Vec::new())),
            capability_set: crate::security::CapabilitySet::default(),
        }
    }

    /// Create a default context for the given workspace root (for tests and tools).
    pub fn default_for_workspace(workspace_root: &str) -> Self {
        Self::new(workspace_root.to_string())
    }

    /// Check if this execution has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Add tokens to the running total. Returns the new total.
    pub fn add_tokens(&self, tokens: u64) -> u64 {
        self.tokens_used
            .fetch_add(tokens, std::sync::atomic::Ordering::Relaxed)
            + tokens
    }

    /// Add cost (in micro-dollars) to the running total. Returns the new total.
    pub fn add_cost(&self, microdollars: u64) -> u64 {
        self.cost_microdollars
            .fetch_add(microdollars, std::sync::atomic::Ordering::Relaxed)
            + microdollars
    }

    /// Current accumulated token usage.
    pub fn current_tokens(&self) -> u64 {
        self.tokens_used.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Current accumulated cost in micro-dollars.
    pub fn current_cost(&self) -> u64 {
        self.cost_microdollars
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Check if the token or cost budget has been exceeded.
    /// Returns `Some(reason)` if a budget limit has been exceeded, `None` otherwise.
    pub fn budget_exceeded(&self) -> Option<String> {
        if self.max_tokens > 0 && self.current_tokens() > self.max_tokens {
            Some(format!(
                "token budget exceeded: {} > {}",
                self.current_tokens(),
                self.max_tokens,
            ))
        } else if self.max_cost_microdollars > 0 && self.current_cost() > self.max_cost_microdollars
        {
            Some(format!(
                "cost budget exceeded: {} > {} microdollars",
                self.current_cost(),
                self.max_cost_microdollars,
            ))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: String,
}

/// A tool definition sent to the LLM for native tool use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

impl ToolDefinition {
    /// Build from a Tool trait object, returning `None` if the tool has no schema.
    pub fn from_tool(tool: &dyn Tool) -> Option<Self> {
        let schema = tool.input_schema()?;
        Some(Self {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            input_schema: schema,
        })
    }
}

// ---------------------------------------------------------------------------
// Tool selection
// ---------------------------------------------------------------------------

/// Strategy for selecting which tools to pass to the LLM provider.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolSelectionMode {
    /// Pass all available tools (default, backward-compatible).
    #[default]
    All,
    /// Use keyword/TF-IDF matching on tool descriptions. No LLM call.
    Keyword,
    /// Use a lightweight LLM call to classify relevant tools.
    Ai,
    /// Two-stage: keyword/embedding pre-filter → LLM refinement on shortlist.
    TwoStage,
}

impl std::fmt::Display for ToolSelectionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => write!(f, "all"),
            Self::Keyword => write!(f, "keyword"),
            Self::Ai => write!(f, "ai"),
            Self::TwoStage => write!(f, "two_stage"),
        }
    }
}

impl std::str::FromStr for ToolSelectionMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "all" => Ok(Self::All),
            "keyword" => Ok(Self::Keyword),
            "ai" => Ok(Self::Ai),
            "two_stage" | "twostage" => Ok(Self::TwoStage),
            other => Err(format!("unknown tool selection mode: {other}")),
        }
    }
}

/// Lightweight summary of a tool for selection purposes (name + description only).
#[derive(Debug, Clone)]
pub struct ToolSummary {
    pub name: String,
    pub description: String,
}

/// Selects a subset of tools relevant to a given task.
#[async_trait]
pub trait ToolSelector: Send + Sync {
    /// Given a task description and available tools, return the names of relevant tools.
    async fn select(
        &self,
        task_description: &str,
        available_tools: &[ToolSummary],
    ) -> anyhow::Result<Vec<String>>;
}

/// The LLM's request to invoke a tool (from a tool_use response).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseRequest {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// A tool result message sent back to the LLM after execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultMessage {
    pub tool_use_id: String,
    pub content: String,
    #[serde(default)]
    pub is_error: bool,
}

/// A content part within a multi-modal user message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        media_type: String,
        data: String, // base64-encoded
    },
    #[serde(rename = "audio")]
    Audio {
        media_type: String,
        data: String, // base64-encoded
    },
}

/// A message in a multi-turn conversation (for structured tool use).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum ConversationMessage {
    #[serde(rename = "system")]
    System { content: String },
    #[serde(rename = "user")]
    User {
        content: String,
        /// Multi-modal content parts (images, etc.). Empty for text-only messages.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parts: Vec<ContentPart>,
    },
    #[serde(rename = "assistant")]
    Assistant {
        content: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<ToolUseRequest>,
    },
    #[serde(rename = "tool_result")]
    ToolResult(ToolResultMessage),
}

impl ConversationMessage {
    /// Estimate the character count of this message for truncation budgeting.
    /// Create a text-only user message.
    pub fn user(content: String) -> Self {
        Self::User {
            content,
            parts: Vec::new(),
        }
    }

    /// Create a multi-modal user message with content parts.
    pub fn user_with_parts(content: String, parts: Vec<ContentPart>) -> Self {
        Self::User { content, parts }
    }

    /// Estimate the character count of this message for truncation budgeting.
    pub fn char_count(&self) -> usize {
        match self {
            Self::System { content } => content.len(),
            Self::User { content, parts } => {
                content.len()
                    + parts
                        .iter()
                        .map(|p| match p {
                            ContentPart::Text { text } => text.len(),
                            ContentPart::Image { .. } => 100, // placeholder estimate for images
                            ContentPart::Audio { .. } => 100, // placeholder estimate for audio
                        })
                        .sum::<usize>()
            }
            Self::Assistant {
                content,
                tool_calls,
            } => {
                content.as_ref().map_or(0, |c| c.len())
                    + tool_calls
                        .iter()
                        .map(|tc| {
                            tc.name.len()
                                + serde_json::to_string(&tc.input).unwrap_or_default().len()
                        })
                        .sum::<usize>()
            }
            Self::ToolResult(r) => r.content.len(),
        }
    }
}

/// Why the LLM stopped generating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Other(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub role: String,
    pub content: String,
    /// Privacy boundary under which this entry was created (e.g. "local_only",
    /// "encrypted_only", "any"). Empty string means unrestricted (visible to all).
    #[serde(default)]
    pub privacy_boundary: String,
    /// Channel that originated this entry (e.g. "telegram", "cli").
    #[serde(default)]
    pub source_channel: Option<String>,
    /// Conversation this entry belongs to. Empty string means global (legacy behavior).
    #[serde(default)]
    pub conversation_id: String,
    /// ISO-8601 timestamp when this entry was created (populated on retrieval from storage).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Optional expiration timestamp (unix seconds). `None` means the entry
    /// never expires. Expired entries are excluded from queries and removed by
    /// periodic garbage collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    /// Organization that owns this entry (multi-tenancy isolation).
    /// Empty string means no org restriction (single-tenant / legacy).
    #[serde(default)]
    pub org_id: String,
    /// Agent that created this entry (per-agent memory isolation).
    /// Empty string means shared across all agents (legacy behavior).
    #[serde(default)]
    pub agent_id: String,
    /// Optional embedding vector for semantic recall.
    /// Stored as little-endian f32 BLOB in SQLite, excluded from JSON serialization.
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
    /// SHA-256 hash of `role + ":" + content`.  Content-addressed identifier
    /// for deduplication and integrity verification.  Empty when uncomputed.
    #[serde(default)]
    pub content_hash: String,
}

/// Structured record of a single tool execution for quality tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionRecord {
    pub tool_name: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub latency_ms: u64,
    pub timestamp: u64,
}

/// Typed audit detail — shapes-only, no content.  Structured variants make
/// it structurally impossible to log raw user/tool content in most code
/// paths.  The `Custom` escape hatch still runs through `redact_text` in the
/// `FileAuditSink`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuditDetail {
    ToolExecution {
        request_id: String,
        iteration: u32,
        tool_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_len: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    ProviderCall {
        request_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        token_count: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        truncated: Option<bool>,
    },
    MemoryOp {
        request_id: String,
        op: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_count: Option<usize>,
    },
    HookEvent {
        stage: String,
        tier: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    LoopDetection {
        request_id: String,
        iteration: u32,
        action: String,
    },
    FlowEvent {
        request_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        iteration: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
    },
    /// Escape hatch for untyped audit data.  `redact_text()` is applied to
    /// Custom values before they are written to disk.
    Custom(Value),
}

impl AuditDetail {
    /// Serialize to a `Value` for JSON inspection (used in tests and sinks).
    /// For `Custom` variants, returns the inner `Value` directly (no wrapper).
    /// For typed variants, includes the `kind` discriminator.
    pub fn to_value(&self) -> Value {
        match self {
            AuditDetail::Custom(v) => v.clone(),
            other => serde_json::to_value(other).unwrap_or(Value::Null),
        }
    }
}

impl From<Value> for AuditDetail {
    fn from(v: Value) -> Self {
        AuditDetail::Custom(v)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Monotonic sequence number within a session (0 = unsequenced).
    #[serde(default)]
    pub seq: u64,
    /// Session identifier grouping related events for replay.
    #[serde(default)]
    pub session_id: String,
    pub stage: String,
    pub detail: AuditDetail,
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
    #[error("budget exceeded: {reason}")]
    BudgetExceeded { reason: String },
}

#[async_trait]
pub trait Provider: Send + Sync {
    /// Whether this provider supports native streaming. Used by channels to
    /// decide whether to create draft messages for token-by-token updates.
    /// Defaults to `false`; override in providers that implement real streaming.
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Estimate the number of tokens in a text string.
    ///
    /// Returns `None` if the provider doesn't have a tokenizer (most cloud
    /// providers). Local providers with in-process tokenizers should override
    /// this to enable context window management and overflow prevention.
    fn estimate_tokens(&self, _text: &str) -> Option<usize> {
        None
    }

    async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult>;
    async fn complete_with_reasoning(
        &self,
        prompt: &str,
        _reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        self.complete(prompt).await
    }
    /// Stream completion tokens through `sender`. Default implementation falls
    /// back to `complete()` and sends a single chunk with the full result.
    async fn complete_streaming(
        &self,
        prompt: &str,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        let result = self.complete(prompt).await?;
        let _ = sender.send(StreamChunk {
            delta: result.output_text.clone(),
            done: true,
            tool_call_delta: None,
        });
        Ok(result)
    }
    /// Complete with structured tool definitions. The provider sends tool schemas
    /// to the LLM and returns any tool_use requests in `ChatResult::tool_calls`.
    /// Default falls back to `complete_with_reasoning()`, ignoring tools.
    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        _tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        let prompt = messages
            .iter()
            .filter_map(|m| match m {
                ConversationMessage::User { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        self.complete_with_reasoning(&prompt, reasoning).await
    }
    /// Stream completion with structured tool definitions. Sends incremental
    /// text deltas and tool call deltas through `sender`. Default falls back
    /// to non-streaming `complete_with_tools()`.
    async fn complete_streaming_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        let result = self.complete_with_tools(messages, tools, reasoning).await?;
        let _ = sender.send(StreamChunk {
            delta: result.output_text.clone(),
            done: true,
            tool_call_delta: None,
        });
        Ok(result)
    }
}

/// Parse an ISO-8601 datetime string (as returned by SQLite's `datetime()`)
/// into a unix epoch timestamp in seconds. Supports `YYYY-MM-DD HH:MM:SS` format.
fn parse_iso_to_epoch(s: &str) -> Option<i64> {
    // SQLite's datetime(created_at, 'unixepoch') produces "YYYY-MM-DD HH:MM:SS"
    let mut parts = s.split(' ');
    let date = parts.next()?;
    let time = parts.next()?;
    let mut d = date.splitn(3, '-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    let mut t = time.splitn(3, ':');
    let hour: i64 = t.next()?.parse().ok()?;
    let min: i64 = t.next()?.parse().ok()?;
    let sec: i64 = t.next()?.parse().ok()?;

    // Days from year 1970 using a simplified calculation (no leap-second accuracy needed).
    let mut days: i64 = 0;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    let month_days = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    for m in 0..(month - 1) as usize {
        days += month_days.get(m).copied().unwrap_or(30) as i64;
    }
    days += day - 1;
    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()>;
    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>>;

    /// Query recent entries filtered by privacy boundary.
    ///
    /// Only entries whose `privacy_boundary` is compatible with `boundary` are
    /// returned.  The default implementation over-fetches via `recent()` and
    /// filters in-memory; backends can override with an optimized query.
    async fn recent_for_boundary(
        &self,
        limit: usize,
        boundary: &str,
        source_channel: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        use crate::common::privacy_helpers::boundary_allows_recall;
        let all = self.recent(limit * 2).await?;
        Ok(all
            .into_iter()
            .filter(|e| {
                boundary_allows_recall(&e.privacy_boundary, boundary)
                    && source_channel.map_or(true, |ch| {
                        e.source_channel.as_deref().map_or(true, |s| s == ch)
                    })
            })
            .take(limit)
            .collect())
    }

    /// Query recent entries scoped to a specific conversation.
    async fn recent_for_conversation(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let all = self.recent(limit * 2).await?;
        Ok(all
            .into_iter()
            .filter(|e| e.conversation_id == conversation_id)
            .take(limit)
            .collect())
    }

    /// Fork a conversation: copy all entries from `from_id` into `new_id`.
    async fn fork_conversation(&self, _from_id: &str, _new_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    /// List all distinct conversation IDs in the store.
    async fn list_conversations(&self) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Remove entries whose `expires_at` timestamp has passed.
    /// Default implementation is a no-op; backends with TTL column support
    /// should override this.
    async fn gc_expired(&self) -> anyhow::Result<u64> {
        Ok(0)
    }

    /// Query recent entries scoped to an organization.
    /// Default filters in-memory; backends should override with an optimized query.
    async fn recent_for_org(&self, org_id: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let all = self.recent(limit * 2).await?;
        Ok(all
            .into_iter()
            .filter(|e| e.org_id == org_id)
            .take(limit)
            .collect())
    }

    /// Query recent entries for a conversation scoped to an organization.
    /// Ensures org A cannot read org B's transcripts.
    async fn recent_for_org_conversation(
        &self,
        org_id: &str,
        conversation_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let all = self.recent_for_conversation(conversation_id, limit).await?;
        Ok(all.into_iter().filter(|e| e.org_id == org_id).collect())
    }

    /// List conversations belonging to a specific organization.
    async fn list_conversations_for_org(&self, _org_id: &str) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Query recent entries scoped to a specific agent.
    /// Default filters in-memory; backends should override with an optimized query.
    async fn recent_for_agent(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let all = self.recent(limit * 2).await?;
        Ok(all
            .into_iter()
            .filter(|e| e.agent_id == agent_id)
            .take(limit)
            .collect())
    }

    /// Query recent entries for a conversation scoped to a specific agent.
    async fn recent_for_agent_conversation(
        &self,
        agent_id: &str,
        conversation_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let all = self.recent_for_conversation(conversation_id, limit).await?;
        Ok(all.into_iter().filter(|e| e.agent_id == agent_id).collect())
    }

    /// List conversations belonging to a specific agent.
    async fn list_conversations_for_agent(&self, _agent_id: &str) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Query recent entries within a time range (unix seconds).
    ///
    /// Both `since` and `until` are optional — omit either to leave that bound open.
    /// Default implementation over-fetches via `recent()` and filters in-memory
    /// by parsing the ISO-8601 `created_at` field; backends should override with
    /// an optimized SQL query.
    async fn recent_for_timerange(
        &self,
        since: Option<i64>,
        until: Option<i64>,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let all = self.recent(limit * 4).await?;
        Ok(all
            .into_iter()
            .filter(|e| {
                let ts = e.created_at.as_deref().and_then(parse_iso_to_epoch);
                match ts {
                    Some(t) => since.map_or(true, |s| t >= s) && until.map_or(true, |u| t <= u),
                    // Entries without timestamps pass if no bounds are set.
                    None => since.is_none() && until.is_none(),
                }
            })
            .take(limit)
            .collect())
    }

    /// Append a memory entry with an associated embedding vector.
    ///
    /// Default implementation ignores the embedding and delegates to `append()`.
    /// Backends with embedding support should override to store the vector.
    async fn append_with_embedding(
        &self,
        entry: MemoryEntry,
        _embedding: Vec<f32>,
    ) -> anyhow::Result<()> {
        self.append(entry).await
    }

    /// Retrieve entries ranked by cosine similarity to a query embedding.
    ///
    /// Default implementation loads all entries with embeddings and ranks
    /// in-process. Backends can override with optimized queries.
    async fn semantic_recall(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        use crate::embedding::cosine_similarity;

        // Load a larger window and rank by similarity.
        let candidates = self.recent(limit * 10).await?;
        let mut scored: Vec<(f32, MemoryEntry)> = candidates
            .into_iter()
            .filter_map(|entry| {
                let embedding = entry.embedding.as_ref()?;
                let sim = cosine_similarity(query_embedding, embedding);
                Some((sim, entry))
            })
            .collect();

        // Sort descending by similarity.
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored.into_iter().take(limit).map(|(_, e)| e).collect())
    }

    /// Hybrid retrieval combining keyword (substring) matching with semantic
    /// recall via reciprocal rank fusion. Returns the merged top-`limit` results.
    ///
    /// Default implementation:
    ///   1. Semantic ranking via [`semantic_recall`](Self::semantic_recall)
    ///   2. Keyword ranking by substring-matching `query_text` against
    ///      `entry.content` over a window of recent entries
    ///   3. Fuse rankings with [`reciprocal_rank_fusion`](crate::search::reciprocal_rank_fusion)
    ///
    /// Backends with native full-text search (Tantivy, FTS5) can override this
    /// to use BM25 ranking instead of substring matching.
    async fn hybrid_recall(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        use crate::search::{reciprocal_rank_fusion, DEFAULT_RRF_K};
        use std::collections::HashMap;

        if limit == 0 {
            return Ok(Vec::new());
        }

        // Over-fetch from each side so RRF has enough overlap to work with.
        let overfetch = limit * 4;

        let semantic = self.semantic_recall(query_embedding, overfetch).await?;
        let candidates = self.recent(overfetch.max(64)).await?;

        // Keyword scoring: substring match (case-insensitive). Order preserved
        // by recency, which is the natural order returned by `recent()`.
        let needle = query_text.to_ascii_lowercase();
        let keyword: Vec<MemoryEntry> = candidates
            .into_iter()
            .filter(|e| e.content.to_ascii_lowercase().contains(&needle))
            .take(overfetch)
            .collect();

        if semantic.is_empty() && keyword.is_empty() {
            return Ok(Vec::new());
        }

        // Use a stable fingerprint as the RRF key. We don't have row IDs at the
        // trait level, so hash (role, content, created_at) which is stable across
        // the same query session.
        let mut fingerprint_to_entry: HashMap<i64, MemoryEntry> = HashMap::new();
        let fp_of = |e: &MemoryEntry| -> i64 {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            e.role.hash(&mut h);
            e.content.hash(&mut h);
            e.created_at.hash(&mut h);
            h.finish() as i64
        };

        let semantic_ids: Vec<i64> = semantic
            .iter()
            .map(|e| {
                let fp = fp_of(e);
                fingerprint_to_entry.entry(fp).or_insert_with(|| e.clone());
                fp
            })
            .collect();
        let keyword_ids: Vec<i64> = keyword
            .iter()
            .map(|e| {
                let fp = fp_of(e);
                fingerprint_to_entry.entry(fp).or_insert_with(|| e.clone());
                fp
            })
            .collect();

        let fused = reciprocal_rank_fusion(&[semantic_ids, keyword_ids], DEFAULT_RRF_K);
        Ok(fused
            .into_iter()
            .filter_map(|(fp, _)| fingerprint_to_entry.remove(&fp))
            .take(limit)
            .collect())
    }

    // ── Tree-structured sessions (Plan 45 Phase 2) ──────────────────────

    /// Fork a conversation at a specific entry, creating a branch.
    ///
    /// Copies entries from `from_id` up to and including `at_entry_id` into
    /// `new_id`, and records the branch relationship in the conversation tree.
    async fn fork_conversation_at(
        &self,
        _from_id: &str,
        _new_id: &str,
        _at_entry_id: i64,
        _label: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Retrieve the conversation tree rooted at `root_id`.
    async fn conversation_tree(&self, _root_id: &str) -> anyhow::Result<Option<ConversationTree>> {
        Ok(None)
    }

    /// Get the chain of ancestor conversation IDs from a branch back to root.
    async fn conversation_ancestors(&self, _conversation_id: &str) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------------------
// Conversation tree types (Plan 45 Phase 2)
// ---------------------------------------------------------------------------

/// A node in the conversation tree, representing a single conversation branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationNode {
    pub conversation_id: String,
    pub parent_id: Option<String>,
    /// The `memory.id` at which this branch diverged from its parent.
    pub branch_point_entry_id: Option<i64>,
    pub created_at: i64,
    /// User-defined label for this branch.
    pub label: String,
}

/// A tree of related conversations sharing a common root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTree {
    pub root: String,
    pub nodes: HashMap<String, ConversationNode>,
}

/// Blanket implementation allowing `Arc<dyn MemoryStore>` to be used anywhere
/// a `Box<dyn MemoryStore>` is expected (via `Box::new(arc.clone())`).
///
/// This enables a single store instance to be shared across multiple agents
/// — e.g. the coordinator creates one `SqliteMemoryStore` and wraps it in
/// `Arc`, then each agent receives `Box::new(arc.clone())`.
#[async_trait]
impl<T: MemoryStore + ?Sized> MemoryStore for std::sync::Arc<T> {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        (**self).append(entry).await
    }

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        (**self).recent(limit).await
    }

    async fn recent_for_boundary(
        &self,
        limit: usize,
        boundary: &str,
        source_channel: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        (**self)
            .recent_for_boundary(limit, boundary, source_channel)
            .await
    }

    async fn recent_for_conversation(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        (**self)
            .recent_for_conversation(conversation_id, limit)
            .await
    }

    async fn fork_conversation(&self, from_id: &str, new_id: &str) -> anyhow::Result<()> {
        (**self).fork_conversation(from_id, new_id).await
    }

    async fn list_conversations(&self) -> anyhow::Result<Vec<String>> {
        (**self).list_conversations().await
    }

    async fn gc_expired(&self) -> anyhow::Result<u64> {
        (**self).gc_expired().await
    }

    async fn recent_for_org(&self, org_id: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        (**self).recent_for_org(org_id, limit).await
    }

    async fn recent_for_org_conversation(
        &self,
        org_id: &str,
        conversation_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        (**self)
            .recent_for_org_conversation(org_id, conversation_id, limit)
            .await
    }

    async fn recent_for_agent(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        (**self).recent_for_agent(agent_id, limit).await
    }

    async fn recent_for_agent_conversation(
        &self,
        agent_id: &str,
        conversation_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        (**self)
            .recent_for_agent_conversation(agent_id, conversation_id, limit)
            .await
    }

    async fn list_conversations_for_agent(&self, agent_id: &str) -> anyhow::Result<Vec<String>> {
        (**self).list_conversations_for_agent(agent_id).await
    }

    async fn recent_for_timerange(
        &self,
        since: Option<i64>,
        until: Option<i64>,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        (**self).recent_for_timerange(since, until, limit).await
    }

    async fn append_with_embedding(
        &self,
        entry: MemoryEntry,
        embedding: Vec<f32>,
    ) -> anyhow::Result<()> {
        (**self).append_with_embedding(entry, embedding).await
    }

    async fn semantic_recall(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        (**self).semantic_recall(query_embedding, limit).await
    }

    async fn hybrid_recall(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        (**self)
            .hybrid_recall(query_text, query_embedding, limit)
            .await
    }

    async fn fork_conversation_at(
        &self,
        from_id: &str,
        new_id: &str,
        at_entry_id: i64,
        label: &str,
    ) -> anyhow::Result<()> {
        (**self)
            .fork_conversation_at(from_id, new_id, at_entry_id, label)
            .await
    }

    async fn conversation_tree(&self, root_id: &str) -> anyhow::Result<Option<ConversationTree>> {
        (**self).conversation_tree(root_id).await
    }

    async fn conversation_ancestors(&self, conversation_id: &str) -> anyhow::Result<Vec<String>> {
        (**self).conversation_ancestors(conversation_id).await
    }
}

/// In-memory [`MemoryStore`] for ephemeral agents (workflow steps, delegates).
///
/// Entries live only for the lifetime of this struct — nothing is persisted to
/// disk. Useful when an agent does not need cross-session memory (e.g. a
/// short-lived workflow step that runs once and produces output).
#[derive(Default)]
pub struct EphemeralMemory {
    entries: std::sync::Mutex<Vec<MemoryEntry>>,
}

#[async_trait]
impl MemoryStore for EphemeralMemory {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        self.entries
            .lock()
            .expect("ephemeral memory lock poisoned")
            .push(entry);
        Ok(())
    }

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let entries = self.entries.lock().expect("ephemeral memory lock poisoned");
        Ok(entries.iter().rev().take(limit).cloned().collect())
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique tool identifier (e.g. `"read_file"`, `"shell"`).
    fn name(&self) -> &'static str;

    /// Human-readable description of what this tool does.
    /// Used in system prompts so the LLM knows when to invoke this tool.
    fn description(&self) -> &'static str {
        ""
    }

    /// JSON Schema describing the expected input parameters.
    /// Returns `None` if the tool accepts free-form text input.
    ///
    /// When provided, this enables:
    /// - Structured tool-use APIs (Anthropic tool_use, OpenAI function calling)
    /// - Input validation before execution
    /// - Auto-generated documentation
    fn input_schema(&self) -> Option<serde_json::Value> {
        None
    }

    /// Execute the tool with the given input and context.
    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult>;
}

// ---------------------------------------------------------------------------
// Skill bundle types — progressive skill loading (Plan 45 Phase 1)
// ---------------------------------------------------------------------------

/// When a skill should be activated in a session.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SkillTrigger {
    /// Load in every session automatically.
    Always,
    /// Activate when any keyword is detected in the user message.
    Keyword { keywords: Vec<String> },
    /// Only activated via explicit `/skill activate <name>` command.
    #[default]
    Manual,
}

/// A tool definition bundled inside a skill.
///
/// This is a lightweight descriptor — the actual `Box<dyn Tool>` is
/// constructed at activation time by the skill loader in `agentzero-infra`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SkillToolDef {
    /// A shell/HTTP/composite dynamic tool definition (same schema as
    /// `DynamicToolDef` from the infra crate, but stored as raw JSON so
    /// core doesn't depend on infra).
    DynamicTool {
        /// Serialized `DynamicToolDef` — deserialized lazily by the loader.
        definition: serde_json::Value,
    },
    /// An MCP server to start and expose tools from.
    McpServer {
        name: String,
        config: serde_json::Value,
    },
}

/// A self-contained skill bundle: prompt fragment + optional tool definitions.
///
/// Loaded from `.agentzero/skills/<name>/skill.toml` + `prompt.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillBundle {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub trigger: SkillTrigger,
    /// System prompt fragment injected when the skill is active.
    /// Loaded from `prompt.md` in the skill directory.
    #[serde(default)]
    pub prompt_template: String,
    /// Tools provided by this skill.
    #[serde(default)]
    pub tool_defs: Vec<SkillToolDef>,
    /// Other skills that must be active before this one.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Loading priority (lower = earlier). Default 100.
    #[serde(default = "default_skill_priority")]
    pub priority: i32,
}

fn default_skill_priority() -> i32 {
    100
}

/// Summary metadata for listing available skills without loading full bundles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillBundleMeta {
    pub name: String,
    pub description: String,
    pub trigger: SkillTrigger,
    pub has_tools: bool,
    pub dependencies: Vec<String>,
}

/// The result of activating a skill — prompt to inject and tools to register.
///
/// Cannot derive `Debug` because `Box<dyn Tool>` is not Debug, so we provide
/// a manual implementation.
pub struct SkillActivation {
    /// System prompt fragment to append after the base system prompt.
    pub prompt_fragment: String,
    /// Tools to add to the active tool set for this session.
    pub tools: Vec<Box<dyn Tool>>,
}

impl std::fmt::Debug for SkillActivation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkillActivation")
            .field("prompt_fragment", &self.prompt_fragment)
            .field("tools_count", &self.tools.len())
            .finish()
    }
}

/// Loads and manages skill bundles at runtime.
#[async_trait]
pub trait SkillLoader: Send + Sync {
    /// Load the full bundle for a skill by name.
    async fn load_bundle(&self, name: &str) -> anyhow::Result<SkillBundle>;

    /// List metadata for all available skill bundles.
    async fn list_available(&self) -> anyhow::Result<Vec<SkillBundleMeta>>;

    /// Activate a skill: load its prompt and instantiate its tools.
    async fn activate(&self, name: &str, ctx: &ToolContext) -> anyhow::Result<SkillActivation>;

    /// Deactivate a skill (remove its prompt and tools from the session).
    async fn deactivate(&self, name: &str) -> anyhow::Result<()>;
}

#[async_trait]
pub trait AuditSink: Send + Sync {
    async fn record(&self, event: AuditEvent) -> anyhow::Result<()>;
}

/// Blanket implementation allowing `Arc<dyn AuditSink>` to be used anywhere
/// a `Box<dyn AuditSink>` is expected — mirrors the `MemoryStore` Arc blanket.
#[async_trait]
impl<T: AuditSink + ?Sized> AuditSink for std::sync::Arc<T> {
    async fn record(&self, event: AuditEvent) -> anyhow::Result<()> {
        (**self).record(event).await
    }
}

#[async_trait]
pub trait HookSink: Send + Sync {
    async fn record(&self, event: HookEvent) -> anyhow::Result<()>;
}

pub trait MetricsSink: Send + Sync {
    fn increment_counter(&self, name: &'static str, value: u64);
    fn observe_histogram(&self, _name: &'static str, _value: f64) {}
}

/// Abstraction for sending a message to an agent and getting a response.
///
/// Implemented by the orchestrator (e.g. wrapping an mpsc task channel +
/// oneshot result channel). Consumed by `ConverseTool` in `agentzero-tools`.
#[async_trait]
pub trait AgentEndpoint: Send + Sync {
    /// Send a conversational message to the target agent and wait for its response.
    ///
    /// The `conversation_id` groups multiple turns together so the target agent
    /// can retrieve prior context from its memory store.
    async fn send(&self, message: &str, conversation_id: &str) -> anyhow::Result<String>;

    /// The identifier of the target agent.
    fn agent_id(&self) -> &str;
}

/// Abstraction for sending a message to a human via a channel and waiting for
/// their reply.
///
/// Implemented by the orchestrator using `ChannelRegistry` + event bus
/// subscription. Consumed by `ConverseTool` for human-in-the-loop flows.
#[async_trait]
pub trait ChannelEndpoint: Send + Sync {
    /// Send a message through `channel` to `recipient` and block until a human
    /// reply arrives (or `timeout_secs` elapses).
    async fn send_and_wait(
        &self,
        channel: &str,
        recipient: &str,
        message: &str,
        conversation_id: &str,
        timeout_secs: u64,
    ) -> anyhow::Result<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definition_round_trip() {
        let def = ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file from disk".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        };
        let json = serde_json::to_string(&def).unwrap();
        let parsed: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "read_file");
        assert_eq!(parsed.description, "Read a file from disk");
    }

    #[test]
    fn tool_use_request_round_trip() {
        let req = ToolUseRequest {
            id: "call_123".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({"path": "/tmp/foo.txt"}),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ToolUseRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "call_123");
        assert_eq!(parsed.name, "read_file");
        assert_eq!(parsed.input["path"], "/tmp/foo.txt");
    }

    #[test]
    fn tool_result_message_round_trip() {
        let msg = ToolResultMessage {
            tool_use_id: "call_123".to_string(),
            content: "file contents here".to_string(),
            is_error: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ToolResultMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tool_use_id, "call_123");
        assert!(!parsed.is_error);
    }

    #[test]
    fn conversation_message_system_serde() {
        let msg = ConversationMessage::System {
            content: "You are a helpful assistant.".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\":\"system\""));
        assert!(json.contains("You are a helpful assistant."));
        let parsed: ConversationMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            ConversationMessage::System { content } => {
                assert_eq!(content, "You are a helpful assistant.")
            }
            _ => panic!("expected System variant"),
        }
    }

    #[test]
    fn conversation_message_system_char_count() {
        let msg = ConversationMessage::System {
            content: "Be brief.".to_string(),
        };
        assert_eq!(msg.char_count(), 9);
    }

    #[test]
    fn conversation_message_user_serde() {
        let msg = ConversationMessage::user("hello".to_string());
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\":\"user\""));
        assert!(!json.contains("parts")); // empty parts omitted
        let parsed: ConversationMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            ConversationMessage::User { content, parts } => {
                assert_eq!(content, "hello");
                assert!(parts.is_empty());
            }
            _ => panic!("expected User variant"),
        }
    }

    #[test]
    fn conversation_message_user_backward_compat_no_parts() {
        // Old serialized format without `parts` field should deserialize fine.
        let json = r#"{"role":"user","content":"legacy message"}"#;
        let parsed: ConversationMessage = serde_json::from_str(json).unwrap();
        match parsed {
            ConversationMessage::User { content, parts } => {
                assert_eq!(content, "legacy message");
                assert!(parts.is_empty());
            }
            _ => panic!("expected User variant"),
        }
    }

    #[test]
    fn content_part_serde_round_trip() {
        let text = ContentPart::Text {
            text: "hello".to_string(),
        };
        let json = serde_json::to_string(&text).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        let parsed: ContentPart = serde_json::from_str(&json).unwrap();
        match parsed {
            ContentPart::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected Text variant"),
        }

        let img = ContentPart::Image {
            media_type: "image/png".to_string(),
            data: "base64data".to_string(),
        };
        let json = serde_json::to_string(&img).unwrap();
        assert!(json.contains("\"type\":\"image\""));
        let parsed: ContentPart = serde_json::from_str(&json).unwrap();
        match parsed {
            ContentPart::Image { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "base64data");
            }
            _ => panic!("expected Image variant"),
        }
    }

    #[test]
    fn memory_entry_backward_compat_no_conversation_id() {
        let json = r#"{"role":"user","content":"hello"}"#;
        let parsed: MemoryEntry = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.conversation_id, "");
    }

    #[test]
    fn conversation_message_assistant_with_tool_calls() {
        let msg = ConversationMessage::Assistant {
            content: Some("I'll read that file.".to_string()),
            tool_calls: vec![ToolUseRequest {
                id: "call_1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "/tmp/test"}),
            }],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\":\"assistant\""));
        assert!(json.contains("\"tool_calls\""));
        let parsed: ConversationMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            ConversationMessage::Assistant {
                content,
                tool_calls,
            } => {
                assert_eq!(content.unwrap(), "I'll read that file.");
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].name, "read_file");
            }
            _ => panic!("expected Assistant variant"),
        }
    }

    #[test]
    fn conversation_message_assistant_no_tool_calls_omits_field() {
        let msg = ConversationMessage::Assistant {
            content: Some("Just text.".to_string()),
            tool_calls: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("tool_calls"));
    }

    #[test]
    fn stop_reason_serde_variants() {
        let cases = vec![
            (StopReason::EndTurn, "\"EndTurn\""),
            (StopReason::ToolUse, "\"ToolUse\""),
            (StopReason::MaxTokens, "\"MaxTokens\""),
            (StopReason::StopSequence, "\"StopSequence\""),
        ];
        for (variant, expected_json) in cases {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_json);
            let parsed: StopReason = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn stop_reason_other_variant() {
        let reason = StopReason::Other("custom".to_string());
        let json = serde_json::to_string(&reason).unwrap();
        let parsed: StopReason = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, StopReason::Other("custom".to_string()));
    }

    #[test]
    fn chat_result_default_has_empty_tool_calls_and_no_stop_reason() {
        let result = ChatResult::default();
        assert!(result.output_text.is_empty());
        assert!(result.tool_calls.is_empty());
        assert!(result.stop_reason.is_none());
    }

    #[test]
    fn chat_result_serde_omits_empty_fields() {
        let result = ChatResult {
            output_text: "hello".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("tool_calls"));
        assert!(!json.contains("stop_reason"));
    }

    #[test]
    fn chat_result_serde_includes_tool_calls_when_present() {
        let result = ChatResult {
            output_text: String::new(),
            tool_calls: vec![ToolUseRequest {
                id: "id1".to_string(),
                name: "test".to_string(),
                input: serde_json::json!({}),
            }],
            stop_reason: Some(StopReason::ToolUse),
            ..Default::default()
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("tool_calls"));
        assert!(json.contains("stop_reason"));
        let parsed: ChatResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.stop_reason, Some(StopReason::ToolUse));
    }

    #[test]
    fn depth_policy_no_rules_passes_all() {
        let policy = DepthPolicy::default();
        let tools = vec!["shell", "read_file", "write_file"];
        let filtered = policy.filter_tools(0, &tools);
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn depth_policy_denylist_removes_tools() {
        let policy = DepthPolicy {
            rules: vec![DepthRule {
                max_depth: 1,
                allowed_tools: HashSet::new(),
                denied_tools: HashSet::from(["shell".to_string(), "write_file".to_string()]),
            }],
        };
        let tools = vec!["shell", "read_file", "write_file", "web_search"];
        let filtered = policy.filter_tools(1, &tools);
        assert_eq!(filtered, vec!["read_file", "web_search"]);
    }

    #[test]
    fn depth_policy_allowlist_restricts_tools() {
        let policy = DepthPolicy {
            rules: vec![DepthRule {
                max_depth: 2,
                allowed_tools: HashSet::from([
                    "read_file".to_string(),
                    "content_search".to_string(),
                ]),
                denied_tools: HashSet::new(),
            }],
        };
        let tools = vec!["shell", "read_file", "write_file", "content_search"];
        let filtered = policy.filter_tools(2, &tools);
        assert_eq!(filtered, vec!["read_file", "content_search"]);
    }

    #[test]
    fn depth_policy_allowlist_with_denylist() {
        let policy = DepthPolicy {
            rules: vec![DepthRule {
                max_depth: 1,
                allowed_tools: HashSet::from(["read_file".to_string(), "shell".to_string()]),
                denied_tools: HashSet::from(["shell".to_string()]),
            }],
        };
        let tools = vec!["shell", "read_file", "write_file"];
        let filtered = policy.filter_tools(1, &tools);
        // shell is in allowlist but also in denylist — denylist wins.
        assert_eq!(filtered, vec!["read_file"]);
    }

    #[test]
    fn depth_policy_no_matching_rule_passes_all() {
        let policy = DepthPolicy {
            rules: vec![DepthRule {
                max_depth: 0,
                allowed_tools: HashSet::new(),
                denied_tools: HashSet::from(["shell".to_string()]),
            }],
        };
        // Depth 3 doesn't match max_depth 0, so no rule applies.
        let tools = vec!["shell", "read_file"];
        let filtered = policy.filter_tools(3, &tools);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn run_id_generates_unique_ids() {
        let a = RunId::new();
        let b = RunId::new();
        assert_ne!(a, b);
        assert!(a.as_str().starts_with("run-"));
    }

    #[test]
    fn job_status_terminal_check() {
        assert!(!JobStatus::Pending.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
        assert!(JobStatus::Completed {
            result: "ok".to_string()
        }
        .is_terminal());
        assert!(JobStatus::Failed {
            error: "err".to_string()
        }
        .is_terminal());
        assert!(JobStatus::Cancelled.is_terminal());
    }

    #[test]
    fn queue_mode_serde_steer() {
        let mode = QueueMode::Steer;
        let json = serde_json::to_string(&mode).unwrap();
        assert!(json.contains("\"mode\":\"steer\""));
        let parsed: QueueMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, QueueMode::Steer);
    }

    #[test]
    fn queue_mode_serde_followup() {
        let mode = QueueMode::Followup {
            run_id: RunId("run-123".to_string()),
        };
        let json = serde_json::to_string(&mode).unwrap();
        assert!(json.contains("\"followup\""));
        let parsed: QueueMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, mode);
    }

    #[test]
    fn queue_mode_serde_collect() {
        let mode = QueueMode::Collect;
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: QueueMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, QueueMode::Collect);
    }

    #[test]
    fn queue_mode_serde_interrupt() {
        let mode = QueueMode::Interrupt;
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: QueueMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, QueueMode::Interrupt);
    }

    #[test]
    fn queue_mode_default_is_steer() {
        assert_eq!(QueueMode::default(), QueueMode::Steer);
    }

    #[test]
    fn loop_action_variants() {
        let actions = [
            LoopAction::Continue,
            LoopAction::InjectMessage("try different".to_string()),
            LoopAction::RestrictTools(vec!["shell".to_string()]),
            LoopAction::ForceComplete("budget exceeded".to_string()),
        ];
        // Verify all variants exist and are distinct.
        assert_ne!(actions[0], actions[1]);
        assert_ne!(actions[1], actions[2]);
        assert_ne!(actions[2], actions[3]);
    }

    #[test]
    fn tool_context_tracks_tokens_used() {
        let ctx = ToolContext::new("/tmp/test".to_string());
        assert_eq!(ctx.current_tokens(), 0);
        let total = ctx.add_tokens(150);
        assert_eq!(total, 150);
        let total = ctx.add_tokens(50);
        assert_eq!(total, 200);
        assert_eq!(ctx.current_tokens(), 200);
    }

    #[test]
    fn tool_context_tracks_cost_microdollars() {
        let ctx = ToolContext::new("/tmp/test".to_string());
        assert_eq!(ctx.current_cost(), 0);
        let total = ctx.add_cost(5000);
        assert_eq!(total, 5000);
        let total = ctx.add_cost(3000);
        assert_eq!(total, 8000);
        assert_eq!(ctx.current_cost(), 8000);
    }

    #[test]
    fn tool_context_budget_limits_set() {
        let mut ctx = ToolContext::new("/tmp/test".to_string());
        ctx.max_tokens = 1000;
        ctx.max_cost_microdollars = 50_000;

        // Under budget — no exceeded
        ctx.add_tokens(500);
        ctx.add_cost(25_000);
        assert!(ctx.budget_exceeded().is_none());

        // Exceed token budget
        ctx.add_tokens(600);
        let reason = ctx.budget_exceeded();
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("token budget exceeded"));

        // Reset and test cost budget exceeded
        let mut ctx2 = ToolContext::new("/tmp/test".to_string());
        ctx2.max_tokens = 0; // unlimited
        ctx2.max_cost_microdollars = 10_000;
        ctx2.add_cost(15_000);
        let reason = ctx2.budget_exceeded();
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("cost budget exceeded"));
    }

    #[test]
    fn tool_context_sender_id_defaults_to_none() {
        let ctx = ToolContext::new("/tmp/test".to_string());
        assert!(ctx.sender_id.is_none(), "sender_id should default to None");
    }
}
