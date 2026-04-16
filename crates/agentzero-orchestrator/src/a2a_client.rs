//! A2A client — calls external A2A agents via the Agent-to-Agent protocol.
//!
//! Implements `AgentEndpoint` so external A2A agents become first-class
//! swarm participants through `ConverseTool`.

use agentzero_core::a2a_types::*;
use agentzero_core::types::AgentEndpoint;
use async_trait::async_trait;
use serde_json::json;

/// An `AgentEndpoint` that communicates with external A2A agents via HTTP.
#[derive(Debug)]
pub struct A2aAgentEndpoint {
    /// Unique identifier for this endpoint within the swarm.
    id: String,
    /// Base URL of the external A2A agent (e.g., "https://agent.example.com").
    base_url: String,
    /// Optional bearer token for authentication.
    auth_token: Option<String>,
    /// HTTP client.
    client: reqwest::Client,
    /// Timeout in seconds for A2A calls.
    timeout_secs: u64,
    /// Capability ceiling forwarded to the remote agent on every `tasks/send`.
    max_capabilities: Vec<agentzero_core::security::capability::Capability>,
}

impl A2aAgentEndpoint {
    pub fn new(
        id: String,
        base_url: String,
        auth_token: Option<String>,
        timeout_secs: u64,
        max_capabilities: Vec<agentzero_core::security::capability::Capability>,
    ) -> anyhow::Result<Self> {
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(anyhow::anyhow!(
                "A2A agent URL must use http:// or https:// scheme, got: {base_url}"
            ));
        }
        Ok(Self {
            id,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_token,
            client: reqwest::Client::new(),
            timeout_secs,
            max_capabilities,
        })
    }

    /// Send a JSON-RPC request to the A2A agent and return the parsed response.
    async fn rpc_call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!("{}/a2a", self.base_url);
        let rpc_request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let mut req = self.client.post(&url).json(&rpc_request);
        if let Some(ref token) = self.auth_token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }

        let resp = req
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("A2A {method} request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("A2A {method} returned {status}: {body}"));
        }

        let rpc_response: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("failed to parse A2A {method} response: {e}"))?;

        if let Some(error) = rpc_response.get("error") {
            return Err(anyhow::anyhow!("A2A {method} error: {error}"));
        }

        rpc_response
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("A2A {method} response missing result"))
    }

    /// Check the status of an existing A2A task.
    pub async fn check_status(&self, task_id: &str) -> anyhow::Result<Task> {
        let result = self.rpc_call("tasks/get", json!({ "id": task_id })).await?;
        serde_json::from_value(result)
            .map_err(|e| anyhow::anyhow!("failed to parse task status: {e}"))
    }

    /// Cancel an existing A2A task.
    pub async fn cancel_task(&self, task_id: &str) -> anyhow::Result<Task> {
        let result = self
            .rpc_call("tasks/cancel", json!({ "id": task_id }))
            .await?;
        serde_json::from_value(result)
            .map_err(|e| anyhow::anyhow!("failed to parse cancelled task: {e}"))
    }

    /// Fetch the remote agent's Agent Card.
    pub async fn fetch_agent_card(&self) -> anyhow::Result<AgentCard> {
        let url = format!("{}/.well-known/agent.json", self.base_url);
        let mut req = self.client.get(&url);
        if let Some(ref token) = self.auth_token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        let resp = req
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("failed to fetch agent card: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(anyhow::anyhow!("agent card request failed: {status}"));
        }

        resp.json::<AgentCard>()
            .await
            .map_err(|e| anyhow::anyhow!("failed to parse agent card: {e}"))
    }
}

#[async_trait]
impl AgentEndpoint for A2aAgentEndpoint {
    async fn send(&self, message: &str, conversation_id: &str) -> anyhow::Result<String> {
        let mut params = serde_json::json!({
            "id": conversation_id,
            "message": {
                "role": "user",
                "parts": [{"type": "text", "text": message}]
            }
        });

        if !self.max_capabilities.is_empty() {
            params["metadata"] = serde_json::json!({
                "agentZeroMaxCapabilities":
                    serde_json::to_value(&self.max_capabilities)
                        .unwrap_or(serde_json::Value::Null)
            });
        }

        let result = self.rpc_call("tasks/send", params).await?;

        // Try to get the status message text, or fall back to last history entry.
        let text = result
            .get("status")
            .and_then(|s| s.get("message"))
            .and_then(|m| m.get("parts"))
            .and_then(|parts| parts.as_array())
            .and_then(|parts| parts.iter().find_map(|p| p.get("text")))
            .and_then(|t| t.as_str())
            .unwrap_or("(no response text)");

        Ok(text.to_string())
    }

