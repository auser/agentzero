//! Ollama model provider for local LLM inference.
//!
//! Connects to a local Ollama server (default: http://localhost:11434)
//! via its REST API. All calls stay local — no data leaves the machine.
//!
//! Supports:
//! - Chat completions (non-streaming and streaming)
//! - Tool calling (model can request tool invocations)

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
            tool_calls: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            tool_calls: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
            tool_calls: None,
        }
    }

    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: content.into(),
            tool_calls: None,
        }
    }
}

/// A tool call requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub function: ToolCallFunction,
}

/// Function details within a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Tool definition sent to Ollama.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ToolFunctionDef,
}

/// Function definition within a tool.
#[derive(Debug, Clone, Serialize)]
pub struct ToolFunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    message: ChatResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCall>>,
}

/// A streaming chunk from Ollama.
#[derive(Debug, Deserialize)]
struct StreamChunk {
    message: StreamChunkMessage,
    #[serde(default)]
    done: bool,
}

#[derive(Debug, Deserialize)]
struct StreamChunkMessage {
    #[serde(default)]
    content: String,
}

/// Response from a chat request — may contain text, tool calls, or both.
#[derive(Debug, Clone)]
pub struct ChatResult {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

impl ChatResult {
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
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
            .timeout(std::time::Duration::from_secs(300))
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

    /// Send a chat completion request to Ollama (non-streaming).
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
        let url = format!("{}/api/chat", self.config.base_url);

        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: messages.to_vec(),
            stream: false,
            tools: tools.map(|t| t.to_vec()),
        };

        debug!(
            model = %self.config.model,
            messages = messages.len(),
            tools = tools.map_or(0, |t| t.len()),
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

        let tool_calls = chat_resp.message.tool_calls.unwrap_or_default();

        info!(
            model = %self.config.model,
            response_len = chat_resp.message.content.len(),
            tool_calls = tool_calls.len(),
            "received chat response"
        );

        Ok(ChatResult {
            content: chat_resp.message.content,
            tool_calls,
        })
    }

    /// Send a streaming chat request, calling `on_token` for each chunk.
    pub async fn chat_streaming<F>(
        &self,
        messages: &[ChatMessage],
        mut on_token: F,
    ) -> Result<String, ModelProviderError>
    where
        F: FnMut(&str),
    {
        let url = format!("{}/api/chat", self.config.base_url);

        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: messages.to_vec(),
            stream: true,
            tools: None,
        };

        debug!(
            model = %self.config.model,
            messages = messages.len(),
            "sending streaming chat request"
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

        let mut full_response = String::new();
        let body = response.text().await.map_err(|e| {
            ModelProviderError::Failed(format!("failed to read streaming response: {e}"))
        })?;

        // Ollama streaming returns newline-delimited JSON
        for line in body.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<StreamChunk>(line) {
                Ok(chunk) => {
                    if !chunk.message.content.is_empty() {
                        on_token(&chunk.message.content);
                        full_response.push_str(&chunk.message.content);
                    }
                    if chunk.done {
                        break;
                    }
                }
                Err(e) => {
                    warn!(error = %e, "failed to parse streaming chunk");
                }
            }
        }

        info!(
            model = %self.config.model,
            response_len = full_response.len(),
            "streaming response complete"
        );

        Ok(full_response)
    }

    /// Return the configured model name.
    pub fn model_name(&self) -> &str {
        &self.config.model
    }

    /// Return the built-in tool definitions for AgentZero tools.
    pub fn agentzero_tool_definitions() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                tool_type: "function".into(),
                function: ToolFunctionDef {
                    name: "read".into(),
                    description: "Read the contents of a file".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Path to the file to read"
                            }
                        },
                        "required": ["path"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".into(),
                function: ToolFunctionDef {
                    name: "list".into(),
                    description: "List the contents of a directory".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Path to the directory to list (defaults to current directory)"
                            }
                        }
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".into(),
                function: ToolFunctionDef {
                    name: "search".into(),
                    description: "Search for a text pattern in files within a directory".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "pattern": {
                                "type": "string",
                                "description": "Text pattern to search for"
                            },
                            "path": {
                                "type": "string",
                                "description": "Directory to search in (defaults to current directory)"
                            }
                        },
                        "required": ["pattern"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".into(),
                function: ToolFunctionDef {
                    name: "write".into(),
                    description: "Write content to a file (requires user approval)".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Path to the file to write"
                            },
                            "content": {
                                "type": "string",
                                "description": "Content to write to the file"
                            }
                        },
                        "required": ["path", "content"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".into(),
                function: ToolFunctionDef {
                    name: "shell".into(),
                    description: "Execute a shell command (requires user approval)".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "command": {
                                "type": "string",
                                "description": "Shell command to execute"
                            }
                        },
                        "required": ["command"]
                    }),
                },
            },
        ]
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

    #[test]
    fn tool_definitions_are_valid() {
        let tools = OllamaProvider::agentzero_tool_definitions();
        assert_eq!(tools.len(), 5);
        assert_eq!(tools[0].function.name, "read");
        assert_eq!(tools[1].function.name, "list");
        assert_eq!(tools[2].function.name, "search");
        assert_eq!(tools[3].function.name, "write");
        assert_eq!(tools[4].function.name, "shell");
    }

    #[test]
    fn chat_message_constructors() {
        let sys = ChatMessage::system("test");
        assert_eq!(sys.role, "system");
        assert_eq!(sys.content, "test");

        let user = ChatMessage::user("hello");
        assert_eq!(user.role, "user");

        let asst = ChatMessage::assistant("hi");
        assert_eq!(asst.role, "assistant");

        let tool = ChatMessage::tool("result");
        assert_eq!(tool.role, "tool");
    }

    #[test]
    fn chat_result_detects_tool_calls() {
        let result = ChatResult {
            content: String::new(),
            tool_calls: vec![ToolCall {
                function: ToolCallFunction {
                    name: "read".into(),
                    arguments: serde_json::json!({"path": "Cargo.toml"}),
                },
            }],
        };
        assert!(result.has_tool_calls());

        let no_tools = ChatResult {
            content: "hello".into(),
            tool_calls: vec![],
        };
        assert!(!no_tools.has_tool_calls());
    }
}
