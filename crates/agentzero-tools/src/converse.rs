use crate::delegate::OutputScanner;
use agentzero_core::{AgentEndpoint, ChannelEndpoint, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

/// Default maximum turns per conversation.
const DEFAULT_MAX_TURNS: usize = 10;
/// Default per-turn timeout in seconds.
const DEFAULT_TURN_TIMEOUT_SECS: u64 = 120;
/// Default maximum concurrent converse calls.
const DEFAULT_MAX_CONCURRENT: usize = 4;

#[derive(Debug, Deserialize)]
struct ConverseInput {
    /// Target agent ID (mutually exclusive with `channel`).
    agent: Option<String>,
    /// Target channel for human-in-the-loop (mutually exclusive with `agent`).
    channel: Option<String>,
    /// Channel recipient (required when `channel` is set).
    recipient: Option<String>,
    /// The message to send.
    message: String,
    /// Conversation identifier shared across turns. The caller generates this
    /// on the first turn and reuses it for subsequent turns.
    conversation_id: String,
}

pub struct ConverseTool {
    /// Agent endpoints keyed by agent_id, provided by the orchestrator.
    agents: HashMap<String, Arc<dyn AgentEndpoint>>,
    /// Optional channel endpoint for human-in-the-loop conversations.
    channel_endpoint: Option<Arc<dyn ChannelEndpoint>>,
    /// Turn count per conversation_id for enforcing max_turns.
    turn_counts: Arc<Mutex<HashMap<String, usize>>>,
    /// Maximum turns allowed per conversation.
    max_turns: usize,
    /// Per-turn timeout in seconds.
    turn_timeout_secs: u64,
    /// Concurrency limiter.
    semaphore: Arc<Semaphore>,
    /// Optional output scanner (leak guard).
    output_scanner: Option<OutputScanner>,
}

impl ConverseTool {
    pub fn new(agents: HashMap<String, Arc<dyn AgentEndpoint>>) -> Self {
        Self {
            agents,
            channel_endpoint: None,
            turn_counts: Arc::new(Mutex::new(HashMap::new())),
            max_turns: DEFAULT_MAX_TURNS,
            turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
            semaphore: Arc::new(Semaphore::new(DEFAULT_MAX_CONCURRENT)),
            output_scanner: None,
        }
    }

    pub fn with_channel_endpoint(mut self, endpoint: Arc<dyn ChannelEndpoint>) -> Self {
        self.channel_endpoint = Some(endpoint);
        self
    }

    pub fn with_max_turns(mut self, max: usize) -> Self {
        self.max_turns = max;
        self
    }

    pub fn with_turn_timeout_secs(mut self, secs: u64) -> Self {
        self.turn_timeout_secs = secs;
        self
    }

    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        self.semaphore = Arc::new(Semaphore::new(max));
        self
    }

    pub fn with_output_scanner(mut self, scanner: OutputScanner) -> Self {
        self.output_scanner = Some(scanner);
        self
    }

    /// List the agent IDs available for conversation.
    fn available_agents(&self) -> Vec<&str> {
        self.agents.keys().map(|s| s.as_str()).collect()
    }
}

