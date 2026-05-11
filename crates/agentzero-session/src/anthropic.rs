//! Anthropic Claude model provider for AgentZero.
//!
//! Implements the Anthropic Messages API (`/v1/messages`) which differs
//! from the OpenAI `/v1/chat/completions` format. Supports tool calling
//! via Anthropic's native tool_use content blocks.
//!
//! This is always a remote provider — PII redaction applies per ADR 0002.

use agentzero_tracing::{debug, info, warn};
use serde::{Deserialize, Serialize};

use crate::ollama::{ChatMessage, ChatResult, ToolCall, ToolCallFunction, ToolDefinition};
use crate::provider::{ModelLocation, ModelProvider, ModelProviderError};

/// Configuration for the Anthropic provider.
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    /// API key (required). Use vault reference: `vault://anthropic/api_key`
    pub api_key: String,
    /// Model name (e.g. "claude-sonnet-4-20250514").
    pub model: String,
    /// Base URL (default: `https://api.anthropic.com`).
    pub base_url: String,
    /// Max tokens for response (default: 4096).
    pub max_tokens: u32,
}

impl AnthropicConfig {
    /// Default config for Claude Sonnet.
    pub fn sonnet(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "claude-sonnet-4-20250514".into(),
            base_url: "https://api.anthropic.com".into(),
            max_tokens: 4096,
        }
    }

    /// Default config for Claude Haiku (fast, cheap).
    pub fn haiku(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "claude-haiku-4-5-20251001".into(),
            base_url: "https://api.anthropic.com".into(),
            max_tokens: 4096,
        }
    }
}

// --- Anthropic API types ---

#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
    #[allow(dead_code)]
    stop_reason: Option<String>,
}

/// Anthropic Claude model provider.
pub struct AnthropicProvider {
    config: AnthropicConfig,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(config: AnthropicConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("reqwest client should build");
        Self { config, client }
    }

    /// Convert AgentZero ChatMessages to Anthropic format.
    ///
    /// Anthropic separates the system prompt from messages and uses
    /// a different role naming convention.
    fn convert_messages(
        &self,
        messages: &[ChatMessage],
    ) -> (Option<String>, Vec<AnthropicMessage>) {
        let mut system = None;
        let mut anthropic_messages = Vec::new();

        for msg in messages {
            match msg.role.as_str() {
                "system" => {
                    system = Some(msg.content.clone());
                }
                "user" => {
                    anthropic_messages.push(AnthropicMessage {
                        role: "user".into(),
                        content: AnthropicContent::Text(msg.content.clone()),
                    });
                }
                "assistant" => {
                    anthropic_messages.push(AnthropicMessage {
                        role: "assistant".into(),
                        content: AnthropicContent::Text(msg.content.clone()),
                    });
                }
                "tool" => {
                    // Anthropic expects tool results as user messages with tool_result blocks
                    anthropic_messages.push(AnthropicMessage {
                        role: "user".into(),
                        content: AnthropicContent::Text(msg.content.clone()),
                    });
                }
                _ => {
                    anthropic_messages.push(AnthropicMessage {
                        role: "user".into(),
                        content: AnthropicContent::Text(msg.content.clone()),
                    });
                }
            }
        }

        (system, anthropic_messages)
    }

    /// Convert tool definitions to Anthropic format.
    fn convert_tools(&self, tools: &[ToolDefinition]) -> Vec<AnthropicTool> {
        tools
            .iter()
            .map(|t| AnthropicTool {
                name: t.function.name.clone(),
                description: t.function.description.clone(),
                input_schema: t.function.parameters.clone(),
            })
            .collect()
    }

    async fn chat_impl(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<ChatResult, ModelProviderError> {
        let url = format!("{}/v1/messages", self.config.base_url);
        let (system, anthropic_messages) = self.convert_messages(messages);

        let anthropic_tools = tools.map(|ts| self.convert_tools(ts));

        let request = MessagesRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            system,
            messages: anthropic_messages,
            tools: anthropic_tools,
        };

        debug!(
            model = %self.config.model,
            messages = messages.len(),
            "sending anthropic messages request"
        );

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    ModelProviderError::Unavailable(format!(
                        "cannot connect to Anthropic API at {}",
                        self.config.base_url
                    ))
                } else if e.is_timeout() {
                    ModelProviderError::Unavailable("request timed out".into())
                } else {
                    ModelProviderError::Unavailable(format!("request failed: {e}"))
                }
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ModelProviderError::Failed(format!(
                "Anthropic API returned {status}: {body}"
            )));
        }

        let resp: MessagesResponse = response
            .json()
            .await
            .map_err(|e| ModelProviderError::Failed(format!("failed to parse response: {e}")))?;

        // Extract text content and tool calls from content blocks
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in resp.content {
            match block {
                ContentBlock::Text { text } => text_parts.push(text),
                ContentBlock::ToolUse { name, input, .. } => {
                    tool_calls.push(ToolCall {
                        function: ToolCallFunction {
                            name,
                            arguments: input,
                        },
                    });
                }
                ContentBlock::ToolResult { .. } => {}
            }
        }

        let content = text_parts.join("");

        info!(
            model = %self.config.model,
            response_len = content.len(),
            tool_calls = tool_calls.len(),
            "received anthropic response"
        );

        Ok(ChatResult {
            content,
            tool_calls,
        })
    }
}

