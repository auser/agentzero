//! LLM provider abstraction for AgentZero.
//!
//! Implements the `Provider` trait for Anthropic and OpenAI-compatible APIs.
//! Handles streaming, tool-use message formatting, model catalog lookup,
//! and provider-specific quirks (reasoning tokens, system prompts).

mod anthropic;
mod catalog;
mod models;
mod openai;
pub(crate) mod transport;

pub use anthropic::AnthropicProvider;
pub use catalog::{find_provider, supported_providers, ProviderDescriptor};
pub use models::{
    find_models_for_provider, model_capabilities, provider_config_fingerprint, ModelCapabilities,
    ModelDescriptor,
};
pub use openai::OpenAiCompatibleProvider;
pub use transport::{health_probe, CircuitBreaker, HealthProbeResult, TransportConfig};

/// Declarative macro that generates the `build_provider` factory from a
/// list of `(kind-pattern => ProviderType)` entries. The last `_ =>` arm
/// is the catch-all for OpenAI-compatible providers.
macro_rules! register_providers {
    ( $( $pat:pat => $ty:ident ),+ $(,)? ) => {
        pub fn build_provider(
            kind: &str,
            base_url: String,
            api_key: String,
            model: String,
        ) -> Box<dyn agentzero_core::Provider> {
            match kind {
                $( $pat => Box::new($ty::new(base_url, api_key, model)), )+
            }
        }
    };
}

register_providers! {
    "anthropic" => AnthropicProvider,
    _ => OpenAiCompatibleProvider,
}

/// Build a provider with privacy enforcement. When `privacy_mode` is
/// `"local_only"` or `"full"`, rejects cloud providers with an error.
pub fn build_provider_with_privacy(
    kind: &str,
    base_url: String,
    api_key: String,
    model: String,
    transport: TransportConfig,
    privacy_mode: &str,
) -> anyhow::Result<Box<dyn agentzero_core::Provider>> {
    if matches!(privacy_mode, "local_only" | "full")
        && !agentzero_core::common::local_providers::is_local_provider(kind)
    {
        anyhow::bail!(
            "privacy mode '{privacy_mode}' requires a local provider, \
             but '{kind}' is a cloud provider"
        );
    }
    Ok(build_provider_with_transport(
        kind, base_url, api_key, model, transport,
    ))
}

/// Build a provider with explicit transport configuration from TOML.
pub fn build_provider_with_transport(
    kind: &str,
    base_url: String,
    api_key: String,
    model: String,
    transport: TransportConfig,
) -> Box<dyn agentzero_core::Provider> {
    match kind {
        "anthropic" => Box::new(AnthropicProvider::with_config(
            base_url, api_key, model, transport,
        )),
        _ => Box::new(OpenAiCompatibleProvider::new(base_url, api_key, model)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_transport() -> TransportConfig {
        TransportConfig {
            timeout_ms: 30_000,
            max_retries: 3,
            circuit_breaker_threshold: 5,
            circuit_breaker_reset_ms: 30_000,
        }
    }

    #[test]
    fn build_provider_with_privacy_allows_local_in_local_only() {
        let result = build_provider_with_privacy(
            "ollama",
            "http://localhost:11434".to_string(),
            String::new(),
            "llama3".to_string(),
            default_transport(),
            "local_only",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn build_provider_with_privacy_rejects_cloud_in_local_only() {
        let result = build_provider_with_privacy(
            "anthropic",
            "https://api.anthropic.com".to_string(),
            "sk-test".to_string(),
            "claude-sonnet-4-6".to_string(),
            default_transport(),
            "local_only",
        );
        let err = result.err().expect("should reject cloud provider");
        assert!(err.to_string().contains("local provider"), "error: {err}");
    }

    #[test]
    fn build_provider_with_privacy_rejects_cloud_in_full_mode() {
        let result = build_provider_with_privacy(
            "openai",
            "https://api.openai.com".to_string(),
            "sk-test".to_string(),
            "gpt-4o".to_string(),
            default_transport(),
            "full",
        );
        assert!(result.is_err());
    }

    #[test]
    fn build_provider_with_privacy_allows_cloud_in_off_mode() {
        let result = build_provider_with_privacy(
            "anthropic",
            "https://api.anthropic.com".to_string(),
            "sk-test".to_string(),
            "claude-sonnet-4-6".to_string(),
            default_transport(),
            "off",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn build_provider_with_privacy_allows_cloud_in_encrypted_mode() {
        let result = build_provider_with_privacy(
            "anthropic",
            "https://api.anthropic.com".to_string(),
            "sk-test".to_string(),
            "claude-sonnet-4-6".to_string(),
            default_transport(),
            "encrypted",
        );
        assert!(result.is_ok());
    }
}
