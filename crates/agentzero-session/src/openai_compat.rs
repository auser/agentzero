//! OpenAI-compatible model provider for AgentZero.
//!
//! Works with any server that implements the OpenAI `/v1/chat/completions`
//! API, including:
//! - llama.cpp server (`--host 0.0.0.0 --port 8080`)
//! - vLLM (`python -m vllm.entrypoints.openai.api_server`)
//! - LM Studio (built-in server)
//! - text-generation-webui (with openai extension)
//! - LocalAI
//!
//! All calls stay local when pointed at a local server.

use agentzero_tracing::{debug, info, warn};
use serde::{Deserialize, Serialize};

use crate::ollama::{ChatMessage, ChatResult, ToolCall, ToolCallFunction, ToolDefinition};
use crate::provider::{ModelLocation, ModelProvider, ModelProviderError};

/// Configuration for an OpenAI-compatible provider.
#[derive(Debug, Clone)]
pub struct OpenAICompatConfig {
    /// Base URL (e.g. `http://localhost:8080` for llama.cpp).
    pub base_url: String,
    /// Model name to send in the request.
    pub model: String,
    /// Whether this server is local (affects data classification routing).
    pub is_local: bool,
    /// Optional API key (some servers require it).
    pub api_key: Option<String>,
}

impl OpenAICompatConfig {
    /// Default config for llama.cpp server.
    pub fn llama_cpp() -> Self {
        Self {
            base_url: "http://localhost:8080".into(),
            model: "default".into(),
            is_local: true,
            api_key: None,
        }
    }

    /// Default config for vLLM server.
    pub fn vllm() -> Self {
        Self {
            base_url: "http://localhost:8000".into(),
            model: "default".into(),
            is_local: true,
            api_key: None,
        }
    }

    /// Default config for LM Studio.
    pub fn lm_studio() -> Self {
        Self {
            base_url: "http://localhost:1234".into(),
            model: "default".into(),
            is_local: true,
            api_key: None,
        }
    }
}

// --- OpenAI API types ---

#[derive(Debug, Serialize)]
struct CompletionRequest {
    model: String,
    messages: Vec<CompletionMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<CompletionTool>>,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct CompletionMessage {
    role: String,
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<CompletionToolCall>>,
}

#[derive(Debug, Serialize)]
struct CompletionTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: CompletionFunction,
}

#[derive(Debug, Serialize)]
struct CompletionFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct CompletionToolCall {
    id: Option<String>,
    #[serde(rename = "type")]
    call_type: Option<String>,
    function: CompletionToolCallFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct CompletionToolCallFunction {
    name: String,
    arguments: String, // JSON string, unlike Ollama which uses Value
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    choices: Vec<CompletionChoice>,
}

#[derive(Debug, Deserialize)]
struct CompletionChoice {
    message: CompletionMessage,
}

/// OpenAI-compatible model provider.
pub struct OpenAICompatProvider {
    config: OpenAICompatConfig,
    client: reqwest::Client,
}

impl OpenAICompatProvider {
    pub fn new(config: OpenAICompatConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("reqwest client should build");
        Self { config, client }
    }

