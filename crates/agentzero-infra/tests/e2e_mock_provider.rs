//! End-to-end agent loop tests using mock providers.
//!
//! Exercises the full agent execution cycle (provider → tool dispatch → response)
//! entirely offline using testkit fakes and purpose-built scripted providers.

use agentzero_core::{
    AgentConfig, ChatResult, ConversationMessage, ReasoningConfig, StopReason, Tool, ToolContext,
    ToolDefinition, ToolResult, ToolUseRequest,
};
use agentzero_infra::runtime::{run_agent_with_runtime, RuntimeExecution};
use agentzero_testkit::{FailingProvider, StaticProvider, TestMemoryStore};
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Test tools
// ---------------------------------------------------------------------------

/// A tool with `input_schema()` so the structured tool-use path activates.
/// Echoes back the `message` field from JSON input.
struct SchemaEchoTool;

#[async_trait]
impl Tool for SchemaEchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self) -> &'static str {
        "Echo back the message field"
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(json!({
            "type": "object",
            "required": ["message"],
            "properties": {
                "message": {"type": "string"}
            }
        }))
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let v: serde_json::Value =
            serde_json::from_str(input).unwrap_or(serde_json::Value::String(input.to_string()));
        let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or(input);
        Ok(ToolResult {
            output: format!("echoed:{msg}"),
        })
    }
}

// ---------------------------------------------------------------------------
// Scripted providers
// ---------------------------------------------------------------------------

/// Provider that issues one tool call on the first invocation, then returns a
/// final text response incorporating the tool result on the second call.
struct ToolCallThenRespondProvider {
    call_count: AtomicUsize,
}

impl ToolCallThenRespondProvider {
    fn new() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl agentzero_core::Provider for ToolCallThenRespondProvider {
    async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
        Ok(ChatResult::default())
    }

    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        _tools: &[ToolDefinition],
        _reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        let n = self.call_count.fetch_add(1, Ordering::Relaxed);
        if n == 0 {
            // First call: request the echo tool.
            Ok(ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    input: json!({"message": "ping"}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            })
        } else {
            // Subsequent calls: extract tool result and return final response.
            let tool_output = messages
                .iter()
                .rev()
                .find_map(|m| match m {
                    ConversationMessage::ToolResult(r) => Some(r.content.as_str()),
                    _ => None,
                })
                .unwrap_or("no tool result");
            Ok(ChatResult {
                output_text: format!("final:{tool_output}"),
                tool_calls: vec![],
                stop_reason: Some(StopReason::EndTurn),
                ..Default::default()
            })
        }
    }
}

/// Provider that always returns a tool call on every invocation. Used to test
/// that `max_tool_iterations` is respected.
struct AlwaysToolCallProvider {
    call_count: Arc<AtomicUsize>,
}

impl AlwaysToolCallProvider {
    fn new() -> (Self, Arc<AtomicUsize>) {
        let counter = Arc::new(AtomicUsize::new(0));
        (
            Self {
                call_count: Arc::clone(&counter),
            },
            counter,
        )
    }
}

