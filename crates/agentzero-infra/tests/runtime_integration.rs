use agentzero_core::{
    AgentConfig, AuditEvent, AuditSink, ChatResult, ConversationMessage, ReasoningConfig,
    StopReason, StreamChunk, Tool, ToolContext, ToolDefinition, ToolResult, ToolUseRequest,
};
use agentzero_infra::runtime::{run_agent_streaming, run_agent_with_runtime, RuntimeExecution};
use agentzero_infra::tools::{default_tools, ToolSecurityPolicy};
use agentzero_testkit::{EchoTool, FailingProvider, StaticProvider, TestMemoryStore};
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// --- Test audit sink ---

#[derive(Default, Clone)]
struct TestAuditSink {
    events: Arc<Mutex<Vec<AuditEvent>>>,
}

#[async_trait]
impl AuditSink for TestAuditSink {
    async fn record(&self, event: AuditEvent) -> anyhow::Result<()> {
        self.events.lock().expect("audit lock poisoned").push(event);
        Ok(())
    }
}

impl TestAuditSink {
    fn events(&self) -> Vec<AuditEvent> {
        self.events.lock().expect("audit lock poisoned").clone()
    }
}

// --- Helpers ---

fn static_execution(response: &str) -> RuntimeExecution {
    RuntimeExecution {
        config: AgentConfig::default(),
        provider: Box::new(StaticProvider {
            output_text: response.to_string(),
        }),
        memory: Box::new(TestMemoryStore::default()),
        tools: vec![Box::new(EchoTool) as Box<dyn Tool>],
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
    }
}

fn streaming_execution(response: &str) -> RuntimeExecution {
    RuntimeExecution {
        config: AgentConfig {
            model_supports_tool_use: false,
            ..Default::default()
        },
        provider: Box::new(StreamingStaticProvider {
            response: response.to_string(),
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
    }
}

// A provider that sends streaming chunks
struct StreamingStaticProvider {
    response: String,
}

#[async_trait]
impl agentzero_core::Provider for StreamingStaticProvider {
    async fn complete(&self, _prompt: &str) -> anyhow::Result<agentzero_core::ChatResult> {
        Ok(agentzero_core::ChatResult {
            output_text: self.response.clone(),
            ..Default::default()
        })
    }

    async fn complete_streaming(
        &self,
        _prompt: &str,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<agentzero_core::ChatResult> {
        for ch in self.response.chars() {
            let _ = sender.send(StreamChunk {
                delta: ch.to_string(),
                done: false,
                tool_call_delta: None,
            });
        }
        let _ = sender.send(StreamChunk {
            delta: String::new(),
            done: true,
            tool_call_delta: None,
        });
        Ok(agentzero_core::ChatResult {
            output_text: self.response.clone(),
            ..Default::default()
        })
    }
}

// --- Integration Tests ---

#[tokio::test]
async fn run_agent_once_with_mock_provider() {
    let execution = static_execution("mock response");
    let output = run_agent_with_runtime(execution, PathBuf::from("/tmp"), "hello".to_string())
        .await
        .expect("runtime execution should succeed");
    assert_eq!(output.response_text, "mock response");
    assert_eq!(output.metrics_snapshot["counters"]["requests_total"], 1);
}

#[tokio::test]
async fn run_agent_streaming_delivers_chunks() {
    let execution = streaming_execution("Hi!");
    let (mut rx, handle) =
        run_agent_streaming(execution, PathBuf::from("/tmp"), "hello".to_string());

    let mut deltas = Vec::new();
    while let Some(chunk) = rx.recv().await {
        deltas.push(chunk);
    }
    handle
        .await
        .expect("task should not panic")
        .expect("should succeed");

    // Should have at least content chunks + done sentinel
    assert!(deltas.len() >= 2, "expected content + done chunks");
    assert!(deltas.last().unwrap().done, "last chunk should be done");

    // Accumulated text should match
    let text: String = deltas
        .iter()
        .filter(|c| !c.done)
        .map(|c| c.delta.as_str())
        .collect();
    assert_eq!(text, "Hi!");
}

#[tokio::test]
async fn run_agent_once_records_audit_events() {
    let audit = TestAuditSink::default();
    let execution = RuntimeExecution {
        config: AgentConfig::default(),
        provider: Box::new(StaticProvider {
            output_text: "audited".to_string(),
        }),
        memory: Box::new(TestMemoryStore::default()),
        tools: vec![],
        audit_sink: Some(Box::new(audit.clone())),
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
    };

    let output = run_agent_with_runtime(execution, PathBuf::from("/tmp"), "test".to_string())
        .await
        .expect("should succeed");
    assert_eq!(output.response_text, "audited");

    let events = audit.events();
    // The agent loop records at least request/response audit events
    assert!(
        !events.is_empty(),
        "audit sink should have received at least one event"
    );
}

#[tokio::test]
async fn run_agent_once_provider_error_propagates() {
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
    };

    let err = run_agent_with_runtime(execution, PathBuf::from("/tmp"), "hello".to_string())
        .await
        .expect_err("failing provider should propagate error");
    assert!(err.to_string().contains("testkit provider failure"));
}

#[tokio::test]
async fn run_agent_streaming_handle_resolves_to_output() {
    let execution = streaming_execution("result text");
    let (mut rx, handle) = run_agent_streaming(execution, PathBuf::from("/tmp"), "hi".to_string());

    // Drain receiver
    while rx.recv().await.is_some() {}

    let output = handle
        .await
        .expect("task should not panic")
        .expect("should succeed");
    assert_eq!(output.response_text, "result text");
    assert_eq!(output.metrics_snapshot["counters"]["requests_total"], 1);
}

#[test]
fn default_tools_read_file_present() {
    let policy = ToolSecurityPolicy::default_for_workspace(
        std::env::current_dir().expect("cwd should be readable"),
    );
    let tools = default_tools(&policy, None, None).expect("default tools should build");
    let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
    assert!(
        names.contains(&"read_file"),
        "read_file should always be present in default tools"
    );
}

#[test]
fn default_tools_write_file_absent_by_default() {
    let policy = ToolSecurityPolicy::default_for_workspace(
        std::env::current_dir().expect("cwd should be readable"),
    );
    // By default, write_file is disabled
    assert!(!policy.enable_write_file);
    let tools = default_tools(&policy, None, None).expect("default tools should build");
    let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
    assert!(
        !names.contains(&"write_file"),
        "write_file should NOT be present when disabled"
    );
    assert!(
        !names.contains(&"apply_patch"),
        "apply_patch should NOT be present when write_file is disabled"
    );
    assert!(
        !names.contains(&"file_edit"),
        "file_edit should NOT be present when write_file is disabled"
    );
}

#[test]
fn default_tools_all_have_schemas() {
    let policy = ToolSecurityPolicy::default_for_workspace(
        std::env::current_dir().expect("cwd should be readable"),
    );
    let tools = default_tools(&policy, None, None).expect("default tools should build");

    let mut missing = Vec::new();
    for tool in &tools {
        if tool.input_schema().is_none() {
            missing.push(tool.name());
        }
    }
    assert!(
        missing.is_empty(),
        "the following tools are missing input_schema(): {:?}",
        missing
    );
}

// --- Full-loop structured tool use test ---

/// A tool that has an `input_schema()` so structured tool use activates,
/// and echoes back the `message` field from its JSON input.
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
        // Input arrives as JSON string for structured tool use.
        let v: serde_json::Value =
            serde_json::from_str(input).unwrap_or(serde_json::Value::String(input.to_string()));
        let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or(input);
        Ok(ToolResult {
            output: format!("echoed:{msg}"),
        })
    }
}

