//! End-to-end tests using a real Ollama instance.
//!
//! These tests are `#[ignore]` by default — they require a running Ollama
//! instance with `llama3.2:latest` pulled. Run with:
//!
//!   cargo nextest run --run-ignored only -E 'test(ollama)' --test-threads 1
//!
//! Or via justfile:
//!
//!   just test-ollama

use agentzero_core::Provider;
use agentzero_providers::OpenAiCompatibleProvider;

/// Check if Ollama is available at localhost:11434.
async fn require_ollama() -> bool {
    let client = reqwest::Client::new();
    match client
        .get("http://localhost:11434/api/tags")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

fn ollama_provider() -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::new(
        "http://localhost:11434".to_string(),
        String::new(),
        "llama3.2:latest".to_string(),
    )
}

#[tokio::test]
#[ignore]
async fn ollama_basic_completion() {
    if !require_ollama().await {
        eprintln!("Ollama not available, skipping");
        return;
    }

    let provider = ollama_provider();
    let result = provider.complete("Say hello in exactly 3 words.").await;
    assert!(result.is_ok(), "completion should succeed: {result:?}");
    let response = result.expect("completion");
    assert!(
        !response.output_text.is_empty(),
        "response should have content"
    );
}

#[tokio::test]
#[ignore]
async fn ollama_streaming_completion() {
    if !require_ollama().await {
        eprintln!("Ollama not available, skipping");
        return;
    }

    let provider = ollama_provider();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let result = provider.complete_streaming("Count from 1 to 5.", tx).await;
    assert!(result.is_ok(), "streaming should succeed: {result:?}");

    let mut chunks = Vec::new();
    while let Ok(chunk) = rx.try_recv() {
        chunks.push(chunk);
    }
    // At minimum the default impl sends one chunk
    assert!(
        !chunks.is_empty(),
        "should receive at least one streaming chunk"
    );
}

#[tokio::test]
#[ignore]
async fn ollama_multi_turn_conversation() {
    if !require_ollama().await {
        eprintln!("Ollama not available, skipping");
        return;
    }

    let provider = ollama_provider();

    // Multi-turn is simulated by concatenating context in the prompt.
    let prompt = "Context: The user's name is Alice.\n\nUser: What is my name?\nAssistant:";
    let result = provider.complete(prompt).await.expect("should succeed");

    assert!(
        result.output_text.to_lowercase().contains("alice"),
        "should reference Alice: {}",
        result.output_text
    );
}
