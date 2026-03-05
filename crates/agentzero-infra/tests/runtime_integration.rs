use agentzero_core::{AgentConfig, AuditEvent, AuditSink, StreamChunk, Tool};
use agentzero_infra::runtime::{run_agent_streaming, run_agent_with_runtime, RuntimeExecution};
use agentzero_infra::tools::{default_tools, ToolSecurityPolicy};
use agentzero_testkit::{EchoTool, FailingProvider, StaticProvider, TestMemoryStore};
use async_trait::async_trait;
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