#[async_trait]
impl agentzero_core::Provider for AlwaysToolCallProvider {
    async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
        Ok(ChatResult::default())
    }

    async fn complete_with_tools(
        &self,
        _messages: &[ConversationMessage],
        _tools: &[ToolDefinition],
        _reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        let n = self.call_count.fetch_add(1, Ordering::Relaxed);
        Ok(ChatResult {
            output_text: String::new(),
            tool_calls: vec![ToolUseRequest {
                id: format!("call_{n}"),
                name: "echo".to_string(),
                input: json!({"message": format!("iter-{n}")}),
            }],
            stop_reason: Some(StopReason::ToolUse),
            ..Default::default()
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Single-turn agent with no tools: the static provider's text is returned
/// directly without entering the tool loop.
#[tokio::test]
async fn agent_single_turn_no_tools() {
    let execution = RuntimeExecution {
        config: AgentConfig {
            model_supports_tool_use: false,
            ..Default::default()
        },
        provider: Box::new(StaticProvider {
            output_text: "Hello world".to_string(),
        }),
        memory: Box::new(TestMemoryStore::default()),
        tools: vec![],
        audit_sink: None,
        hook_sink: None,
        conversation_id: None,
        audio_config: None,
        max_tokens: 0,
        max_cost_microdollars: 0,
        cost_config: Default::default(),
        data_dir: std::path::PathBuf::from("/tmp"),
        tool_selector: None,
        source_channel: None,
        sender_id: None,
        dynamic_registry: None,
        task_manager: None,
        tool_evolver: None,
        tool_fallback: None,
        recipe_store: None,
        pattern_capture: None,
        embedding_provider: None,
        trajectory_recorder: None,
        model_name: String::new(),
        capability_set: Default::default(),
    };

    let output = run_agent_with_runtime(execution, PathBuf::from("/tmp"), "hi".to_string())
        .await
        .expect("single-turn agent should succeed");

    assert_eq!(output.response_text, "Hello world");
}

/// Agent with a tool-calling provider: the provider issues one tool call to
/// `SchemaEchoTool`, the agent executes it, then the provider returns a final
/// response that includes the echoed text.
#[tokio::test]
async fn agent_with_echo_tool() {
    let execution = RuntimeExecution {
        config: AgentConfig {
            model_supports_tool_use: true,
            request_timeout_ms: 10_000,
            ..Default::default()
        },
        provider: Box::new(ToolCallThenRespondProvider::new()),
        memory: Box::new(TestMemoryStore::default()),
        tools: vec![Box::new(SchemaEchoTool) as Box<dyn Tool>],
        audit_sink: None,
        hook_sink: None,
        conversation_id: None,
        audio_config: None,
        max_tokens: 0,
        max_cost_microdollars: 0,
        cost_config: Default::default(),
        data_dir: std::path::PathBuf::from("/tmp"),
        tool_selector: None,
        source_channel: None,
        sender_id: None,
        dynamic_registry: None,
        task_manager: None,
        tool_evolver: None,
        tool_fallback: None,
        recipe_store: None,
        pattern_capture: None,
        embedding_provider: None,
        trajectory_recorder: None,
        model_name: String::new(),
        capability_set: Default::default(),
    };

    let output = run_agent_with_runtime(
        execution,
        PathBuf::from("/tmp"),
        "Please echo ping".to_string(),
    )
    .await
    .expect("tool-calling agent should succeed");

    // The provider's second call sees "echoed:ping" from SchemaEchoTool and
    // wraps it into "final:echoed:ping".
    assert_eq!(output.response_text, "final:echoed:ping");
}

/// Agent with `max_tool_iterations=2` and a provider that always returns tool
/// calls. The loop must stop after exactly 2 iterations.
#[tokio::test]
async fn agent_respects_max_iterations() {
    let (provider, call_count) = AlwaysToolCallProvider::new();

    let execution = RuntimeExecution {
        config: AgentConfig {
            model_supports_tool_use: true,
            max_tool_iterations: 2,
            request_timeout_ms: 10_000,
            ..Default::default()
        },
        provider: Box::new(provider),
        memory: Box::new(TestMemoryStore::default()),
        tools: vec![Box::new(SchemaEchoTool) as Box<dyn Tool>],
        audit_sink: None,
        hook_sink: None,
        conversation_id: None,
        audio_config: None,
        max_tokens: 0,
        max_cost_microdollars: 0,
        cost_config: Default::default(),
        data_dir: std::path::PathBuf::from("/tmp"),
        tool_selector: None,
        source_channel: None,
        sender_id: None,
        dynamic_registry: None,
        task_manager: None,
        tool_evolver: None,
        tool_fallback: None,
        recipe_store: None,
        pattern_capture: None,
        embedding_provider: None,
        trajectory_recorder: None,
        model_name: String::new(),
        capability_set: Default::default(),
    };

    // The loop runs `max_tool_iterations` iterations (0..2). Each iteration
    // calls the provider, which returns a tool call. After the loop exhausts
    // its iterations, the agent makes one final provider call with no tools
    // to get a closing response. Total: max_tool_iterations + 1 = 3 calls.
    let _output =
        run_agent_with_runtime(execution, PathBuf::from("/tmp"), "loop forever".to_string())
            .await
            .expect("agent should complete without error");

    let calls = call_count.load(Ordering::Relaxed);
    assert_eq!(
        calls, 3,
        "provider should be called max_tool_iterations(2) + 1 final = 3 times, got {calls}"
    );
}

/// Agent with `FailingProvider` propagates the error through the runtime.
#[tokio::test]
async fn agent_with_failing_provider_returns_error() {
    let execution = RuntimeExecution {
        config: AgentConfig::default(),
        provider: Box::new(FailingProvider),
        memory: Box::new(TestMemoryStore::default()),
        tools: vec![],
        audit_sink: None,
        hook_sink: None,
        conversation_id: None,
        audio_config: None,
        max_tokens: 0,
        max_cost_microdollars: 0,
        cost_config: Default::default(),
        data_dir: std::path::PathBuf::from("/tmp"),
        tool_selector: None,
        source_channel: None,
        sender_id: None,
        dynamic_registry: None,
        task_manager: None,
        tool_evolver: None,
        tool_fallback: None,
        recipe_store: None,
        pattern_capture: None,
        embedding_provider: None,
        trajectory_recorder: None,
        model_name: String::new(),
        capability_set: Default::default(),
    };

    let err = run_agent_with_runtime(execution, PathBuf::from("/tmp"), "hello".to_string())
        .await
        .expect_err("failing provider should propagate error");

    assert!(
        err.to_string().contains("testkit provider failure"),
        "error should contain the testkit failure message, got: {err}"
    );
}

/// Setting the `ToolContext.cancelled` flag before running causes the agent
/// loop to exit early with an "[Execution cancelled]" response.
///
/// Cancellation is checked at the top of each tool-use iteration, so the
/// provider must return at least one tool call to enter the loop.
#[tokio::test]
async fn agent_cancellation_stops_loop() {
    // We use a provider that always returns tool calls so the loop iterates.
    let (provider, _call_count) = AlwaysToolCallProvider::new();

    // Build the agent and context manually so we can set the cancelled flag.
    let config = AgentConfig {
        model_supports_tool_use: true,
        max_tool_iterations: 100,
        request_timeout_ms: 10_000,
        ..Default::default()
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = Arc::clone(&cancelled);

    // We need a provider that sets the cancelled flag after the first call
    // so the second iteration detects it.
    struct CancelOnSecondCallProvider {
        inner: AlwaysToolCallProvider,
        cancelled: Arc<AtomicBool>,
    }

    #[async_trait]
    impl agentzero_core::Provider for CancelOnSecondCallProvider {
        async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
            self.inner.complete(prompt).await
        }

        async fn complete_with_tools(
            &self,
            messages: &[ConversationMessage],
            tools: &[ToolDefinition],
            reasoning: &ReasoningConfig,
        ) -> anyhow::Result<ChatResult> {
            let result = self
                .inner
                .complete_with_tools(messages, tools, reasoning)
                .await?;
            // After the first provider call completes, set cancelled so the
            // loop will detect it on the next iteration.
            self.cancelled.store(true, Ordering::Relaxed);
            Ok(result)
        }
    }

    let cancel_provider = CancelOnSecondCallProvider {
        inner: provider,
        cancelled: Arc::clone(&cancelled),
    };

    let memory = Box::new(TestMemoryStore::default());
    let tools: Vec<Box<dyn Tool>> = vec![Box::new(SchemaEchoTool)];

    let mut agent = agentzero_core::Agent::new(config, Box::new(cancel_provider), memory, tools);

    let runtime_metrics = agentzero_core::RuntimeMetrics::new();
    agent = agent.with_metrics(Box::new(runtime_metrics));

    let mut ctx = ToolContext::new("/tmp".to_string());
    ctx.cancelled = cancelled_clone;

    let result = agent
        .respond(
            agentzero_core::UserMessage {
                text: "cancel me".to_string(),
            },
            &ctx,
        )
        .await
        .expect("cancelled agent should return Ok, not Err");

    assert_eq!(
        result.text, "[Execution cancelled]",
        "response should indicate cancellation"
    );
}