#[async_trait]
impl Tool for ConverseTool {
    fn name(&self) -> &'static str {
        "converse"
    }

    fn description(&self) -> &'static str {
        "Have a multi-turn conversation with another agent or a human via channel. \
         Each call is one turn — call repeatedly with the same conversation_id to \
         continue the conversation. You control when to stop."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "description": "Target agent ID to converse with (mutually exclusive with channel)"
                },
                "channel": {
                    "type": "string",
                    "description": "Target channel for human-in-the-loop (mutually exclusive with agent)"
                },
                "recipient": {
                    "type": "string",
                    "description": "Channel recipient (required when channel is set)"
                },
                "message": {
                    "type": "string",
                    "description": "The message to send"
                },
                "conversation_id": {
                    "type": "string",
                    "description": "Shared conversation identifier. Generate on first turn, reuse for subsequent turns."
                }
            },
            "required": ["message", "conversation_id"],
            "additionalProperties": false
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        if ctx.is_cancelled() {
            return Ok(ToolResult {
                output: "[Conversation cancelled]".to_string(),
            });
        }

        let parsed: ConverseInput =
            serde_json::from_str(input).map_err(|e| anyhow::anyhow!("invalid input: {e}"))?;

        // Validate mutually exclusive targeting.
        match (&parsed.agent, &parsed.channel) {
            (Some(_), Some(_)) => {
                anyhow::bail!("specify either `agent` or `channel`, not both");
            }
            (None, None) => {
                let available = self.available_agents();
                anyhow::bail!(
                    "must specify `agent` or `channel`. Available agents: {}",
                    available.join(", ")
                );
            }
            _ => {}
        }

        // Enforce turn limit.
        {
            let mut counts = self.turn_counts.lock().await;
            let count = counts.entry(parsed.conversation_id.clone()).or_insert(0);
            if *count >= self.max_turns {
                return Ok(ToolResult {
                    output: format!(
                        "Conversation turn limit reached ({}/{}). \
                         Summarize the conversation and conclude.",
                        *count, self.max_turns
                    ),
                });
            }
            *count += 1;
        }

        // Acquire concurrency permit.
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| anyhow::anyhow!("converse semaphore closed"))?;

        let raw_output =
            if let Some(ref agent_id) = parsed.agent {
                // Agent-to-agent conversation.
                let endpoint = self.agents.get(agent_id).ok_or_else(|| {
                    let available = self.available_agents();
                    anyhow::anyhow!(
                        "unknown agent: `{agent_id}`. Available: {}",
                        available.join(", ")
                    )
                })?;

                tracing::info!(
                    target_agent = %agent_id,
                    conversation_id = %parsed.conversation_id,
                    "converse: sending message to agent"
                );

                endpoint
                    .send(&parsed.message, &parsed.conversation_id)
                    .await?
            } else if let Some(ref channel) = parsed.channel {
                // Human-in-the-loop conversation.
                let channel_ep = self.channel_endpoint.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("no channel endpoint configured for converse")
                })?;

                let recipient = parsed.recipient.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("`recipient` is required when using `channel`")
                })?;

                tracing::info!(
                    channel = %channel,
                    recipient = %recipient,
                    conversation_id = %parsed.conversation_id,
                    "converse: sending message to human"
                );

                channel_ep
                    .send_and_wait(
                        channel,
                        recipient,
                        &parsed.message,
                        &parsed.conversation_id,
                        self.turn_timeout_secs,
                    )
                    .await?
            } else {
                unreachable!("validated above");
            };

        // Leak guard: scan output for credentials.
        let safe_output = if let Some(ref scanner) = self.output_scanner {
            scanner(&raw_output).map_err(|blocked| {
                tracing::warn!(
                    conversation_id = %parsed.conversation_id,
                    "converse output blocked by leak guard: {blocked}"
                );
                anyhow::anyhow!("converse output blocked: credential leak detected in response")
            })?
        } else {
            raw_output
        };

        Ok(ToolResult {
            output: safe_output,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Mock agent endpoint that echoes messages with a turn counter.
    struct MockEndpoint {
        id: String,
        call_count: AtomicU64,
    }

    impl MockEndpoint {
        fn new(id: &str) -> Self {
            Self {
                id: id.to_string(),
                call_count: AtomicU64::new(0),
            }
        }
    }

    #[async_trait]
    impl AgentEndpoint for MockEndpoint {
        async fn send(&self, message: &str, conversation_id: &str) -> anyhow::Result<String> {
            let n = self.call_count.fetch_add(1, Ordering::Relaxed) + 1;
            Ok(format!(
                "[{} turn {n} conv={conversation_id}] echo: {message}",
                self.id
            ))
        }

        fn agent_id(&self) -> &str {
            &self.id
        }
    }

    /// Mock channel endpoint for human-in-the-loop tests.
    struct MockChannelEndpoint;

    #[async_trait]
    impl ChannelEndpoint for MockChannelEndpoint {
        async fn send_and_wait(
            &self,
            channel: &str,
            recipient: &str,
            _message: &str,
            _conversation_id: &str,
            _timeout_secs: u64,
        ) -> anyhow::Result<String> {
            Ok(format!("[{channel}:{recipient}] Approved!"))
        }
    }

    fn test_ctx() -> ToolContext {
        ToolContext::new("/tmp".to_string())
    }

    fn tool_with_mock_agents() -> ConverseTool {
        let mut agents: HashMap<String, Arc<dyn AgentEndpoint>> = HashMap::new();
        agents.insert("analyst".into(), Arc::new(MockEndpoint::new("analyst")));
        agents.insert(
            "researcher".into(),
            Arc::new(MockEndpoint::new("researcher")),
        );
        ConverseTool::new(agents)
    }

    #[tokio::test]
    async fn converse_basic_agent_turn() {
        let tool = tool_with_mock_agents();
        let result = tool
            .execute(
                r#"{"agent":"analyst","message":"What do you think?","conversation_id":"conv-1"}"#,
                &test_ctx(),
            )
            .await
            .unwrap();
        assert!(result.output.contains("echo: What do you think?"));
        assert!(result.output.contains("analyst"));
        assert!(result.output.contains("conv-1"));
    }

    #[tokio::test]
    async fn converse_multi_turn_tracks_count() {
        let tool = tool_with_mock_agents().with_max_turns(3);
        let ctx = test_ctx();

        for i in 1..=3 {
            let result = tool
                .execute(
                    &format!(
                        r#"{{"agent":"analyst","message":"turn {i}","conversation_id":"conv-mt"}}"#
                    ),
                    &ctx,
                )
                .await
                .unwrap();
            assert!(
                result.output.contains(&format!("turn {i}")),
                "turn {i}: {}",
                result.output
            );
        }

        // 4th turn should hit the limit.
        let result = tool
            .execute(
                r#"{"agent":"analyst","message":"turn 4","conversation_id":"conv-mt"}"#,
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.output.contains("turn limit reached"));
    }

    #[tokio::test]
    async fn converse_separate_conversations_independent() {
        let tool = tool_with_mock_agents().with_max_turns(2);
        let ctx = test_ctx();

        // Use up 2 turns on conv-a.
        for _ in 0..2 {
            tool.execute(
                r#"{"agent":"analyst","message":"hi","conversation_id":"conv-a"}"#,
                &ctx,
            )
            .await
            .unwrap();
        }

        // conv-b should still work.
        let result = tool
            .execute(
                r#"{"agent":"analyst","message":"hi","conversation_id":"conv-b"}"#,
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.output.contains("echo: hi"));
    }

    #[tokio::test]
    async fn converse_unknown_agent_returns_error() {
        let tool = tool_with_mock_agents();
        let err = tool
            .execute(
                r#"{"agent":"unknown","message":"hi","conversation_id":"c"}"#,
                &test_ctx(),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown agent"));
        assert!(err.to_string().contains("analyst"));
    }

    #[tokio::test]
    async fn converse_requires_agent_or_channel() {
        let tool = tool_with_mock_agents();
        let err = tool
            .execute(r#"{"message":"hi","conversation_id":"c"}"#, &test_ctx())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("must specify"));
    }

    #[tokio::test]
    async fn converse_rejects_both_agent_and_channel() {
        let tool = tool_with_mock_agents();
        let err = tool
            .execute(
                r#"{"agent":"analyst","channel":"slack","message":"hi","conversation_id":"c"}"#,
                &test_ctx(),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not both"));
    }

    #[tokio::test]
    async fn converse_channel_requires_recipient() {
        let tool = tool_with_mock_agents().with_channel_endpoint(Arc::new(MockChannelEndpoint));
        let err = tool
            .execute(
                r#"{"channel":"slack","message":"hi","conversation_id":"c"}"#,
                &test_ctx(),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("recipient"));
    }

    #[tokio::test]
    async fn converse_channel_human_in_the_loop() {
        let tool = tool_with_mock_agents().with_channel_endpoint(Arc::new(MockChannelEndpoint));
        let result = tool
            .execute(
                r##"{"channel":"slack","recipient":"#eng","message":"Approve?","conversation_id":"c"}"##,
                &test_ctx(),
            )
            .await
            .unwrap();
        assert!(result.output.contains("Approved!"));
        assert!(result.output.contains("slack"));
    }

    #[tokio::test]
    async fn converse_no_channel_endpoint_returns_error() {
        let tool = tool_with_mock_agents(); // No channel endpoint.
        let err = tool
            .execute(
                r##"{"channel":"slack","recipient":"#eng","message":"hi","conversation_id":"c"}"##,
                &test_ctx(),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no channel endpoint"));
    }

    #[tokio::test]
    async fn converse_cancelled_returns_early() {
        let tool = tool_with_mock_agents();
        let ctx = test_ctx();
        ctx.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let result = tool
            .execute(
                r#"{"agent":"analyst","message":"hi","conversation_id":"c"}"#,
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.output, "[Conversation cancelled]");
    }

    #[tokio::test]
    async fn converse_output_scanner_redacts() {
        let tool = tool_with_mock_agents().with_output_scanner(Arc::new(|text| {
            if text.contains("secret") {
                Ok(text.replace("secret", "[REDACTED]"))
            } else {
                Ok(text.to_string())
            }
        }));

        // The mock endpoint echoes "secret" back — scanner should redact it.
        let result = tool
            .execute(
                r#"{"agent":"analyst","message":"the secret code","conversation_id":"c"}"#,
                &test_ctx(),
            )
            .await
            .unwrap();
        assert!(result.output.contains("[REDACTED]"));
        assert!(!result.output.contains("secret"));
    }

    #[tokio::test]
    async fn converse_output_scanner_blocks() {
        let tool = tool_with_mock_agents().with_output_scanner(Arc::new(|text| {
            if text.contains("sk-") {
                Err("credential leak".to_string())
            } else {
                Ok(text.to_string())
            }
        }));

        let err = tool
            .execute(
                r#"{"agent":"analyst","message":"key is sk-abc","conversation_id":"c"}"#,
                &test_ctx(),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("credential leak"));
    }

    #[tokio::test]
    async fn converse_invalid_json_returns_error() {
        let tool = tool_with_mock_agents();
        let err = tool.execute("not json", &test_ctx()).await.unwrap_err();
        assert!(err.to_string().contains("invalid input"));
    }
}