/// A provider that scripts a tool-call round-trip:
/// - First `complete_with_tools` call returns a ToolUseRequest for the "echo" tool.
/// - Second call (after tool result is appended) returns a final text response
///   that includes the tool result.
struct ScriptedToolProvider {
    call_count: Arc<Mutex<usize>>,
}

impl ScriptedToolProvider {
    fn new() -> Self {
        Self {
            call_count: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl agentzero_core::Provider for ScriptedToolProvider {
    async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
        Ok(ChatResult::default())
    }

    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        _tools: &[ToolDefinition],
        _reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        let mut count = self.call_count.lock().expect("lock poisoned");
        *count += 1;
        let call_num = *count;
        drop(count);

        if call_num == 1 {
            // First call: return a tool use request.
            Ok(ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    input: json!({"message": "hello from tool"}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            })
        } else {
            // Second call: the conversation should contain the tool result.
            // Extract it and return a final response referencing it.
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

#[tokio::test]
async fn full_loop_agent_with_tool_call_round_trip() {
    let execution = RuntimeExecution {
        config: AgentConfig {
            model_supports_tool_use: true,
            // Short timeout for test.
            request_timeout_ms: 10_000,
            ..Default::default()
        },
        provider: Box::new(ScriptedToolProvider::new()),
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
    };

    let output = run_agent_with_runtime(
        execution,
        PathBuf::from("/tmp"),
        "Please echo hello".to_string(),
    )
    .await
    .expect("full-loop agent should succeed");

    // The provider's second call sees "echoed:hello from tool" from SchemaEchoTool,
    // then wraps it into the final response.
    assert_eq!(output.response_text, "Tool said: echoed:hello from tool");
}
