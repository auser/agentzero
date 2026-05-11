//! Multi-provider routing for AgentZero.
//!
//! Routes chat requests through providers in priority order: local first,
//! then remote with automatic redaction based on data classification.

use agentzero_core::DataClassification;
use agentzero_tracing::{debug, info, warn};

use crate::anthropic::{AnthropicConfig, AnthropicProvider};
use crate::models_config::{ModelsConfig, ProviderType};
use crate::ollama::{ChatMessage, ChatResult, OllamaConfig, OllamaProvider, ToolDefinition};
use crate::openai_compat::{OpenAICompatConfig, OpenAICompatProvider};
use crate::provider::{ModelLocation, ModelProvider, ModelProviderError};

/// A configured provider entry with priority metadata.
struct ProviderEntry {
    provider: Box<dyn ModelProvider>,
    #[allow(dead_code)]
    priority: u8, // lower = higher priority
}

/// Multi-provider router that tries providers in priority order.
pub struct ProviderRouter {
    providers: Vec<ProviderEntry>,
}

impl ProviderRouter {
    /// Create a router with a single local Ollama provider.
    pub fn local_only(model: &str) -> Self {
        let ollama = OllamaProvider::new(OllamaConfig {
            model: model.to_string(),
            ..OllamaConfig::default()
        });
        Self {
            providers: vec![ProviderEntry {
                provider: Box::new(ollama),
                priority: 0,
            }],
        }
    }

    /// Create a router that tries local Ollama first, then falls back to an OpenAI-compatible provider.
    pub fn with_fallback(local_model: &str, remote_config: OpenAICompatConfig) -> Self {
        let ollama = OllamaProvider::new(OllamaConfig {
            model: local_model.to_string(),
            ..OllamaConfig::default()
        });
        let openai = OpenAICompatProvider::new(remote_config);

        Self {
            providers: vec![
                ProviderEntry {
                    provider: Box::new(ollama),
                    priority: 0,
                },
                ProviderEntry {
                    provider: Box::new(openai),
                    priority: 1,
                },
            ],
        }
    }

    /// Create a router from a `ModelsConfig` (loaded from `models.json`).
    pub fn from_config(config: &ModelsConfig) -> Result<Self, String> {
        let mut providers = Vec::new();

        for (i, pc) in config.providers.iter().enumerate() {
            let provider: Box<dyn ModelProvider> = match pc.provider_type {
                ProviderType::Ollama => {
                    let cfg = OllamaConfig {
                        base_url: pc.url.clone(),
                        model: pc.default_model.clone(),
                    };
                    Box::new(OllamaProvider::new(cfg))
                }
                ProviderType::OpenAICompatible => {
                    let cfg = OpenAICompatConfig {
                        base_url: pc.url.clone(),
                        model: pc.default_model.clone(),
                        is_local: pc.is_local,
                        api_key: pc.api_key.clone(),
                    };
                    Box::new(OpenAICompatProvider::new(cfg))
                }
                ProviderType::Anthropic => {
                    let api_key = pc.api_key.clone().unwrap_or_default();
                    let cfg = AnthropicConfig {
                        api_key,
                        model: pc.default_model.clone(),
                        base_url: pc.url.clone(),
                        max_tokens: 4096,
                    };
                    Box::new(AnthropicProvider::new(cfg))
                }
            };
            providers.push(ProviderEntry {
                provider,
                priority: i as u8,
            });
        }

        if providers.is_empty() {
            return Err("no providers configured in models.json".into());
        }

        Ok(Self { providers })
    }

    /// Check which providers are available.
    pub async fn health_check(&self) -> Vec<(String, bool)> {
        let mut results = Vec::new();
        for entry in &self.providers {
            let ok = entry.provider.health_check().await.unwrap_or(false);
            results.push((entry.provider.name().to_string(), ok));
        }
        results
    }