impl ModelProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn location(&self) -> ModelLocation {
        ModelLocation::Remote
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    fn health_check(
        &self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<bool, ModelProviderError>> + Send + '_>,
    > {
        Box::pin(async {
            // Anthropic doesn't have a dedicated health endpoint.
            // We check if the API key format looks valid and the base URL is reachable.
            if self.config.api_key.is_empty() {
                warn!("anthropic API key is empty");
                return Ok(false);
            }

            let url = format!("{}/v1/messages", self.config.base_url);
            debug!(url = %url, "checking anthropic health via HEAD");

            // A simple connectivity check — don't burn tokens
            match self.client.head(&url).send().await {
                Ok(_) => {
                    info!(model = %self.config.model, "anthropic API reachable");
                    Ok(true)
                }
                Err(e) => {
                    debug!(error = %e, "anthropic API not reachable");
                    Err(ModelProviderError::Unavailable(e.to_string()))
                }
            }
        })
    }

    fn chat_with_tools<'a>(
        &'a self,
        messages: &'a [ChatMessage],
        tools: Option<&'a [ToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ChatResult, ModelProviderError>> + Send + 'a>,
    > {
        Box::pin(async move { self.chat_impl(messages, tools).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::DataClassification;

    #[test]
    fn sonnet_config() {
        let config = AnthropicConfig::sonnet("sk-ant-test");
        assert_eq!(config.model, "claude-sonnet-4-20250514");
        assert_eq!(config.base_url, "https://api.anthropic.com");
        assert_eq!(config.max_tokens, 4096);
    }

    #[test]
    fn haiku_config() {
        let config = AnthropicConfig::haiku("sk-ant-test");
        assert!(config.model.contains("haiku"));
    }

    #[test]
    fn anthropic_is_remote() {
        let provider = AnthropicProvider::new(AnthropicConfig::sonnet("test"));
        assert_eq!(provider.location(), ModelLocation::Remote);
        assert_eq!(provider.name(), "anthropic");
    }

    #[test]
    fn remote_provider_restricts_secret_classification() {
        let provider = AnthropicProvider::new(AnthropicConfig::sonnet("test"));
        assert!(!provider.accepts_classification(DataClassification::Secret));
        assert!(!provider.accepts_classification(DataClassification::Credential));
        assert!(provider.accepts_classification(DataClassification::Public));
    }

    #[test]
    fn convert_messages_extracts_system() {
        let provider = AnthropicProvider::new(AnthropicConfig::sonnet("test"));
        let messages = vec![
            ChatMessage::system("You are helpful"),
            ChatMessage::user("Hello"),
            ChatMessage::assistant("Hi there"),
        ];
        let (system, msgs) = provider.convert_messages(&messages);
        assert_eq!(system, Some("You are helpful".to_string()));
        assert_eq!(msgs.len(), 2); // user + assistant (system extracted)
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
    }

    #[test]
    fn convert_tools_formats_correctly() {
        let provider = AnthropicProvider::new(AnthropicConfig::sonnet("test"));
        let tools = vec![ToolDefinition {
            tool_type: "function".into(),
            function: crate::ollama::ToolFunctionDef {
                name: "read".into(),
                description: "Read a file".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
        }];
        let converted = provider.convert_tools(&tools);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].name, "read");
    }

    #[test]
    fn empty_api_key_health_returns_false() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let provider = AnthropicProvider::new(AnthropicConfig {
            api_key: String::new(),
            model: "claude-sonnet-4-20250514".into(),
            base_url: "https://api.anthropic.com".into(),
            max_tokens: 4096,
        });
        let result = rt.block_on(provider.health_check());
        assert_eq!(result.expect("should not error"), false);
    }
}
