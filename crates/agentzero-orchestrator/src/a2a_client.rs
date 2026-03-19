//! A2A client — calls external A2A agents via the Agent-to-Agent protocol.
//!
//! Implements `AgentEndpoint` so external A2A agents become first-class
//! swarm participants through `ConverseTool`.

use agentzero_core::a2a_types::*;
use agentzero_core::types::AgentEndpoint;
use async_trait::async_trait;
use serde_json::json;

/// An `AgentEndpoint` that communicates with external A2A agents via HTTP.
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
}

impl A2aAgentEndpoint {
    pub fn new(
        id: String,
        base_url: String,
        auth_token: Option<String>,
        timeout_secs: u64,
    ) -> Self {
        Self {
            id,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_token,
            client: reqwest::Client::new(),
            timeout_secs,
        }
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
        let url = format!("{}/a2a", self.base_url);

        let rpc_request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tasks/send",
            "params": {
                "id": conversation_id,
                "message": {
                    "role": "user",
                    "parts": [{"type": "text", "text": message}]
                }
            }
        });

        let mut req = self.client.post(&url).json(&rpc_request);
        if let Some(ref token) = self.auth_token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }

        let resp = req
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("A2A request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("A2A request returned {status}: {body}"));
        }

        let rpc_response: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("failed to parse A2A response: {e}"))?;

        // Check for JSON-RPC error.
        if let Some(error) = rpc_response.get("error") {
            return Err(anyhow::anyhow!("A2A error: {error}"));
        }

        // Extract the agent's response text from the task result.
        let result = rpc_response
            .get("result")
            .ok_or_else(|| anyhow::anyhow!("A2A response missing result"))?;

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
        );
        assert_eq!(endpoint.agent_id(), "remote-agent");
    }

    #[test]
    fn a2a_endpoint_base_url_trimmed() {
        let endpoint = A2aAgentEndpoint::new(
            "test".to_string(),
            "https://agent.example.com/".to_string(),
            None,
            30,
        );
        assert_eq!(endpoint.base_url, "https://agent.example.com");
    }

    #[tokio::test]
    async fn send_returns_error_on_connection_failure() {
        let endpoint = A2aAgentEndpoint::new(
            "test".to_string(),
            "http://localhost:1".to_string(), // Invalid port
            None,
            5,
        );
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
        );
        let err = endpoint.fetch_agent_card().await.expect_err("should fail");
        assert!(err.to_string().contains("fetch agent card"));
    }
}
