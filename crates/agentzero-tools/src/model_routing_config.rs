use agentzero_core::routing::ModelRouter;
use agentzero_core::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct Input {
    op: String,
    #[serde(default)]
    hint: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

pub struct ModelRoutingConfigTool {
    router: ModelRouter,
}

impl ModelRoutingConfigTool {
    pub fn new(router: ModelRouter) -> Self {
        Self { router }
    }
}

#[async_trait]
impl Tool for ModelRoutingConfigTool {
    fn name(&self) -> &'static str {
        "model_routing_config"
    }

    fn description(&self) -> &'static str {
        "View or modify the model routing configuration at runtime."
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: Input =
            serde_json::from_str(input).map_err(|e| anyhow::anyhow!("invalid input: {e}"))?;

        let output = match parsed.op.as_str() {
            "list_routes" => {
                let hints: Vec<&str> = self
                    .router
                    .model_routes
                    .iter()
                    .map(|r| r.hint.as_str())
                    .collect();
                json!({ "routes": hints }).to_string()
            }
            "list_embedding_routes" => {
                let hints: Vec<&str> = self
                    .router
                    .embedding_routes
                    .iter()
                    .map(|r| r.hint.as_str())
                    .collect();
                json!({ "embedding_routes": hints }).to_string()
            }
            "resolve_hint" => {
                let hint = parsed
                    .hint
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("resolve_hint requires a `hint` field"))?;
                match self.router.resolve_hint(hint) {
                    Some(route) => json!({
                        "hint": route.matched_hint,
                        "provider": route.provider,
                        "model": route.model,
                        "max_tokens": route.max_tokens,
                    })
                    .to_string(),
                    None => json!({ "error": format!("unknown hint: {hint}") }).to_string(),
                }
            }
            "classify_query" => {
                let query = parsed
                    .query
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("classify_query requires a `query` field"))?;
                match self.router.classify_query(query) {
                    Some(hint) => json!({ "hint": hint }).to_string(),
                    None => json!({ "hint": null }).to_string(),
                }
            }
            "route_query" => {
                let query = parsed
                    .query
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("route_query requires a `query` field"))?;
                match self.router.route_query(query) {
                    Some(route) => json!({
                        "hint": route.matched_hint,
                        "provider": route.provider,
                        "model": route.model,
                        "max_tokens": route.max_tokens,
                    })
                    .to_string(),
                    None => json!({ "route": null }).to_string(),
                }
            }
            other => json!({ "error": format!("unknown op: {other}") }).to_string(),
        };

        Ok(ToolResult { output })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::routing::{ClassificationRule, EmbeddingRoute, ModelRoute, ModelRouter};

    fn test_router() -> ModelRouter {
        ModelRouter {
            model_routes: vec![
                ModelRoute {
                    hint: "fast".into(),
                    provider: "openai".into(),
                    model: "gpt-4o-mini".into(),
                    max_tokens: Some(4096),
                    api_key: None,
                    transport: None,
                },
                ModelRoute {
                    hint: "reasoning".into(),
                    provider: "openai".into(),
                    model: "o1".into(),
                    max_tokens: Some(8192),
                    api_key: None,
                    transport: None,
                },
            ],
            embedding_routes: vec![EmbeddingRoute {
                hint: "default".into(),
                provider: "openai".into(),
                model: "text-embedding-3-small".into(),
                dimensions: Some(1536),
                api_key: None,
            }],
            classification_rules: vec![ClassificationRule {
                hint: "reasoning".into(),
                keywords: vec!["explain".into(), "why".into()],
                patterns: vec![],
                min_length: None,
                max_length: None,
                priority: 10,
            }],
            classification_enabled: true,
        }
    }

    fn test_ctx() -> ToolContext {
        ToolContext::new("/tmp".to_string())
    }

    #[tokio::test]
    async fn list_routes_returns_hints() {
        let tool = ModelRoutingConfigTool::new(test_router());
        let result = tool
            .execute(r#"{"op":"list_routes"}"#, &test_ctx())
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        let routes = v["routes"].as_array().unwrap();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0], "fast");
        assert_eq!(routes[1], "reasoning");
    }

    #[tokio::test]
    async fn list_embedding_routes_returns_hints() {
        let tool = ModelRoutingConfigTool::new(test_router());
        let result = tool
            .execute(r#"{"op":"list_embedding_routes"}"#, &test_ctx())
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        let routes = v["embedding_routes"].as_array().unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0], "default");
    }

    #[tokio::test]
    async fn resolve_hint_returns_route() {
        let tool = ModelRoutingConfigTool::new(test_router());
        let result = tool
            .execute(r#"{"op":"resolve_hint","hint":"fast"}"#, &test_ctx())
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["model"], "gpt-4o-mini");
        assert_eq!(v["provider"], "openai");
    }

    #[tokio::test]
    async fn resolve_hint_unknown_returns_error() {
        let tool = ModelRoutingConfigTool::new(test_router());
        let result = tool
            .execute(r#"{"op":"resolve_hint","hint":"nonexistent"}"#, &test_ctx())
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert!(v["error"].as_str().unwrap().contains("unknown hint"));
    }

    #[tokio::test]
    async fn classify_query_returns_hint() {
        let tool = ModelRoutingConfigTool::new(test_router());
        let result = tool
            .execute(
                r#"{"op":"classify_query","query":"explain why this is wrong"}"#,
                &test_ctx(),
            )
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["hint"], "reasoning");
    }

    #[tokio::test]
    async fn invalid_op_returns_error() {
        let tool = ModelRoutingConfigTool::new(test_router());
        let result = tool
            .execute(r#"{"op":"delete_everything"}"#, &test_ctx())
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert!(v["error"].as_str().unwrap().contains("unknown op"));
    }
}
