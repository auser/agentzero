use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;

use crate::event_bus::EventBus;

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
    /// Optional system prompt sent to the LLM at the start of each conversation.
    pub system_prompt: Option<String>,
    /// Effective privacy boundary for this agent: "local_only", "encrypted_only", "any", or empty (inherit).
    pub privacy_boundary: String,
    /// Per-tool privacy boundaries. Keys are tool names, values are boundary
    /// strings that override the agent-level boundary for that tool.
    pub tool_boundaries: HashMap<String, String>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatResult {
    pub output_text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolUseRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
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
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("tool_calls"));
        assert!(json.contains("stop_reason"));
        let parsed: ChatResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.stop_reason, Some(StopReason::ToolUse));
    }
}
