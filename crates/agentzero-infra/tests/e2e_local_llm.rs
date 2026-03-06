//! End-to-end tests that exercise the full agent loop against a real local LLM.
//!
//! These tests are `#[ignore]`d by default and only run when:
//! - A local LLM server is running (e.g. `ollama serve`)
//! - A small model is available (e.g. `ollama pull tinyllama`)
//! - Tests are invoked with `cargo test -- --ignored`
//!
//! Environment variables:
//! - `LOCAL_LLM_URL` — base URL (default: `http://localhost:11434`)
//! - `LOCAL_LLM_MODEL` — model name (default: `tinyllama`)

use agentzero_core::AgentConfig;
use agentzero_infra::runtime::{run_agent_with_runtime, RuntimeExecution};
use agentzero_testkit::{local_llm_available, local_llm_provider, EchoTool, TestMemoryStore};
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[tokio::test]
#[ignore]
async fn e2e_basic_completion() {
    if !local_llm_available().await {
        eprintln!("skipping: local LLM not available");
        return;
    }

    let execution = RuntimeExecution {
        config: AgentConfig {
            max_tool_iterations: 0,
            ..Default::default()
        },
        provider: local_llm_provider(),
        memory: Box::new(TestMemoryStore::default()),
        tools: vec![],
        audit_sink: None,
        hook_sink: None,
    };

    let output = run_agent_with_runtime(execution, workspace_root(), "Say hello.".to_string())
        .await
        .expect("agent should produce a response");

    assert!(
        !output.response_text.is_empty(),
        "expected non-empty response from local LLM"
    );
}

#[tokio::test]
#[ignore]
async fn e2e_agent_with_echo_tool() {
    if !local_llm_available().await {
        eprintln!("skipping: local LLM not available");
        return;
    }

    let execution = RuntimeExecution {
        config: AgentConfig {
            max_tool_iterations: 3,
            ..Default::default()
        },
        provider: local_llm_provider(),
        memory: Box::new(TestMemoryStore::default()),
        tools: vec![Box::new(EchoTool)],
        audit_sink: None,
        hook_sink: None,
    };

    let output = run_agent_with_runtime(
        execution,
        workspace_root(),
        "Use the echo tool with the input 'hello world'.".to_string(),
    )
    .await
    .expect("agent should produce a response");

    // The response should exist — whether or not the LLM actually used the tool
    // depends on the model, but the loop should complete without error.
    assert!(
        !output.response_text.is_empty(),
        "expected non-empty response"
    );
}

#[tokio::test]
#[ignore]
async fn e2e_multi_turn_memory() {
    if !local_llm_available().await {
        eprintln!("skipping: local LLM not available");
        return;
    }

    let memory = TestMemoryStore::default();

    // Turn 1: establish a fact.
    let execution1 = RuntimeExecution {
        config: AgentConfig {
            max_tool_iterations: 0,
            ..Default::default()
        },
        provider: local_llm_provider(),
        memory: Box::new(memory.clone()),
        tools: vec![],
        audit_sink: None,
        hook_sink: None,
    };

    let output1 = run_agent_with_runtime(
        execution1,
        workspace_root(),
        "My name is TestUser42. Remember that.".to_string(),
    )
    .await
    .expect("turn 1 should succeed");

    assert!(!output1.response_text.is_empty());

    // Turn 2: ask about the fact — memory should carry context.
    let execution2 = RuntimeExecution {
        config: AgentConfig {
            max_tool_iterations: 0,
            ..Default::default()
        },
        provider: local_llm_provider(),
        memory: Box::new(memory.clone()),
        tools: vec![],
        audit_sink: None,
        hook_sink: None,
    };

    let output2 =
        run_agent_with_runtime(execution2, workspace_root(), "What is my name?".to_string())
            .await
            .expect("turn 2 should succeed");

    assert!(
        !output2.response_text.is_empty(),
        "expected non-empty response on turn 2"
    );
    // The LLM should reference the name from turn 1 if memory is working.
    // Small models may not always get this right, so we just verify the response exists.
}

#[tokio::test]
#[ignore]
async fn e2e_agent_router_with_real_llm() {
    if !local_llm_available().await {
        eprintln!("skipping: local LLM not available");
        return;
    }

    use agentzero_orchestrator::{AgentDescriptor, AgentRouter};

    let router = AgentRouter::new(Some(local_llm_provider()), true);

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

    // Ask to draw something — should route to image-gen.
    let result = router
        .route(
            "Please draw me a picture of a sunset over mountains",
            &agents,
        )
        .await
        .expect("routing should succeed");

    // Small models may not always classify correctly, but the router
    // should return a valid result (Some agent or None).
    if let Some(ref id) = result {
        assert!(
            agents.iter().any(|a| &a.id == id),
            "router returned unknown agent id: {id}"
        );
    }
}
