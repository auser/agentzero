//! Tool for switching the active model/provider mid-session.
//!
//! Uses the shared `ProviderPool` from `agentzero-providers` to switch
//! between pre-built providers without restarting the agent.

use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_providers::ProviderPool;
use async_trait::async_trait;
use std::sync::Arc;

/// Tool that lets the agent (or user via chat) switch models mid-session.
pub struct ModelSwitchTool {
    pool: Arc<ProviderPool>,
}

impl ModelSwitchTool {
    pub fn new(pool: Arc<ProviderPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Tool for ModelSwitchTool {
    fn name(&self) -> &'static str {
        "model_switch"
    }

    fn description(&self) -> &'static str {
        "Switch the active LLM model/provider mid-session. Input: JSON with a \
         \"model\" field containing the provider key (e.g. \"fast\", \"reasoning\", \
         \"anthropic:claude-3-opus\"). Use \"list\" as input to see available models."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "model": {
                    "type": "string",
                    "description": "Provider key to switch to, or 'list' to show available models"
                }
            },
            "required": ["model"]
        }))
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        // Parse JSON input or treat raw string as model key.
        let model_key = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(input) {
            parsed
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or(input.trim())
                .to_string()
        } else {
            input.trim().to_string()
        };

        if model_key == "list" {
            let available = self.pool.list_available();
            let active = self.pool.active_key().await;
            let listing: Vec<String> = available
                .iter()
                .map(|k| {
                    if k == &active {
                        format!("* {k} (active)")
                    } else {
                        format!("  {k}")
                    }
                })
                .collect();
            return Ok(ToolResult {
                output: format!("Available models:\n{}", listing.join("\n")),
            });
        }

        let previous = self.pool.active_key().await;
        self.pool.switch_to(&model_key).await?;

        Ok(ToolResult {
            output: format!("Switched model from `{previous}` to `{model_key}`"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::{ChatResult, Provider};
    use std::collections::HashMap;

    struct MockProvider {
        _name: String,
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            Ok(ChatResult::default())
        }
    }

    fn make_pool() -> Arc<ProviderPool> {
        let mut providers: HashMap<String, Arc<dyn Provider>> = HashMap::new();
        providers.insert(
            "fast".into(),
            Arc::new(MockProvider {
                _name: "fast".into(),
            }),
        );
        providers.insert(
            "reasoning".into(),
            Arc::new(MockProvider {
                _name: "reasoning".into(),
            }),
        );
        Arc::new(ProviderPool::new(providers, "fast".into()))
    }

    #[tokio::test]
    async fn switch_via_json_input() {
        let pool = make_pool();
        let tool = ModelSwitchTool::new(Arc::clone(&pool));
        let ctx = ToolContext::new("/tmp/test".to_string());

        let result = tool
            .execute(r#"{"model": "reasoning"}"#, &ctx)
            .await
            .expect("execute");
        assert!(result.output.contains("reasoning"));
        assert_eq!(pool.active_key().await, "reasoning");
    }

    #[tokio::test]
    async fn list_models() {
        let pool = make_pool();
        let tool = ModelSwitchTool::new(pool);
        let ctx = ToolContext::new("/tmp/test".to_string());

        let result = tool
            .execute(r#"{"model": "list"}"#, &ctx)
            .await
            .expect("execute");
        assert!(result.output.contains("fast"));
        assert!(result.output.contains("reasoning"));
        assert!(result.output.contains("(active)"));
    }

    #[tokio::test]
    async fn switch_to_unknown_fails() {
        let pool = make_pool();
        let tool = ModelSwitchTool::new(pool);
        let ctx = ToolContext::new("/tmp/test".to_string());

        let err = tool
            .execute(r#"{"model": "nonexistent"}"#, &ctx)
            .await
            .expect_err("should fail");
        assert!(err.to_string().contains("not found in pool"));
    }
}
