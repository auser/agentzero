use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ComposioInput {
    action: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
    #[serde(default)]
    api_key: Option<String>,
}

/// Composio integration tool for external action execution.
///
/// Composio provides a unified API for executing actions across third-party
/// services (GitHub, Slack, Jira, etc.). This tool sends action requests
/// to the Composio API.
///
/// Requires `COMPOSIO_API_KEY` environment variable or `api_key` in input.
#[derive(Debug, Default, Clone, Copy)]
pub struct ComposioTool;

#[async_trait]
impl Tool for ComposioTool {
    fn name(&self) -> &'static str {
        "composio"
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: ComposioInput =
            serde_json::from_str(input).context("composio expects JSON: {\"action\", ...}")?;

        if req.action.trim().is_empty() {
            return Err(anyhow!("action must not be empty"));
        }

        let api_key = req
            .api_key
            .or_else(|| std::env::var("COMPOSIO_API_KEY").ok())
            .ok_or_else(|| anyhow!("COMPOSIO_API_KEY not set and no api_key provided in input"))?;

        if api_key.trim().is_empty() {
            return Err(anyhow!("api_key must not be empty"));
        }

        let params = req
            .params
            .unwrap_or(serde_json::Value::Object(Default::default()));

        let client = reqwest::Client::new();
        let response = client
            .post("https://backend.composio.dev/api/v1/actions/execute")
            .header("x-api-key", &api_key)
            .json(&serde_json::json!({
                "action": req.action,
                "params": params,
            }))
            .send()
            .await
            .context("failed to reach Composio API")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "(no body)".to_string());

        if status.is_success() {
            Ok(ToolResult { output: body })
        } else {
            Err(anyhow!(
                "Composio API returned {}: {}",
                status.as_u16(),
                body
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolContext;

    fn test_ctx() -> ToolContext {
        ToolContext::new("/tmp".to_string())
    }

    #[tokio::test]
    async fn composio_rejects_empty_action() {
        let tool = ComposioTool;
        let err = tool
            .execute(r#"{"action": ""}"#, &test_ctx())
            .await
            .expect_err("empty action should fail");
        assert!(err.to_string().contains("action must not be empty"));
    }

    #[tokio::test]
    async fn composio_rejects_missing_api_key() {
        // Ensure env var is not set for this test
        let had_key = std::env::var("COMPOSIO_API_KEY").ok();
        std::env::remove_var("COMPOSIO_API_KEY");

        let tool = ComposioTool;
        let err = tool
            .execute(r#"{"action": "github.star_repo"}"#, &test_ctx())
            .await
            .expect_err("missing api key should fail");
        assert!(err.to_string().contains("COMPOSIO_API_KEY"));

        // Restore env var if it was set
        if let Some(key) = had_key {
            std::env::set_var("COMPOSIO_API_KEY", key);
        }
    }

    #[tokio::test]
    async fn composio_rejects_empty_api_key() {
        let tool = ComposioTool;
        let err = tool
            .execute(
                r#"{"action": "github.star_repo", "api_key": ""}"#,
                &test_ctx(),
            )
            .await
            .expect_err("empty api key should fail");
        assert!(err.to_string().contains("api_key must not be empty"));
    }
}
