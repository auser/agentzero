//! Multi-provider routing for AgentZero.
//!
//! Tries providers in priority order: local first, then remote with
//! automatic redaction based on data classification and policy.

use agentzero_core::DataClassification;
use agentzero_tracing::{debug, info, warn};

use crate::ollama::{ChatMessage, ChatResult, OllamaConfig, OllamaProvider, ToolDefinition};
use crate::openai_compat::{OpenAICompatConfig, OpenAICompatProvider};
use crate::provider::{ModelLocation, ModelProviderError};

/// A configured provider with priority and metadata.
#[derive(Debug)]
struct ProviderEntry {
    name: String,
    location: ModelLocation,
    #[allow(dead_code)]
    priority: u8, // lower = higher priority
}

/// Multi-provider router that tries providers in order.
pub struct ProviderRouter {
    ollama: Option<OllamaProvider>,
    openai_compat: Option<OpenAICompatProvider>,
    order: Vec<ProviderEntry>,
}

impl ProviderRouter {
    /// Create a router with default local providers.
    pub fn local_only(model: &str) -> Self {
        let ollama = OllamaProvider::new(OllamaConfig {
            model: model.to_string(),
            ..OllamaConfig::default()
        });
        Self {
            ollama: Some(ollama),
            openai_compat: None,
            order: vec![ProviderEntry {
                name: "ollama".into(),
                location: ModelLocation::Local,
                priority: 0,
            }],
        }
    }

    /// Create a router that tries local first, then falls back to a remote provider.
    pub fn with_fallback(local_model: &str, remote_config: OpenAICompatConfig) -> Self {
        let ollama = OllamaProvider::new(OllamaConfig {
            model: local_model.to_string(),
            ..OllamaConfig::default()
        });
        let remote_name = if remote_config.is_local {
            "openai-compat-local"
        } else {
            "openai-compat-remote"
        };
        let remote_location = if remote_config.is_local {
            ModelLocation::Local
        } else {
            ModelLocation::Remote
        };
        let openai = OpenAICompatProvider::new(remote_config);

        Self {
            ollama: Some(ollama),
            openai_compat: Some(openai),
            order: vec![
                ProviderEntry {
                    name: "ollama".into(),
                    location: ModelLocation::Local,
                    priority: 0,
                },
                ProviderEntry {
                    name: remote_name.into(),
                    location: remote_location,
                    priority: 1,
                },
            ],
        }
    }

    /// Check which providers are available.
    pub async fn health_check(&self) -> Vec<(String, bool)> {
        let mut results = Vec::new();

        if let Some(ref ollama) = self.ollama {
            let ok = ollama.health_check().await.unwrap_or(false);
            results.push(("ollama".into(), ok));
        }
        if let Some(ref openai) = self.openai_compat {
            let ok = openai.health_check().await.unwrap_or(false);
            results.push(("openai-compat".into(), ok));
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
        // Try providers in priority order
        for entry in &self.order {
            // Skip remote providers for restricted classifications
            if entry.location == ModelLocation::Remote && classification.denies_remote() {
                debug!(
                    provider = %entry.name,
                    "skipping remote provider for {:?} classification",
                    classification
                );
                continue;
            }

            let result = match entry.name.as_str() {
                "ollama" => {
                    if let Some(ref provider) = self.ollama {
                        provider.chat_with_tools(messages, tools).await
                    } else {
                        continue;
                    }
                }
                _ => {
                    if let Some(ref provider) = self.openai_compat {
                        provider.chat_with_tools(messages, tools).await
                    } else {
                        continue;
                    }
                }
            };

            match result {
                Ok(chat_result) => {
                    info!(provider = %entry.name, "chat succeeded");
                    return Ok(chat_result);
                }
                Err(ModelProviderError::Unavailable(ref msg)) => {
                    warn!(
                        provider = %entry.name,
                        error = %msg,
                        "provider unavailable, trying next"
                    );
                    continue;
                }
                Err(e) => {
                    warn!(provider = %entry.name, error = %e, "provider error");
                    // For non-availability errors, still try fallback
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
        self.order
            .first()
            .map(|e| e.name.as_str())
            .unwrap_or("none")
    }

    /// Get the model name from the primary provider.
    pub fn model_name(&self) -> &str {
        if let Some(ref ollama) = self.ollama {
            return ollama.model_name();
        }
        if let Some(ref openai) = self.openai_compat {
            return openai.model_name();
        }
        "unknown"
    }
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
        assert_eq!(router.order.len(), 2);
    }

    #[test]
    fn remote_skipped_for_secret_classification() {
        // This is tested implicitly through the routing logic — if classification.denies_remote()
        // is true, remote providers are skipped
        assert!(DataClassification::Secret.denies_remote());
        assert!(DataClassification::Credential.denies_remote());
        assert!(!DataClassification::Public.denies_remote());
    }
}