    fn agent_id(&self) -> &str {
        &self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a2a_endpoint_agent_id() {
        let endpoint = A2aAgentEndpoint::new(
            "remote-agent".to_string(),
            "https://agent.example.com".to_string(),
            None,
            30,
            vec![],
        )
        .expect("valid URL should succeed");
        assert_eq!(endpoint.agent_id(), "remote-agent");
    }

    #[test]
    fn a2a_endpoint_base_url_trimmed() {
        let endpoint = A2aAgentEndpoint::new(
            "test".to_string(),
            "https://agent.example.com/".to_string(),
            None,
            30,
            vec![],
        )
        .expect("valid URL should succeed");
        assert_eq!(endpoint.base_url, "https://agent.example.com");
    }

    #[test]
    fn a2a_endpoint_rejects_non_http_url() {
        let err = A2aAgentEndpoint::new(
            "test".to_string(),
            "ftp://agent.example.com".to_string(),
            None,
            30,
            vec![],
        )
        .expect_err("ftp scheme should be rejected");
        assert!(err.to_string().contains("http://"));
    }

    #[tokio::test]
    async fn send_returns_error_on_connection_failure() {
        let endpoint = A2aAgentEndpoint::new(
            "test".to_string(),
            "http://localhost:1".to_string(), // Invalid port
            None,
            5,
            vec![],
        )
        .expect("valid URL should succeed");
        let err = endpoint
            .send("hello", "conv-1")
            .await
            .expect_err("should fail");
        assert!(
            err.to_string().contains("request failed"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn fetch_agent_card_returns_error_on_connection_failure() {
        let endpoint = A2aAgentEndpoint::new(
            "test".to_string(),
            "http://localhost:1".to_string(),
            None,
            5,
            vec![],
        )
        .expect("valid URL should succeed");
        let err = endpoint.fetch_agent_card().await.expect_err("should fail");
        assert!(err.to_string().contains("fetch agent card"));
    }

    #[tokio::test]
    async fn check_status_returns_error_on_connection_failure() {
        let endpoint = A2aAgentEndpoint::new(
            "test".to_string(),
            "http://localhost:1".to_string(),
            None,
            5,
            vec![],
        )
        .expect("valid URL should succeed");
        let err = endpoint
            .check_status("task-123")
            .await
            .expect_err("should fail");
        assert!(
            err.to_string().contains("request failed"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn cancel_task_returns_error_on_connection_failure() {
        let endpoint = A2aAgentEndpoint::new(
            "test".to_string(),
            "http://localhost:1".to_string(),
            None,
            5,
            vec![],
        )
        .expect("valid URL should succeed");
        let err = endpoint
            .cancel_task("task-456")
            .await
            .expect_err("should fail");
        assert!(
            err.to_string().contains("request failed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn a2a_endpoint_new_with_max_capabilities() {
        use agentzero_core::security::capability::Capability;
        let caps = vec![Capability::Tool {
            name: "web_search".to_string(),
        }];
        let endpoint = A2aAgentEndpoint::new(
            "agent-x".to_string(),
            "https://agent.example.com".to_string(),
            None,
            30,
            caps,
        )
        .expect("valid");
        assert_eq!(endpoint.agent_id(), "agent-x");
        assert_eq!(endpoint.max_capabilities.len(), 1);
    }

    #[test]
    fn a2a_max_capabilities_metadata_serializes_correctly() {
        use agentzero_core::security::capability::Capability;
        let caps = vec![
            Capability::Tool {
                name: "web_search".to_string(),
            },
            Capability::Tool {
                name: "read_file".to_string(),
            },
        ];
        let metadata = serde_json::json!({
            "agentZeroMaxCapabilities": serde_json::to_value(&caps).unwrap()
        });
        let arr = metadata["agentZeroMaxCapabilities"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }
}