    /// Send a chat request, trying providers in priority order.
    ///
    /// If the primary provider fails, falls back to the next one.
    /// Remote providers are only used if the classification allows it.
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[ToolDefinition]>,
        classification: DataClassification,
    ) -> Result<ChatResult, ModelProviderError> {
        for entry in &self.providers {
            // Skip remote providers for restricted classifications
            if entry.provider.location() == ModelLocation::Remote
                && classification.denies_remote()
            {
                debug!(
                    provider = %entry.provider.name(),
                    "skipping remote provider for {:?} classification",
                    classification
                );
                continue;
            }

            match entry.provider.chat_with_tools(messages, tools).await {
                Ok(chat_result) => {
                    info!(provider = %entry.provider.name(), "chat succeeded");
                    return Ok(chat_result);
                }
                Err(ModelProviderError::Unavailable(ref msg)) => {
                    warn!(
                        provider = %entry.provider.name(),
                        error = %msg,
                        "provider unavailable, trying next"
                    );
                    continue;
                }
                Err(e) => {
                    warn!(provider = %entry.provider.name(), error = %e, "provider error");
                    continue;
                }
            }
        }

        Err(ModelProviderError::Unavailable(
            "all providers unavailable".into(),
        ))
    }

    /// Get the name of the primary (highest priority) provider.
    pub fn primary_name(&self) -> &str {
        self.providers
            .first()
            .map(|e| e.provider.name())
            .unwrap_or("none")
    }

    /// Get the model name from the primary provider.
    pub fn model_name(&self) -> &str {
        self.providers
            .first()
            .map(|e| e.provider.model_name())
            .unwrap_or("unknown")
    }

    /// List all configured providers with their model names and active status.
    pub fn list_models(&self) -> Vec<ModelInfo> {
        self.providers
            .iter()
            .enumerate()
            .map(|(i, e)| ModelInfo {
                name: e.provider.model_name().to_string(),
                provider: e.provider.name().to_string(),
                is_local: e.provider.location() == ModelLocation::Local,
                active: i == 0,
            })
            .collect()
    }
}

/// Information about a configured model.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub provider: String,
    pub is_local: bool,
    pub active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_only_router() {
        let router = ProviderRouter::local_only("llama3.2");
        assert_eq!(router.primary_name(), "ollama");
        assert_eq!(router.model_name(), "llama3.2");
    }

    #[test]
    fn with_fallback_router() {
        let router = ProviderRouter::with_fallback(
            "llama3.2",
            OpenAICompatConfig {
                base_url: "http://localhost:8080".into(),
                model: "codellama".into(),
                is_local: true,
                api_key: None,
            },
        );
        assert_eq!(router.primary_name(), "ollama");
        assert_eq!(router.providers.len(), 2);
    }

    #[test]
    fn remote_skipped_for_secret_classification() {
        assert!(DataClassification::Secret.denies_remote());
        assert!(DataClassification::Credential.denies_remote());
        assert!(!DataClassification::Public.denies_remote());
    }

    #[test]
    fn list_models_returns_entries() {
        let router = ProviderRouter::with_fallback(
            "llama3.2",
            OpenAICompatConfig {
                base_url: "http://localhost:8080".into(),
                model: "codellama".into(),
                is_local: true,
                api_key: None,
            },
        );
        let models = router.list_models();
        assert_eq!(models.len(), 2);
        assert!(models[0].active);
        assert!(!models[1].active);
        assert_eq!(models[0].name, "llama3.2");
        assert_eq!(models[1].name, "codellama");
    }

    #[test]
    fn from_config_creates_router() {
        let config = ModelsConfig {
            providers: vec![
                crate::models_config::ProviderConfig {
                    name: "ollama".into(),
                    provider_type: ProviderType::Ollama,
                    url: "http://localhost:11434".into(),
                    default_model: "llama3.2".into(),
                    is_local: true,
                    api_key: None,
                },
                crate::models_config::ProviderConfig {
                    name: "lm-studio".into(),
                    provider_type: ProviderType::OpenAICompatible,
                    url: "http://localhost:1234".into(),
                    default_model: "gemma-4".into(),
                    is_local: true,
                    api_key: Some("lm-studio".into()),
                },
            ],
        };
        let router = ProviderRouter::from_config(&config).expect("should create");
        assert_eq!(router.providers.len(), 2);
        assert_eq!(router.model_name(), "llama3.2");
        let models = router.list_models();
        assert_eq!(models[1].name, "gemma-4");
    }

    #[test]
    fn from_config_empty_fails() {
        let config = ModelsConfig {
            providers: vec![],
        };
        assert!(ProviderRouter::from_config(&config).is_err());
    }
}
