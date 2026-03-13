use agentzero_core::{AgentConfig, Tool};
use agentzero_infra::runtime::{run_agent_with_runtime, RuntimeExecution};
use agentzero_testkit::{EchoTool, FailingProvider, StaticProvider, TestMemoryStore};
use std::path::PathBuf;

#[tokio::test]
async fn runtime_exec_uses_testkit_components_success_path() {
    let execution = RuntimeExecution {
        config: AgentConfig::default(),
        provider: Box::new(StaticProvider {
            output_text: "ok".to_string(),
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
    };

    let output = run_agent_with_runtime(execution, PathBuf::from("."), "hello".to_string())
        .await
        .expect("runtime execution should succeed");
    assert_eq!(output.response_text, "ok");
    assert_eq!(output.metrics_snapshot["counters"]["requests_total"], 1);
}

#[tokio::test]
async fn runtime_exec_uses_testkit_components_negative_path() {
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
    };

    let err = run_agent_with_runtime(execution, PathBuf::from("."), "hello".to_string())
        .await
        .expect_err("runtime execution should fail");
    assert!(err.to_string().contains("testkit provider failure"));
}
