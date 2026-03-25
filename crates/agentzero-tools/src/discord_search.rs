use agentzero_core::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct Input {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    20
}

/// Search tool for Discord message history.
///
/// When constructed with a `DiscordHistoryStore`, queries the SQLite database
/// for matching messages. Without a store, returns a helpful message.
#[derive(Default)]
pub struct DiscordSearchTool {
    store: Option<Arc<agentzero_storage::discord::DiscordHistoryStore>>,
}

impl DiscordSearchTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_store(store: Arc<agentzero_storage::discord::DiscordHistoryStore>) -> Self {
        Self { store: Some(store) }
    }
}

#[async_trait]
impl Tool for DiscordSearchTool {
    fn name(&self) -> &'static str {
        "discord_search"
    }

    fn description(&self) -> &'static str {
        "Search Discord message history for keywords. Returns matching messages with sender names and timestamps."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query keywords" },
                "limit": { "type": "integer", "description": "Maximum results (default: 20)", "default": 20 }
            },
            "required": ["query"]
        }))
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: Input = serde_json::from_str(input).map_err(|e| {
            anyhow::anyhow!("discord_search expects JSON: {{\"query\": \"...\"}}: {e}")
        })?;

        if parsed.query.trim().is_empty() {
            return Err(anyhow::anyhow!("query must not be empty"));
        }

        let store = match &self.store {
            Some(s) => s,
            None => {
                return Ok(ToolResult {
                    output: format!(
                        "Discord search for '{}' (limit: {}) — history database not connected. \
                         Enable the discord-history channel to start recording messages.",
                        parsed.query, parsed.limit
                    ),
                });
            }
        };

        let results = store.search(&parsed.query, parsed.limit)?;

        if results.is_empty() {
            return Ok(ToolResult {
                output: format!("No Discord messages found matching '{}'.", parsed.query),
            });
        }

        let entries: Vec<serde_json::Value> = results
            .iter()
            .map(|msg| {
                json!({
                    "author": msg.author_name,
                    "content": msg.content,
                    "channel_id": msg.channel_id,
                    "timestamp": msg.created_at,
                })
            })
            .collect();

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&json!({
                "query": parsed.query,
                "count": entries.len(),
                "messages": entries,
            }))
            .unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_search_has_schema() {
        let tool = DiscordSearchTool::new();
        let schema = tool.input_schema();
        assert!(schema.is_some());
        let schema = schema.expect("schema");
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["query"].is_object());
    }

    #[tokio::test]
    async fn discord_search_empty_query_error() {
        let tool = DiscordSearchTool::new();
        let ctx = ToolContext::new(".".to_string());
        let result = tool.execute(r#"{"query": "  "}"#, &ctx).await;
        assert!(result.is_err());
        assert!(result
            .expect_err("error")
            .to_string()
            .contains("query must not be empty"));
    }

    #[tokio::test]
    async fn discord_search_invalid_json_error() {
        let tool = DiscordSearchTool::new();
        let ctx = ToolContext::new(".".to_string());
        let result = tool.execute("not json", &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn discord_search_without_store_returns_message() {
        let tool = DiscordSearchTool::new();
        let ctx = ToolContext::new(".".to_string());
        let result = tool
            .execute(r#"{"query": "hello"}"#, &ctx)
            .await
            .expect("should succeed");
        assert!(result.output.contains("not connected"));
    }

    #[tokio::test]
    async fn discord_search_with_store_returns_results() {
        use agentzero_storage::discord::{DiscordHistoryStore, DiscordMessage};
        use std::time::{SystemTime, UNIX_EPOCH};

        let dir = std::env::temp_dir().join(format!("agentzero-ds-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let store = Arc::new(DiscordHistoryStore::open(&dir.join("discord.db")).expect("open"));
        store
            .insert(&DiscordMessage {
                channel_id: "ch1".to_string(),
                author_id: "u1".to_string(),
                author_name: "Bob".to_string(),
                content: "hello world from discord".to_string(),
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("time")
                    .as_secs(),
            })
            .expect("insert");

        let tool = DiscordSearchTool::with_store(store);
        let ctx = ToolContext::new(".".to_string());
        let result = tool
            .execute(r#"{"query": "hello"}"#, &ctx)
            .await
            .expect("search");
        assert!(result.output.contains("Bob"));
        assert!(result.output.contains("hello world"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn discord_search_no_matches_returns_empty() {
        use agentzero_storage::discord::DiscordHistoryStore;

        let dir = std::env::temp_dir().join(format!("agentzero-ds-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let store = Arc::new(DiscordHistoryStore::open(&dir.join("discord.db")).expect("open"));

        let tool = DiscordSearchTool::with_store(store);
        let ctx = ToolContext::new(".".to_string());
        let result = tool
            .execute(r#"{"query": "nonexistent"}"#, &ctx)
            .await
            .expect("search");
        assert!(result.output.contains("No Discord messages found"));

        std::fs::remove_dir_all(dir).ok();
    }
}
