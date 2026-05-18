//! Configuration types for `models.json` — the single source of truth
//! for AI model providers.
//!
//! Both the CLI and ACP server load this to construct a `ProviderRouter`.

use serde::{Deserialize, Serialize};

/// Top-level models configuration (deserialized from `.agentzero/models.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsConfig {
    pub providers: Vec<ProviderConfig>,
}

/// A single provider entry in the models config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Human-readable name (e.g. "ollama", "lm-studio", "my-gpu-server").
    pub name: String,
    /// Provider type determines which client to instantiate.
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    /// Base URL for the provider's API.
    pub url: String,
    /// Default model to use with this provider.
    pub default_model: String,
    /// Whether this provider runs locally (affects data classification routing).
    #[serde(default = "default_true")]
    pub is_local: bool,
    /// Optional API key (some servers require it). Supports "vault:provider/key" references.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Driver to use for `Custom` provider type.
    /// Maps to an existing provider implementation: `"ollama"`, `"openai-compatible"`, or `"anthropic"`.
    /// Defaults to `"openai-compatible"` when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Supported provider types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderType {
    /// Native Ollama API (`/api/chat`).
    #[serde(rename = "ollama")]
    Ollama,
    /// Any OpenAI-compatible server (`/v1/chat/completions`).
    /// Works with llama.cpp, vLLM, LM Studio, LocalAI, text-gen-webui.
    #[serde(rename = "openai-compatible")]
    OpenAICompatible,
    /// Anthropic Claude API (`/v1/messages`).
    /// Always remote — PII redaction applies per ADR 0002.
    #[serde(rename = "anthropic")]
    Anthropic,
    /// Custom provider backed by an existing driver.
    /// Use the `driver` field in `ProviderConfig` to select the underlying
    /// client (`"ollama"`, `"openai-compatible"`, or `"anthropic"`).
    /// Defaults to `"openai-compatible"` when `driver` is omitted.
    /// This lets users add any compatible endpoint via `models.json`
    /// without code changes.
    #[serde(rename = "custom")]
    Custom,
}

impl ModelsConfig {
    /// Load from a JSON file path.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("failed to parse {}: {e}", path.display()))
    }

    /// Create a default config with a single local Ollama provider.
    pub fn default_ollama() -> Self {
        Self {
            providers: vec![ProviderConfig {
                name: "ollama".into(),
                provider_type: ProviderType::Ollama,
                url: "http://localhost:11434".into(),
                default_model: "llama3.2".into(),
                is_local: true,
                api_key: None,
                driver: None,
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_models_config() {
        let json = r#"{
            "providers": [
                {
                    "name": "ollama",
                    "type": "ollama",
                    "url": "http://localhost:11434",
                    "default_model": "llama3.2",
                    "is_local": true
                },
                {
                    "name": "lm-studio",
                    "type": "openai-compatible",
                    "url": "http://localhost:1234/v1",
                    "default_model": "google/gemma-4-26b",
                    "is_local": true,
                    "api_key": "lm-studio"
                }
            ]
        }"#;
        let config: ModelsConfig = serde_json::from_str(json).expect("should parse");
        assert_eq!(config.providers.len(), 2);
        assert_eq!(config.providers[0].provider_type, ProviderType::Ollama);
        assert_eq!(
            config.providers[1].provider_type,
            ProviderType::OpenAICompatible
        );
        assert_eq!(config.providers[1].api_key, Some("lm-studio".into()));
    }

    #[test]
    fn serialize_models_config() {
        let config = ModelsConfig::default_ollama();
        let json = serde_json::to_string_pretty(&config).expect("should serialize");
        assert!(json.contains("ollama"));
        assert!(json.contains("llama3.2"));
    }

    #[test]
    fn is_local_defaults_to_true() {
        let json = r#"{
            "providers": [{
                "name": "test",
                "type": "ollama",
                "url": "http://localhost:11434",
                "default_model": "llama3.2"
            }]
        }"#;
        let config: ModelsConfig = serde_json::from_str(json).expect("should parse");
        assert!(config.providers[0].is_local);
    }

    #[test]
    fn anthropic_provider_config() {
        let json = r#"{
            "providers": [{
                "name": "claude",
                "type": "anthropic",
                "url": "https://api.anthropic.com",
                "default_model": "claude-sonnet-4-20250514",
                "is_local": false,
                "api_key": "sk-ant-test"
            }]
        }"#;
        let config: ModelsConfig = serde_json::from_str(json).expect("should parse");
        assert_eq!(config.providers[0].provider_type, ProviderType::Anthropic);
        assert!(!config.providers[0].is_local);
    }

    #[test]
    fn remote_provider_config() {
        let json = r#"{
            "providers": [{
                "name": "cloud-api",
                "type": "openai-compatible",
                "url": "https://api.example.com",
                "default_model": "gpt-4",
                "is_local": false,
                "api_key": "sk-1234"
            }]
        }"#;
        let config: ModelsConfig = serde_json::from_str(json).expect("should parse");
        assert!(!config.providers[0].is_local);
        assert_eq!(config.providers[0].api_key, Some("sk-1234".into()));
    }

    #[test]
    fn custom_provider_with_driver() {
        let json = r#"{
            "providers": [{
                "name": "together-ai",
                "type": "custom",
                "driver": "openai-compatible",
                "url": "https://api.together.xyz/v1",
                "default_model": "meta-llama/Llama-3-70b",
                "is_local": false,
                "api_key": "vault:together/key"
            }]
        }"#;
        let config: ModelsConfig = serde_json::from_str(json).expect("should parse");
        assert_eq!(config.providers[0].provider_type, ProviderType::Custom);
        assert_eq!(config.providers[0].driver, Some("openai-compatible".into()));
        assert!(!config.providers[0].is_local);
    }

    #[test]
    fn custom_provider_without_driver_defaults_to_none() {
        let json = r#"{
            "providers": [{
                "name": "my-server",
                "type": "custom",
                "url": "http://my-gpu:8080/v1",
                "default_model": "my-model",
                "is_local": true
            }]
        }"#;
        let config: ModelsConfig = serde_json::from_str(json).expect("should parse");
        assert_eq!(config.providers[0].provider_type, ProviderType::Custom);
        assert_eq!(config.providers[0].driver, None);
    }
}
