use std::pin::Pin;

use agentzero_core::DataClassification;
use thiserror::Error;

use crate::ollama::{ChatMessage, ChatResult, ToolDefinition};

#[derive(Debug, Error)]
pub enum ModelProviderError {
    #[error("model call denied: {0}")]
    Denied(String),
    #[error("model unavailable: {0}")]
    Unavailable(String),
    #[error("model error: {0}")]
    Failed(String),
}

/// Whether a model provider is local or remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelLocation {
    Local,
    Remote,
}

/// A model provider that can generate completions.
///
/// This is the unified trait for all AI model providers. Implementations
/// must provide metadata (name, location, model_name) and chat capabilities
/// (chat_with_tools, health_check). Streaming is optional.
pub trait ModelProvider: Send + Sync {
    /// Human-readable name of the provider (e.g. "ollama", "openai-compatible").
    fn name(&self) -> &str;

    /// Whether this provider runs locally or remotely.
    fn location(&self) -> ModelLocation;

    /// The model name this provider is configured to use.
    fn model_name(&self) -> &str;

    /// Check whether a given data classification is safe to send to this provider.
    fn accepts_classification(&self, classification: DataClassification) -> bool {
        match self.location() {
            ModelLocation::Local => true,
            ModelLocation::Remote => classification.allows_remote_unredacted(),
        }
    }

    /// Check if the provider's backend server is reachable.
    fn health_check(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bool, ModelProviderError>> + Send + '_>>;

    /// Send a chat completion request with optional tool definitions.
    fn chat_with_tools<'a>(
        &'a self,
        messages: &'a [ChatMessage],
        tools: Option<&'a [ToolDefinition]>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ChatResult, ModelProviderError>> + Send + 'a>>;

    /// Send a streaming chat request, calling `on_token` for each chunk.
    ///
    /// Default implementation falls back to non-streaming `chat_with_tools`.
    fn chat_streaming<'a>(
        &'a self,
        messages: &'a [ChatMessage],
        on_token: Box<dyn FnMut(&str) + Send + 'a>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, ModelProviderError>> + Send + 'a>>
    {
        let _ = on_token;
        Box::pin(async move {
            let result = self.chat_with_tools(messages, None).await?;
            Ok(result.content)
        })
    }
}

/// A stub local model provider for testing and demo purposes.
///
/// Always returns a canned response. No network calls.
pub struct LocalStubProvider;

impl ModelProvider for LocalStubProvider {
    fn name(&self) -> &str {
        "local-stub"
    }

    fn location(&self) -> ModelLocation {
        ModelLocation::Local
    }

    fn model_name(&self) -> &str {
        "stub"
    }

    fn health_check(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bool, ModelProviderError>> + Send + '_>>
    {
        Box::pin(async { Ok(true) })
    }

    fn chat_with_tools<'a>(
        &'a self,
        _messages: &'a [ChatMessage],
        _tools: Option<&'a [ToolDefinition]>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ChatResult, ModelProviderError>> + Send + 'a>>
    {
        Box::pin(async {
            Ok(ChatResult {
                content: "I'm a stub provider. No real model is connected.".into(),
                tool_calls: vec![],
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_provider_accepts_all_classifications() {
        let provider = LocalStubProvider;
        assert!(provider.accepts_classification(DataClassification::Secret));
        assert!(provider.accepts_classification(DataClassification::Pii));
        assert!(provider.accepts_classification(DataClassification::Private));
        assert!(provider.accepts_classification(DataClassification::Public));
    }

    #[test]
    fn local_stub_is_local() {
        let provider = LocalStubProvider;
        assert_eq!(provider.location(), ModelLocation::Local);
        assert_eq!(provider.name(), "local-stub");
        assert_eq!(provider.model_name(), "stub");
    }

    #[tokio::test]
    async fn local_stub_health_check() {
        let provider = LocalStubProvider;
        assert!(provider.health_check().await.unwrap_or(false));
    }

    #[tokio::test]
    async fn local_stub_chat() {
        let provider = LocalStubProvider;
        let messages = vec![ChatMessage::user("hello")];
        let result = provider.chat_with_tools(&messages, None).await;
        assert!(result.is_ok());
        assert!(result.unwrap().content.contains("stub"));
    }
}
