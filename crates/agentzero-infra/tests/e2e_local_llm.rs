//! End-to-end tests that exercise the full agent loop with mock providers.
//!
//! These tests cover the same scenarios as real-LLM tests (tool use,
//! multi-turn memory, routing) but use scripted providers so they run
//! deterministically without external services.

use agentzero_core::{
    AgentConfig, ChatResult, ConversationMessage, MemoryStore, Provider, ReasoningConfig,
    StopReason, Tool, ToolContext, ToolDefinition, ToolResult, ToolUseRequest,
};
use agentzero_infra::runtime::{run_agent_with_runtime, RuntimeExecution};
use agentzero_testkit::{StaticProvider, TestMemoryStore};
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Echo tool with an `input_schema` so the agent's tool-use path activates.
struct SchemaEchoTool;

#[async_trait]
impl Tool for SchemaEchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self) -> &'static str {
        "Echo back the message"
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

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ---------------------------------------------------------------------------
// Scripted providers for e2e scenarios
// ---------------------------------------------------------------------------

/// Provider that issues one tool call to `echo`, then returns a final response
/// incorporating the tool result.
struct EchoToolCallProvider {
    call_count: AtomicUsize,
}

impl EchoToolCallProvider {
    fn new() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl Provider for EchoToolCallProvider {
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
            Ok(ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    input: json!({"message": "hello world"}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            })
        } else {
            let tool_output = messages
                .iter()
                .rev()
                .find_map(|m| match m {
                    ConversationMessage::ToolResult(r) => Some(r.content.as_str()),
                    _ => None,
                })
                .unwrap_or("no tool result");
            Ok(ChatResult {
                output_text: format!("Tool said: {tool_output}"),
                tool_calls: vec![],
                stop_reason: Some(StopReason::EndTurn),
                ..Default::default()
            })
        }
    }
}

/// Provider that echoes back the most recent user message from memory entries,
/// simulating multi-turn awareness.
struct MemoryAwareProvider;

#[async_trait]
impl Provider for MemoryAwareProvider {
    async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
        Ok(ChatResult {
            output_text: format!("received: {prompt}"),
            ..Default::default()
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_basic_completion() {
    let execution = RuntimeExecution {
        config: AgentConfig {
            max_tool_iterations: 0,
            model_supports_tool_use: false,
            ..Default::default()
        },
        provider: Box::new(StaticProvider {
            output_text: "Hello from mock LLM".to_string(),
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
        recipe_store: None,
        pattern_capture: None,
        embedding_provider: None,
    };

    let output = run_agent_with_runtime(execution, workspace_root(), "Say hello.".to_string())
        .await
        .expect("agent should produce a response");

    assert_eq!(output.response_text, "Hello from mock LLM");
}

#[tokio::test]
async fn e2e_agent_with_echo_tool() {
    let execution = RuntimeExecution {
        config: AgentConfig {
            max_tool_iterations: 3,
            model_supports_tool_use: true,
            request_timeout_ms: 10_000,
            ..Default::default()
        },
        provider: Box::new(EchoToolCallProvider::new()),
        memory: Box::new(TestMemoryStore::default()),
        tools: vec![Box::new(SchemaEchoTool)],
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
        recipe_store: None,
        pattern_capture: None,
        embedding_provider: None,
    };

    let output = run_agent_with_runtime(
        execution,
        workspace_root(),
        "Use the echo tool with the input 'hello world'.".to_string(),
    )
    .await
    .expect("agent should produce a response");

    assert!(
        output.response_text.contains("echoed:hello world"),
        "expected tool result in response, got: {}",
        output.response_text
    );
}

#[tokio::test]
async fn e2e_multi_turn_memory() {
    let memory = TestMemoryStore::default();

    // Turn 1: establish a fact.
    let execution1 = RuntimeExecution {
        config: AgentConfig {
            max_tool_iterations: 0,
            model_supports_tool_use: false,
            ..Default::default()
        },
        provider: Box::new(StaticProvider {
            output_text: "Got it, your name is TestUser42.".to_string(),
        }),
        memory: Box::new(memory.clone()),
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
        recipe_store: None,
        pattern_capture: None,
        embedding_provider: None,
    };

    let output1 = run_agent_with_runtime(
        execution1,
        workspace_root(),
        "My name is TestUser42. Remember that.".to_string(),
    )
    .await
    .expect("turn 1 should succeed");

    assert!(!output1.response_text.is_empty());

    // Verify memory was populated.
    let entries = memory.recent(10).await.expect("memory should be readable");
    assert!(
        entries.len() >= 2,
        "memory should have at least user + assistant entries, got {}",
        entries.len()
    );

    // Turn 2: use MemoryAwareProvider — it reads the prompt which includes
    // memory context injected by the runtime.
    let execution2 = RuntimeExecution {
        config: AgentConfig {
            max_tool_iterations: 0,
            model_supports_tool_use: false,
            ..Default::default()
        },
        provider: Box::new(MemoryAwareProvider),
        memory: Box::new(memory.clone()),
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
        recipe_store: None,
        pattern_capture: None,
        embedding_provider: None,
    };

    let output2 =
        run_agent_with_runtime(execution2, workspace_root(), "What is my name?".to_string())
            .await
            .expect("turn 2 should succeed");

    assert!(
        !output2.response_text.is_empty(),
        "expected non-empty response on turn 2"
    );
}

#[tokio::test]
async fn e2e_agent_router_keyword_fallback() {
    use agentzero_orchestrator::{AgentDescriptor, AgentRouter};

    let router = AgentRouter::new(None, true);

    let agents = vec![
        AgentDescriptor {
            id: "image-gen".to_string(),
            name: "Image Generator".to_string(),
            description: "Creates images and illustrations from text descriptions".to_string(),
            keywords: vec!["draw".into(), "image".into(), "picture".into()],
            subscribes_to: vec![],
            produces: vec![],
            privacy_boundary: "any".to_string(),
        },
        AgentDescriptor {
            id: "code-review".to_string(),
            name: "Code Reviewer".to_string(),
            description: "Reviews source code for bugs, style issues, and improvements".to_string(),
            keywords: vec!["review".into(), "code".into(), "PR".into()],
            subscribes_to: vec![],
            produces: vec![],
            privacy_boundary: "any".to_string(),
        },
    ];

    // "draw" keyword should route to image-gen.
    let result = router
        .route(
            "Please draw me a picture of a sunset over mountains",
            &agents,
        )
        .await
        .expect("routing should succeed");

    assert_eq!(
        result.as_deref(),
        Some("image-gen"),
        "should route to image-gen based on 'draw' and 'picture' keywords"
    );
}
