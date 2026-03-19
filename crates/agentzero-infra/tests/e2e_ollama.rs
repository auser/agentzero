//! End-to-end tests against a real Ollama LLM server.
//!
//! All tests are `#[ignore]` so they don't run during normal CI (`just test`).
//! Run them with: `just test-ollama` (requires Ollama running locally).
//!
//! Model: llama3.2:latest (2GB) — smallest model with tool-calling support.

use agentzero_core::{ConversationMessage, Provider, ReasoningConfig, StreamChunk, ToolDefinition};
use agentzero_orchestrator::agent_router::{AgentDescriptor, AgentRouter};
use agentzero_providers::OpenAiCompatibleProvider;
use serde_json::json;

const OLLAMA_MODEL: &str = "llama3.2:latest";
const OLLAMA_BASE_URL: &str = "http://localhost:11434";

fn ollama_provider() -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::new(
        OLLAMA_BASE_URL.to_string(),
        String::new(),
        OLLAMA_MODEL.to_string(),
    )
}

/// Check if Ollama is reachable. Returns false (with diagnostic) if not.
async fn require_ollama() -> bool {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .expect("build reqwest client");

    match client
        .get(format!("{OLLAMA_BASE_URL}/api/tags"))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => true,
        Ok(resp) => {
            eprintln!(
                "Ollama responded with status {}, skipping e2e tests",
                resp.status()
            );
            false
        }
        Err(e) => {
            eprintln!("Ollama not reachable at {OLLAMA_BASE_URL}: {e}, skipping e2e tests");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn ollama_basic_completion() {
    if !require_ollama().await {
        return;
    }

    let provider = ollama_provider();
    let result = provider
        .complete("What is 2+2? Reply with ONLY the number, nothing else.")
        .await
        .expect("completion should succeed");

    assert!(
        result.output_text.contains('4'),
        "expected response to contain '4', got: {}",
        result.output_text
    );
}

#[tokio::test]
#[ignore]
async fn ollama_streaming_completion() {
    if !require_ollama().await {
        return;
    }

    let provider = ollama_provider();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamChunk>();

    let result = provider
        .complete_streaming("Count from 1 to 5. Just the numbers, one per line.", tx)
        .await
        .expect("streaming completion should succeed");

    // Collect all chunks
    let mut chunks = Vec::new();
    while let Ok(chunk) = rx.try_recv() {
        chunks.push(chunk);
    }

    // Should have received multiple incremental chunks
    let non_empty_chunks: Vec<_> = chunks
        .iter()
        .filter(|c| !c.delta.is_empty() && !c.done)
        .collect();
    assert!(
        non_empty_chunks.len() >= 2,
        "expected at least 2 non-empty streaming chunks, got {}",
        non_empty_chunks.len()
    );

    // Final accumulated text should contain the digits 1-5
    let text = &result.output_text;
    for digit in ["1", "2", "3", "4", "5"] {
        assert!(
            text.contains(digit),
            "expected streaming result to contain '{digit}', got: {text}"
        );
    }
}

#[tokio::test]
#[ignore]
async fn ollama_multi_turn_conversation() {
    if !require_ollama().await {
        return;
    }

    let provider = ollama_provider();
    let reasoning = ReasoningConfig::default();

    // Turn 1: establish a fact
    let messages_turn1 = vec![
        ConversationMessage::System {
            content: "You are a helpful assistant with perfect memory.".to_string(),
        },
        ConversationMessage::User {
            parts: vec![],
            content: "My favorite color is blue. Just acknowledge this.".to_string(),
        },
    ];

    let turn1 = provider
        .complete_with_tools(&messages_turn1, &[], &reasoning)
        .await
        .expect("turn 1 should succeed");

    // Turn 2: recall the fact
    let mut messages_turn2 = messages_turn1;
    messages_turn2.push(ConversationMessage::Assistant {
        content: Some(turn1.output_text),
        tool_calls: vec![],
    });
    messages_turn2.push(ConversationMessage::User {
        parts: vec![],
        content: "What is my favorite color? Reply with just the color name.".to_string(),
    });

    let turn2 = provider
        .complete_with_tools(&messages_turn2, &[], &reasoning)
        .await
        .expect("turn 2 should succeed");

    let lower = turn2.output_text.to_lowercase();
    assert!(
        lower.contains("blue"),
        "expected turn 2 to recall 'blue', got: {}",
        turn2.output_text
    );
}

#[tokio::test]
#[ignore]
async fn ollama_tool_use() {
    if !require_ollama().await {
        return;
    }

    let provider = ollama_provider();
    let reasoning = ReasoningConfig::default();

    let tools = vec![ToolDefinition {
        name: "echo".to_string(),
        description: "Echo back the message field. You MUST use this tool when asked to echo."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "required": ["message"],
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message to echo back"
                }
            }
        }),
    }];

    let messages = vec![
        ConversationMessage::System {
            content: "You have access to an echo tool. When asked to echo something, you MUST call the echo tool with the message.".to_string(),
        },
        ConversationMessage::User {
            parts: vec![],
            content: "Please use the echo tool to echo the word 'hello'.".to_string(),
        },
    ];

    let result = provider
        .complete_with_tools(&messages, &tools, &reasoning)
        .await
        .expect("tool use completion should succeed");

    // llama3.2 supports tool calling — verify it made a tool call
    assert!(
        !result.tool_calls.is_empty(),
        "expected at least one tool call, got none. Response text: {}",
        result.output_text
    );

    let call = &result.tool_calls[0];
    assert_eq!(call.name, "echo", "expected tool call to 'echo'");
    let args = &call.input;
    let msg = args.get("message").and_then(|m| m.as_str()).unwrap_or("");
    assert!(
        msg.to_lowercase().contains("hello"),
        "expected echo tool to be called with 'hello', got args: {args}"
    );
}

#[tokio::test]
#[ignore]
async fn ollama_router_classification() {
    if !require_ollama().await {
        return;
    }

    let provider = ollama_provider();
    let router = AgentRouter::new(Some(Box::new(provider)), true);

    let agents = vec![
        AgentDescriptor {
            id: "image-gen".to_string(),
            name: "Image Generator".to_string(),
            description: "Creates images, illustrations, and visual art".to_string(),
            keywords: vec!["image".into(), "draw".into(), "illustration".into()],
            subscribes_to: vec![],
            produces: vec![],
            privacy_boundary: "any".to_string(),
        },
        AgentDescriptor {
            id: "code-review".to_string(),
            name: "Code Reviewer".to_string(),
            description: "Reviews code, pull requests, and identifies bugs".to_string(),
            keywords: vec!["code".into(), "review".into(), "pr".into(), "bug".into()],
            subscribes_to: vec![],
            produces: vec![],
            privacy_boundary: "any".to_string(),
        },
    ];

    let result = router
        .route(
            "Please review my pull request for bugs and style issues",
            &agents,
        )
        .await
        .expect("routing should succeed");

    assert_eq!(
        result,
        Some("code-review".to_string()),
        "expected router to pick 'code-review' for a PR review request, got: {result:?}"
    );
}