    /// Check if the server is reachable.
    pub async fn health_check(&self) -> Result<bool, ModelProviderError> {
        let url = format!("{}/v1/models", self.config.base_url);
        debug!(url = %url, "checking openai-compat health");

        let mut req = self.client.get(&url);
        if let Some(ref key) = self.config.api_key {
            req = req.bearer_auth(key);
        }

        match req.send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    info!(model = %self.config.model, "openai-compat server available");
                    Ok(true)
                } else {
                    warn!(status = %resp.status(), "openai-compat server returned non-success");
                    Ok(false)
                }
            }
            Err(e) => {
                debug!(error = %e, "openai-compat server not reachable");
                Err(ModelProviderError::Unavailable(e.to_string()))
            }
        }
    }

    /// Send a chat completion request.
    pub async fn chat(&self, messages: &[ChatMessage]) -> Result<String, ModelProviderError> {
        let result = self.chat_with_tools(messages, None).await?;
        Ok(result.content)
    }

    /// Send a chat completion with tool definitions.
    pub async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<ChatResult, ModelProviderError> {
        let url = format!("{}/v1/chat/completions", self.config.base_url);

        let oai_messages: Vec<CompletionMessage> = messages
            .iter()
            .map(|m| CompletionMessage {
                role: m.role.clone(),
                content: Some(m.content.clone()),
                tool_calls: None,
            })
            .collect();

        let oai_tools = tools.map(|ts| {
            ts.iter()
                .map(|t| CompletionTool {
                    tool_type: "function".into(),
                    function: CompletionFunction {
                        name: t.function.name.clone(),
                        description: t.function.description.clone(),
                        parameters: t.function.parameters.clone(),
                    },
                })
                .collect()
        });

        let request = CompletionRequest {
            model: self.config.model.clone(),
            messages: oai_messages,
            tools: oai_tools,
            stream: false,
        };

        debug!(
            model = %self.config.model,
            messages = messages.len(),
            "sending openai-compat chat request"
        );

        let mut req = self.client.post(&url).json(&request);
        if let Some(ref key) = self.config.api_key {
            req = req.bearer_auth(key);
        }

        let response = req
            .send()
            .await
            .map_err(|e| ModelProviderError::Unavailable(format!("server unreachable: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ModelProviderError::Failed(format!(
                "server returned {status}: {body}"
            )));
        }

        let resp: CompletionResponse = response
            .json()
            .await
            .map_err(|e| ModelProviderError::Failed(format!("failed to parse response: {e}")))?;

        let choice = resp
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ModelProviderError::Failed("no choices in response".into()))?;

        let content = choice.message.content.unwrap_or_default();

        // Convert OpenAI tool calls to our format
        let tool_calls = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .filter_map(|tc| {
                let args: serde_json::Value = serde_json::from_str(&tc.function.arguments).ok()?;
                Some(ToolCall {
                    function: ToolCallFunction {
                        name: tc.function.name,
                        arguments: args,
                    },
                })
            })
            .collect();

        info!(
            model = %self.config.model,
            response_len = content.len(),
            "received openai-compat response"
        );

        Ok(ChatResult {
            content,
            tool_calls,
        })
    }

    pub fn model_name(&self) -> &str {
        &self.config.model
    }

    pub fn server_type(&self) -> &str {
        if self.config.base_url.contains(":8080") {
            "llama.cpp"
        } else if self.config.base_url.contains(":8000") {
            "vLLM"
        } else if self.config.base_url.contains(":1234") {
            "LM Studio"
        } else {
            "openai-compatible"
        }
    }
}

impl ModelProvider for OpenAICompatProvider {
    fn name(&self) -> &str {
        "openai-compatible"
    }

    fn location(&self) -> ModelLocation {
        if self.config.is_local {
            ModelLocation::Local
        } else {
            ModelLocation::Remote
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::DataClassification;

    #[test]
    fn llama_cpp_config() {
        let config = OpenAICompatConfig::llama_cpp();
        assert_eq!(config.base_url, "http://localhost:8080");
        assert!(config.is_local);
        assert!(config.api_key.is_none());
    }

    #[test]
    fn vllm_config() {
        let config = OpenAICompatConfig::vllm();
        assert_eq!(config.base_url, "http://localhost:8000");
        assert!(config.is_local);
    }

    #[test]
    fn lm_studio_config() {
        let config = OpenAICompatConfig::lm_studio();
        assert_eq!(config.base_url, "http://localhost:1234");
        assert!(config.is_local);
    }

    #[test]
    fn local_provider_accepts_all_classifications() {
        let provider = OpenAICompatProvider::new(OpenAICompatConfig::llama_cpp());
        assert!(provider.accepts_classification(DataClassification::Secret));
        assert!(provider.accepts_classification(DataClassification::Pii));
        assert!(provider.accepts_classification(DataClassification::Private));
    }

    #[test]
    fn remote_provider_restricts_classifications() {
        let config = OpenAICompatConfig {
            base_url: "https://api.remote.example".into(),
            model: "gpt-4".into(),
            is_local: false,
            api_key: Some("sk-test".into()),
        };
        let provider = OpenAICompatProvider::new(config);
        assert_eq!(provider.location(), ModelLocation::Remote);
        assert!(!provider.accepts_classification(DataClassification::Secret));
        assert!(provider.accepts_classification(DataClassification::Public));
    }

    #[test]
    fn server_type_detection() {
        assert_eq!(
            OpenAICompatProvider::new(OpenAICompatConfig::llama_cpp()).server_type(),
            "llama.cpp"
        );
        assert_eq!(
            OpenAICompatProvider::new(OpenAICompatConfig::vllm()).server_type(),
            "vLLM"
        );
        assert_eq!(
            OpenAICompatProvider::new(OpenAICompatConfig::lm_studio()).server_type(),
            "LM Studio"
        );
    }
}
