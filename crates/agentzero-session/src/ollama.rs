//! Ollama model provider for local LLM inference.
//!
//! Connects to a local Ollama server (default: http://localhost:11434)
//! via its REST API. All calls stay local — no data leaves the machine.

use agentzero_tracing::{debug, info, warn};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::provider::{ModelLocation, ModelProvider, ModelProviderError};

#[derive(Debug, Error)]
pub enum OllamaError {
    #[error("ollama connection failed: {0}")]
    ConnectionFailed(String),
    #[error("ollama request failed: {0}")]
    RequestFailed(String),
    #[error("ollama returned error: {0}")]
    ApiError(String),
}

/// Message in an Ollama chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    message: ChatMessage,
}

/// Ollama provider configuration.
#[derive(Debug, Clone)]
pub struct OllamaConfig {
    pub base_url: String,
    pub model: String,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434".into(),
            model: "llama3.2".into(),
        }
    }
}

/// Ollama model provider for local inference.
pub struct OllamaProvider {
    config: OllamaConfig,
    client: reqwest::Client,
}

impl OllamaProvider {
    /// Create a new Ollama provider with the given configuration.
    pub fn new(config: OllamaConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("reqwest client should build");
        Self { config, client }
    }

    /// Create with default configuration (localhost:11434, llama3.2).
    pub fn default_local() -> Self {
        Self::new(OllamaConfig::default())
    }

    /// Check if the Ollama server is reachable.
    pub async fn health_check(&self) -> Result<bool, OllamaError> {
        let url = format!("{}/api/tags", self.config.base_url);
        debug!(url = %url, "checking ollama health");
        match self.client.get(&url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    info!(model = %self.config.model, "ollama is available");
                    Ok(true)
                } else {
                    warn!(status = %resp.status(), "ollama returned non-success");
                    Ok(false)
                }
            }
            Err(e) => {
                debug!(error = %e, "ollama not reachable");
                Err(OllamaError::ConnectionFailed(e.to_string()))
            }
        }
    }

    /// Send a chat completion request to Ollama.
    pub async fn chat(&self, messages: &[ChatMessage]) -> Result<String, ModelProviderError> {
        let url = format!("{}/api/chat", self.config.base_url);

        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: messages.to_vec(),
            stream: false,
        };

        debug!(
            model = %self.config.model,
            messages = messages.len(),
            "sending chat request to ollama"
        );

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| ModelProviderError::Unavailable(format!("ollama unreachable: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ModelProviderError::Failed(format!(
                "ollama returned {status}: {body}"
            )));
        }

        let chat_resp: ChatResponse = response
            .json()
            .await
            .map_err(|e| ModelProviderError::Failed(format!("failed to parse response: {e}")))?;

        info!(
            model = %self.config.model,
            response_len = chat_resp.message.content.len(),
            "received chat response"
        );

        Ok(chat_resp.message.content)
    }

    /// Return the configured model name.
    pub fn model_name(&self) -> &str {
        &self.config.model
    }
}

impl ModelProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn location(&self) -> ModelLocation {
        ModelLocation::Local
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::DataClassification;

    #[test]
    fn default_config() {
        let config = OllamaConfig::default();
        assert_eq!(config.base_url, "http://localhost:11434");
        assert_eq!(config.model, "llama3.2");
    }

    #[test]
    fn provider_is_local() {
        let provider = OllamaProvider::default_local();
        assert_eq!(provider.location(), ModelLocation::Local);
        assert_eq!(provider.name(), "ollama");
    }

    #[test]
    fn accepts_all_classifications() {
        let provider = OllamaProvider::default_local();
        assert!(provider.accepts_classification(DataClassification::Secret));
        assert!(provider.accepts_classification(DataClassification::Pii));
        assert!(provider.accepts_classification(DataClassification::Private));
    }

    #[test]
    fn custom_config() {
        let config = OllamaConfig {
            base_url: "http://gpu-server:11434".into(),
            model: "codellama".into(),
        };
        let provider = OllamaProvider::new(config);
        assert_eq!(provider.model_name(), "codellama");
    }
}
