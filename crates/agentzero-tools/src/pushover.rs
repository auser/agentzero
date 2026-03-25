use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct PushoverInput {
    /// Notification message
    message: String,
    /// Optional notification title
    #[serde(default)]
    title: Option<String>,
    /// Priority: -2 to 2
    #[serde(default)]
    priority: Option<i8>,
    /// Pushover API token (or use PUSHOVER_TOKEN env)
    #[serde(default)]
    token: Option<String>,
    /// Pushover user key (or use PUSHOVER_USER env)
    #[serde(default)]
    user: Option<String>,
}

/// Pushover push notification tool.
///
/// Sends push notifications via the Pushover API. Requires either:
/// - `PUSHOVER_TOKEN` and `PUSHOVER_USER` environment variables, or
/// - `token` and `user` fields in the input JSON.
///
/// Priority levels: -2 (lowest), -1, 0 (normal), 1 (high), 2 (emergency).
#[tool(
    name = "pushover",
    description = "Send push notifications via the Pushover service."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct PushoverTool;

#[async_trait]
impl Tool for PushoverTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(PushoverInput::schema())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: PushoverInput =
            serde_json::from_str(input).context("pushover expects JSON: {\"message\", ...}")?;

        if req.message.trim().is_empty() {
            return Err(anyhow!("message must not be empty"));
        }

        let token = req
            .token
            .or_else(|| std::env::var("PUSHOVER_TOKEN").ok())
            .ok_or_else(|| anyhow!("PUSHOVER_TOKEN not set and no token provided"))?;

        let user = req
            .user
            .or_else(|| std::env::var("PUSHOVER_USER").ok())
            .ok_or_else(|| anyhow!("PUSHOVER_USER not set and no user provided"))?;

        if token.trim().is_empty() {
            return Err(anyhow!("token must not be empty"));
        }
        if user.trim().is_empty() {
            return Err(anyhow!("user must not be empty"));
        }

        let priority = req.priority.unwrap_or(0);
        if !(-2..=2).contains(&priority) {
            return Err(anyhow!("priority must be between -2 and 2, got {priority}"));
        }

        let mut form = vec![
            ("token", token),
            ("user", user),
            ("message", req.message.clone()),
            ("priority", priority.to_string()),
        ];
        if let Some(ref title) = req.title {
            form.push(("title", title.clone()));
        }

        let client = reqwest::Client::new();
        let response = client
            .post("https://api.pushover.net/1/messages.json")
            .form(&form)
            .send()
            .await
            .context("failed to reach Pushover API")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "(no body)".to_string());

        if status.is_success() {
            Ok(ToolResult {
                output: format!("notification sent: {body}"),
            })
        } else {
            Err(anyhow!(
                "Pushover API returned {}: {}",
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
    async fn pushover_rejects_empty_message() {
        let tool = PushoverTool;
        let err = tool
            .execute(r#"{"message": ""}"#, &test_ctx())
            .await
            .expect_err("empty message should fail");
        assert!(err.to_string().contains("message must not be empty"));
    }

    #[tokio::test]
    async fn pushover_rejects_missing_token() {
        let had_token = std::env::var("PUSHOVER_TOKEN").ok();
        let had_user = std::env::var("PUSHOVER_USER").ok();
        std::env::remove_var("PUSHOVER_TOKEN");
        std::env::remove_var("PUSHOVER_USER");

        let tool = PushoverTool;
        let err = tool
            .execute(r#"{"message": "test"}"#, &test_ctx())
            .await
            .expect_err("missing token should fail");
        assert!(err.to_string().contains("PUSHOVER_TOKEN"));

        if let Some(t) = had_token {
            std::env::set_var("PUSHOVER_TOKEN", t);
        }
        if let Some(u) = had_user {
            std::env::set_var("PUSHOVER_USER", u);
        }
    }

    #[tokio::test]
    async fn pushover_rejects_invalid_priority() {
        let tool = PushoverTool;
        let err = tool
            .execute(
                r#"{"message": "test", "token": "tok", "user": "usr", "priority": 5}"#,
                &test_ctx(),
            )
            .await
            .expect_err("invalid priority should fail");
        assert!(err.to_string().contains("priority must be between"));
    }

    #[tokio::test]
    async fn pushover_rejects_empty_token() {
        let tool = PushoverTool;
        let err = tool
            .execute(
                r#"{"message": "test", "token": "", "user": "usr"}"#,
                &test_ctx(),
            )
            .await
            .expect_err("empty token should fail");
        assert!(err.to_string().contains("token must not be empty"));
    }
}
