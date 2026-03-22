//! A2A (Agent-to-Agent) protocol tool — enables agents to dynamically
//! discover and communicate with external A2A agents via HTTP.

use agentzero_core::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Deserialize)]
struct Input {
    /// Action to perform: "discover", "send", "status", or "cancel".
    action: String,
    /// Base URL of the A2A agent. Required for discover and send.
    #[serde(default)]
    url: Option<String>,
    /// Message text to send. Required for send.
    #[serde(default)]
    message: Option<String>,
    /// Task ID for status/cancel operations.
    #[serde(default)]
    task_id: Option<String>,
}

/// Tool that interacts with external A2A (Agent-to-Agent) protocol agents.
///
/// Supports four actions:
/// - `discover`: Fetch an agent's Agent Card from `{url}/.well-known/agent.json`
/// - `send`: Send a message to an A2A agent, creating a new task
/// - `status`: Query the status of an existing task
/// - `cancel`: Cancel an existing task
pub struct A2aTool;

impl Default for A2aTool {
    fn default() -> Self {
        Self
    }
}

/// Validate that a URL uses an HTTP or HTTPS scheme.
fn validate_url_scheme(url: &str) -> anyhow::Result<()> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(anyhow::anyhow!(
            "URL must use http:// or https:// scheme, got: {url}"
        ));
    }
    Ok(())
}

/// Generate a pseudo-unique task ID from the current time.
fn generate_task_id() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}

#[async_trait]
impl Tool for A2aTool {
    fn name(&self) -> &'static str {
        "a2a"
    }

    fn description(&self) -> &'static str {
        "Interact with external A2A (Agent-to-Agent) protocol agents. \
         Discover agent capabilities, send messages, check task status, \
         or cancel tasks on remote A2A-compatible agents."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["discover", "send", "status", "cancel"],
                    "description": "Action to perform: discover, send, status, or cancel"
                },
                "url": {
                    "type": "string",
                    "description": "Base URL of the A2A agent (required for discover and send)"
                },
                "message": {
                    "type": "string",
                    "description": "Message text to send (required for send)"
                },
                "task_id": {
                    "type": "string",
                    "description": "Task ID (required for status and cancel)"
                }
            }
        }))
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: Input = serde_json::from_str(input)
            .map_err(|e| anyhow::anyhow!("a2a expects JSON: {{\"action\": \"...\"}}: {e}"))?;

        match parsed.action.as_str() {
            "discover" => execute_discover(parsed.url).await,
            "send" => execute_send(parsed.url, parsed.message).await,
            "status" => execute_status(parsed.url, parsed.task_id).await,
            "cancel" => execute_cancel(parsed.url, parsed.task_id).await,
            other => Err(anyhow::anyhow!(
                "unknown action: {other}. Must be one of: discover, send, status, cancel"
            )),
        }
    }
}

fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

async fn execute_discover(url: Option<String>) -> anyhow::Result<ToolResult> {
    let url = url
        .filter(|u| !u.is_empty())
        .ok_or_else(|| anyhow::anyhow!("url is required for discover action"))?;
    validate_url_scheme(&url)?;

    let base = url.trim_end_matches('/');
    let agent_card_url = format!("{base}/.well-known/agent.json");

    let client = build_client();
    let resp =
        client.get(&agent_card_url).send().await.map_err(|e| {
            anyhow::anyhow!("failed to fetch agent card from {agent_card_url}: {e}")
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(anyhow::anyhow!(
            "agent card request to {agent_card_url} returned {status}"
        ));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("failed to parse agent card JSON: {e}"))?;

    Ok(ToolResult {
        output: serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()),
    })
}

