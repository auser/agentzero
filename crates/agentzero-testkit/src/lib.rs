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
