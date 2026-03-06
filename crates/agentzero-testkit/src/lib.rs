//! Test utilities for AgentZero.
//!
//! Provides fake implementations of `Provider`, `MemoryStore`, and `Tool`
//! for use in unit and integration tests across the workspace.

use agentzero_core::{
    ChatResult, MemoryEntry, MemoryStore, Provider, Tool, ToolContext, ToolResult,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
pub struct TestMemoryStore {
    entries: Arc<Mutex<Vec<MemoryEntry>>>,
}

#[async_trait]
impl MemoryStore for TestMemoryStore {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        self.entries
            .lock()
            .expect("test memory lock poisoned")
            .push(entry);
        Ok(())
    }

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let entries = self.entries.lock().expect("test memory lock poisoned");
        Ok(entries.iter().rev().take(limit).cloned().collect())
    }
}

impl TestMemoryStore {
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .expect("test memory lock poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone)]
pub struct StaticProvider {
    pub output_text: String,
}

#[async_trait]
impl Provider for StaticProvider {
    async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
        Ok(ChatResult {
            output_text: self.output_text.clone(),
            ..Default::default()
        })
    }
}

pub struct FailingProvider;

#[async_trait]
impl Provider for FailingProvider {
    async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
        Err(anyhow::anyhow!("testkit provider failure"))
    }
}

// ─── Local LLM Helpers ───────────────────────────────────────────────────────

const DEFAULT_LOCAL_LLM_URL: &str = "http://localhost:11434";
const DEFAULT_LOCAL_LLM_MODEL: &str = "tinyllama";

/// Create an `OpenAiCompatibleProvider` pointing at a local LLM server.
///
/// Reads `LOCAL_LLM_URL` (default `http://localhost:11434`) and
/// `LOCAL_LLM_MODEL` (default `tinyllama`) from the environment.
pub fn local_llm_provider() -> Box<dyn Provider> {
    let url = std::env::var("LOCAL_LLM_URL").unwrap_or_else(|_| DEFAULT_LOCAL_LLM_URL.to_string());
    let model =
        std::env::var("LOCAL_LLM_MODEL").unwrap_or_else(|_| DEFAULT_LOCAL_LLM_MODEL.to_string());
    Box::new(agentzero_providers::OpenAiCompatibleProvider::new(
        url,
        String::new(), // no API key for local LLMs
        model,
    ))
}

/// Returns `true` if a local LLM server is reachable. Intended for use at the
/// top of `#[ignore]` tests so they skip gracefully when no server is running.
pub async fn local_llm_available() -> bool {
    let url = std::env::var("LOCAL_LLM_URL").unwrap_or_else(|_| DEFAULT_LOCAL_LLM_URL.to_string());
    let health_url = format!("{url}/v1/models");
    match reqwest::Client::new()
        .get(&health_url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Wait for a local LLM server to become available, polling every 500ms.
/// Returns `false` if the server is not ready within `timeout`.
pub async fn wait_for_server(timeout: std::time::Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if local_llm_available().await {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

// ─── Echo Tool ───────────────────────────────────────────────────────────────

pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        Ok(ToolResult {
            output: format!("echoed:{input}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{FailingProvider, StaticProvider, TestMemoryStore};
    use agentzero_core::{MemoryEntry, MemoryStore, Provider};

    #[tokio::test]
    async fn test_memory_store_round_trip_success() {
        let store = TestMemoryStore::default();
        MemoryStore::append(
            &store,
            MemoryEntry {
                role: "user".to_string(),
                content: "hello".to_string(),
                ..Default::default()
            },
        )
        .await
        .expect("append should succeed");
        assert_eq!(store.len(), 1);
    }

    #[tokio::test]
    async fn failing_provider_returns_expected_error() {
        let provider = FailingProvider;
        let err = provider
            .complete("hello")
            .await
            .expect_err("provider should fail");
        assert!(err.to_string().contains("testkit provider failure"));
    }

    #[tokio::test]
    async fn static_provider_returns_fixed_output() {
        let provider = StaticProvider {
            output_text: "ok".to_string(),
        };
        let result = provider
            .complete("ignored")
            .await
            .expect("provider should succeed");
        assert_eq!(result.output_text, "ok");
    }
}
