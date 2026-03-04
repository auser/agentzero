use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone)]
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
        }
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResult {
    pub output_text: String,
}

/// A single chunk emitted during streaming completion.
#[derive(Debug, Clone)]
pub struct StreamChunk {
    /// Incremental text delta for this chunk.
    pub delta: String,
    /// True when the stream is complete (final chunk).
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    pub workspace_root: String,
    #[serde(default)]
    pub allow_sensitive_file_reads: bool,
    #[serde(default)]
    pub allow_sensitive_file_writes: bool,
}

impl ToolContext {
    pub fn new(workspace_root: String) -> Self {
        Self {
            workspace_root,
            allow_sensitive_file_reads: false,
            allow_sensitive_file_writes: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub stage: String,
    pub detail: Value,
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
}

#[async_trait]
pub trait Provider: Send + Sync {
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
        });
        Ok(result)
    }
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()>;
    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>>;
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

#[async_trait]
pub trait AuditSink: Send + Sync {
    async fn record(&self, event: AuditEvent) -> anyhow::Result<()>;
}

#[async_trait]
pub trait HookSink: Send + Sync {
    async fn record(&self, event: HookEvent) -> anyhow::Result<()>;
}

pub trait MetricsSink: Send + Sync {
    fn increment_counter(&self, name: &'static str, value: u64);
    fn observe_histogram(&self, _name: &'static str, _value: f64) {}
}
