//! Semantic recall tool — retrieves memory entries ranked by cosine similarity.
//!
//! Unlike `MemoryRecallTool` (which uses JSON KV storage), this tool queries
//! the `MemoryStore` trait backend (SQLite) using vector embeddings.

use agentzero_core::embedding::EmbeddingProvider;
use agentzero_core::{MemoryStore, Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
struct Input {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct SemanticRecallSchema {
    /// The text to search for semantically similar entries
    query: String,
    /// Maximum results to return (default: 5)
    #[serde(default)]
    limit: Option<i64>,
}

fn default_limit() -> usize {
    5
}

/// Tool that retrieves memory entries ranked by semantic similarity.
///
/// Requires both a `MemoryStore` (for storage) and an `EmbeddingProvider`
/// (for embedding the query text). Returns entries sorted by cosine
/// similarity to the query.
#[tool(
    name = "semantic_recall",
    description = "Retrieve memory entries ranked by semantic similarity to a query. Uses vector embeddings for meaning-based search rather than keyword matching."
)]
pub struct SemanticRecallTool {
    store: Arc<dyn MemoryStore>,
    embedder: Arc<dyn EmbeddingProvider>,
}

impl SemanticRecallTool {
    pub fn new(store: Arc<dyn MemoryStore>, embedder: Arc<dyn EmbeddingProvider>) -> Self {
        Self { store, embedder }
    }
}

#[async_trait]
impl Tool for SemanticRecallTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(SemanticRecallSchema::schema())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: Input = serde_json::from_str(input).map_err(|e| {
            anyhow::anyhow!("semantic_recall expects JSON: {{\"query\": \"...\"}}: {e}")
        })?;

        let query_embedding = self
            .embedder
            .embed(&req.query)
            .await
            .map_err(|e| anyhow::anyhow!("failed to embed query: {e}"))?;

        let entries = self
            .store
            .semantic_recall(&query_embedding, req.limit)
            .await
            .map_err(|e| anyhow::anyhow!("semantic recall failed: {e}"))?;

        if entries.is_empty() {
            return Ok(ToolResult {
                output: "no semantically similar entries found".to_string(),
            });
        }

        let results: Vec<String> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let ts = entry.created_at.as_deref().unwrap_or("unknown");
                format!("{}. [{}] {}: {}", i + 1, ts, entry.role, entry.content)
            })
            .collect();

        Ok(ToolResult {
            output: results.join("\n"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::embedding::EmbeddingProvider;
    use agentzero_core::MemoryEntry;
    use std::sync::Mutex;

    /// Mock embedding provider that returns fixed embeddings based on content.
    struct MockEmbedder;

    #[async_trait]
    impl EmbeddingProvider for MockEmbedder {
        async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
            // Simple deterministic embedding: hash the text into a 4-dim vector.
            let hash = text.bytes().fold(0u32, |acc, b| acc.wrapping_add(b as u32));
            let f = hash as f32;
            Ok(vec![f.sin(), f.cos(), (f * 0.5).sin(), (f * 0.5).cos()])
        }

        fn dimensions(&self) -> usize {
            4
        }
    }

    /// In-memory store that supports embeddings.
    struct MockStore {
        entries: Mutex<Vec<MemoryEntry>>,
    }

    impl MockStore {
        fn new() -> Self {
            Self {
                entries: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl MemoryStore for MockStore {
        async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
            self.entries.lock().expect("lock").push(entry);
            Ok(())
        }

        async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
            let entries = self.entries.lock().expect("lock");
            Ok(entries.iter().rev().take(limit).cloned().collect())
        }

        async fn append_with_embedding(
            &self,
            mut entry: MemoryEntry,
            embedding: Vec<f32>,
        ) -> anyhow::Result<()> {
            entry.embedding = Some(embedding);
            self.entries.lock().expect("lock").push(entry);
            Ok(())
        }
    }

    fn test_ctx() -> ToolContext {
        ToolContext::new("/tmp".to_string())
    }

    #[tokio::test]
    async fn semantic_recall_returns_ranked_results() {
        let store = Arc::new(MockStore::new());
        let embedder = Arc::new(MockEmbedder);

        // Store entries with embeddings
        for (role, content) in &[
            ("user", "What is the weather today?"),
            ("assistant", "The weather is sunny and warm."),
            ("user", "Tell me about machine learning."),
        ] {
            let entry = MemoryEntry {
                role: role.to_string(),
                content: content.to_string(),
                ..Default::default()
            };
            let emb = embedder.embed(content).await.expect("embed");
            store
                .append_with_embedding(entry, emb)
                .await
                .expect("append");
        }

        let tool = SemanticRecallTool::new(store, embedder);
        let result = tool
            .execute(r#"{"query": "weather forecast"}"#, &test_ctx())
            .await
            .expect("should succeed");

        assert!(
            !result.output.contains("no semantically similar"),
            "should find entries"
        );
        // Should return numbered results
        assert!(result.output.contains("1."), "should have numbered results");
    }

    #[tokio::test]
    async fn semantic_recall_empty_store_returns_message() {
        let store = Arc::new(MockStore::new());
        let embedder = Arc::new(MockEmbedder);
        let tool = SemanticRecallTool::new(store, embedder);

        let result = tool
            .execute(r#"{"query": "anything"}"#, &test_ctx())
            .await
            .expect("should succeed");

        assert_eq!(result.output, "no semantically similar entries found");
    }

    #[tokio::test]
    async fn semantic_recall_respects_limit() {
        let store = Arc::new(MockStore::new());
        let embedder = Arc::new(MockEmbedder);

        for i in 0..10 {
            let entry = MemoryEntry {
                role: "user".to_string(),
                content: format!("entry number {i}"),
                ..Default::default()
            };
            let emb = embedder.embed(&entry.content).await.expect("embed");
            store
                .append_with_embedding(entry, emb)
                .await
                .expect("append");
        }

        let tool = SemanticRecallTool::new(store, embedder);
        let result = tool
            .execute(r#"{"query": "entry", "limit": 3}"#, &test_ctx())
            .await
            .expect("should succeed");

        let lines: Vec<&str> = result.output.lines().collect();
        assert_eq!(lines.len(), 3, "should return exactly 3 results");
    }

    #[tokio::test]
    async fn semantic_recall_invalid_input_returns_error() {
        let store = Arc::new(MockStore::new());
        let embedder = Arc::new(MockEmbedder);
        let tool = SemanticRecallTool::new(store, embedder);

        let err = tool
            .execute("not json", &test_ctx())
            .await
            .expect_err("should fail on invalid JSON");
        assert!(err.to_string().contains("semantic_recall expects JSON"));
    }
}