async fn execute_send(url: Option<String>, message: Option<String>) -> anyhow::Result<ToolResult> {
    let url = url
        .filter(|u| !u.is_empty())
        .ok_or_else(|| anyhow::anyhow!("url is required for send action"))?;
    validate_url_scheme(&url)?;

    let message = message
        .filter(|m| !m.is_empty())
        .ok_or_else(|| anyhow::anyhow!("message is required for send action"))?;

    let base = url.trim_end_matches('/');
    let a2a_url = format!("{base}/a2a");
    let task_id = generate_task_id();

    let rpc_body = json!({
        "jsonrpc": "2.0",
        "id": "1",
        "method": "message/send",
        "params": {
            "id": task_id,
            "message": {
                "role": "user",
                "parts": [{"kind": "text", "text": message}]
            }
        }
    });

    let client = build_client();
    let resp = client
        .post(&a2a_url)
        .json(&rpc_body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("A2A send request to {a2a_url} failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("A2A send returned {status}: {body}"));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("failed to parse A2A response: {e}"))?;

    Ok(ToolResult {
        output: serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()),
    })
}

async fn execute_status(
    url: Option<String>,
    task_id: Option<String>,
) -> anyhow::Result<ToolResult> {
    let url = url
        .filter(|u| !u.is_empty())
        .ok_or_else(|| anyhow::anyhow!("url is required for status action"))?;
    validate_url_scheme(&url)?;

    let task_id = task_id
        .filter(|t| !t.is_empty())
        .ok_or_else(|| anyhow::anyhow!("task_id is required for status action"))?;

    let base = url.trim_end_matches('/');
    let a2a_url = format!("{base}/a2a");

    let rpc_body = json!({
        "jsonrpc": "2.0",
        "id": "1",
        "method": "tasks/get",
        "params": {
            "id": task_id
        }
    });

    let client = build_client();
    let resp = client
        .post(&a2a_url)
        .json(&rpc_body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("A2A status request to {a2a_url} failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("A2A status returned {status}: {body}"));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("failed to parse A2A response: {e}"))?;

    Ok(ToolResult {
        output: serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()),
    })
}

async fn execute_cancel(
    url: Option<String>,
    task_id: Option<String>,
) -> anyhow::Result<ToolResult> {
    let url = url
        .filter(|u| !u.is_empty())
        .ok_or_else(|| anyhow::anyhow!("url is required for cancel action"))?;
    validate_url_scheme(&url)?;

    let task_id = task_id
        .filter(|t| !t.is_empty())
        .ok_or_else(|| anyhow::anyhow!("task_id is required for cancel action"))?;

    let base = url.trim_end_matches('/');
    let a2a_url = format!("{base}/a2a");

    let rpc_body = json!({
        "jsonrpc": "2.0",
        "id": "1",
        "method": "tasks/cancel",
        "params": {
            "id": task_id
        }
    });

    let client = build_client();
    let resp = client
        .post(&a2a_url)
        .json(&rpc_body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("A2A cancel request to {a2a_url} failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("A2A cancel returned {status}: {body}"));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("failed to parse A2A response: {e}"))?;

    Ok(ToolResult {
        output: serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolContext;

    fn ctx() -> ToolContext {
        ToolContext::new("/tmp".to_string())
    }

    #[test]
    fn input_schema_is_valid() {
        let tool = A2aTool;
        let schema = tool.input_schema().expect("should have schema");
        assert_eq!(schema["required"][0], "action");
        assert!(schema["properties"]["action"].is_object());
        assert!(schema["properties"]["url"].is_object());
        assert!(schema["properties"]["message"].is_object());
        assert!(schema["properties"]["task_id"].is_object());
    }

    #[tokio::test]
    async fn empty_url_returns_error_for_discover() {
        let tool = A2aTool;
        let err = tool
            .execute(r#"{"action": "discover", "url": ""}"#, &ctx())
            .await
            .expect_err("empty url should fail");
        assert!(err.to_string().contains("url is required"));
    }

    #[tokio::test]
    async fn missing_url_returns_error_for_discover() {
        let tool = A2aTool;
        let err = tool
            .execute(r#"{"action": "discover"}"#, &ctx())
            .await
            .expect_err("missing url should fail");
        assert!(err.to_string().contains("url is required"));
    }

    #[tokio::test]
    async fn missing_message_returns_error_for_send() {
        let tool = A2aTool;
        let err = tool
            .execute(
                r#"{"action": "send", "url": "http://localhost:9999"}"#,
                &ctx(),
            )
            .await
            .expect_err("missing message should fail");
        assert!(err.to_string().contains("message is required"));
    }

    #[tokio::test]
    async fn invalid_url_scheme_rejected() {
        let tool = A2aTool;
        let err = tool
            .execute(
                r#"{"action": "discover", "url": "ftp://example.com"}"#,
                &ctx(),
            )
            .await
            .expect_err("ftp scheme should be rejected");
        assert!(err.to_string().contains("http://"));
    }

    #[tokio::test]
    async fn missing_task_id_returns_error_for_status() {
        let tool = A2aTool;
        let err = tool
            .execute(
                r#"{"action": "status", "url": "http://localhost:9999"}"#,
                &ctx(),
            )
            .await
            .expect_err("missing task_id should fail");
        assert!(err.to_string().contains("task_id is required"));
    }

    #[tokio::test]
    async fn missing_task_id_returns_error_for_cancel() {
        let tool = A2aTool;
        let err = tool
            .execute(
                r#"{"action": "cancel", "url": "http://localhost:9999"}"#,
                &ctx(),
            )
            .await
            .expect_err("missing task_id should fail");
        assert!(err.to_string().contains("task_id is required"));
    }

    #[tokio::test]
    async fn invalid_action_returns_error() {
        let tool = A2aTool;
        let err = tool
            .execute(r#"{"action": "foo"}"#, &ctx())
            .await
            .expect_err("invalid action should fail");
        assert!(err.to_string().contains("unknown action"));
    }

    #[tokio::test]
    async fn invalid_json_returns_error() {
        let tool = A2aTool;
        let err = tool
            .execute("not json", &ctx())
            .await
            .expect_err("invalid JSON should fail");
        assert!(err.to_string().contains("a2a expects JSON"));
    }

    #[test]
    fn validate_url_scheme_accepts_http() {
        assert!(validate_url_scheme("http://example.com").is_ok());
        assert!(validate_url_scheme("https://example.com").is_ok());
    }

    #[test]
    fn validate_url_scheme_rejects_non_http() {
        assert!(validate_url_scheme("ftp://example.com").is_err());
        assert!(validate_url_scheme("file:///etc/passwd").is_err());
        assert!(validate_url_scheme("example.com").is_err());
    }

    #[test]
    fn generate_task_id_is_nonempty() {
        let id = generate_task_id();
        assert!(!id.is_empty());
    }
}
