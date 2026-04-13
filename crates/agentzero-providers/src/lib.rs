//! LLM provider abstraction for AgentZero.
//!
//! Implements the `Provider` trait for Anthropic and OpenAI-compatible APIs.
//! Handles streaming, tool-use message formatting, model catalog lookup,
//! and provider-specific quirks (reasoning tokens, system prompts).

// ---------------------------------------------------------------------------
// Compile-time feature guards
//
// These `compile_error!` blocks turn invalid feature combinations into clear
// build failures at `cargo check` time, instead of cryptic linker errors or
// silent runtime surprises.
// ---------------------------------------------------------------------------

#[cfg(all(feature = "candle-cuda", target_os = "macos"))]
compile_error!(
    "feature `candle-cuda` is not supported on macOS.\n\
     Reason: CUDA requires an NVIDIA GPU; Apple Silicon uses Metal.\n\
     Fix: build with `--features candle-metal` (Apple GPU) \
     or `--features candle` (CPU) instead."
);

#[cfg(all(
    feature = "candle-metal",
    not(any(target_os = "macos", target_os = "ios"))
))]
compile_error!(
    "feature `candle-metal` only works on Apple platforms.\n\
     Reason: Metal is an Apple-specific GPU API.\n\
     Fix: build with `--features candle-cuda` (NVIDIA) \
     or `--features candle` (CPU) on this target."
);

#[cfg(all(feature = "candle-metal", feature = "candle-cuda"))]
compile_error!(
    "features `candle-metal` and `candle-cuda` are mutually exclusive.\n\
     Reason: Candle links exactly one GPU backend per build.\n\
     Fix: pick one — `--features candle-metal` (Apple) \
     or `--features candle-cuda` (NVIDIA)."
);

#[cfg(all(feature = "candle", target_arch = "wasm32"))]
compile_error!(
    "feature `candle` is not supported on wasm32.\n\
     Reason: Candle's tensor backend has no wasm32 support; \
     local LLM inference cannot run in a browser bundle.\n\
     Fix: use a remote provider (`anthropic`, `openai`, etc.) on wasm32."
);

#[cfg(all(feature = "local-model", target_arch = "wasm32"))]
compile_error!(
    "feature `local-model` is not supported on wasm32.\n\
     Reason: `local-model` links llama.cpp via `llama-cpp-2`, which requires \
     a native C++ toolchain and is not available on wasm32.\n\
     Fix: use a remote provider on wasm32."
);

mod anthropic;
#[cfg(feature = "local-model")]
pub mod builtin;
#[cfg(feature = "candle")]
pub mod candle_embedding;
#[cfg(feature = "candle")]
pub mod candle_provider;
mod catalog;
#[cfg(feature = "candle")]
pub mod constrained;
pub mod credential_pool;
pub mod embedding;
mod fallback;
pub mod guardrails;
pub mod local_tools;
#[cfg(any(feature = "local-model", feature = "candle"))]
pub mod model_manager;
mod models;
mod openai;
pub mod pipeline;
mod pricing;
pub mod privacy_layer;
pub mod provider_metrics;
pub(crate) mod transport;

#[cfg(feature = "privacy")]
mod noise_transport;

pub use anthropic::AnthropicProvider;
pub use catalog::{find_provider, supported_providers, ProviderDescriptor};
pub use fallback::{FallbackInfo, FallbackProvider, FALLBACK_INFO};
pub use guardrails::{
    Enforcement, Guard, GuardEntry, GuardVerdict, GuardrailsLayer, PiiRedactionGuard,
    PromptInjectionGuard,
};
pub use models::{
    find_models_for_provider, model_capabilities, provider_config_fingerprint,
    provider_supports_model, ModelCapabilities, ModelDescriptor,
};
pub use openai::OpenAiCompatibleProvider;
pub use pipeline::{
    CostCapLayer, CostEstimateLayer, LlmLayer, MetricsLayer, PipelineBuilder, PromptCacheLayer,
};
pub use pricing::{compute_cost_microdollars, model_pricing, ModelPricing};
pub use transport::{
    health_probe, CircuitBreaker, CircuitBreakerStatus, CooldownState, HealthProbeResult,
    TransportConfig,
};

#[cfg(feature = "privacy")]
pub use noise_transport::perform_noise_handshake;

/// Build an OpenAI-compatible provider with Noise-encrypted transport.
///
/// The provider sends all requests through the Noise session, adding
/// `X-Noise-Session` header and encrypting/decrypting request/response bodies.
/// Only works for OpenAI-compatible endpoints (gateways), not Anthropic.
#[cfg(feature = "privacy")]
pub fn build_provider_with_noise(
    base_url: String,
    api_key: String,
    model: String,
    session: agentzero_core::privacy::noise_client::NoiseClientSession,
) -> Box<dyn agentzero_core::Provider> {
    let transport = std::sync::Arc::new(noise_transport::NoiseHttpTransport::new(session));
    Box::new(OpenAiCompatibleProvider::with_transport(
        base_url, api_key, model, transport,
    ))
}

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

/// Build a builtin (in-process llama.cpp) provider.
///
/// Available only when compiled with the `local-model` feature.
/// The `base_url` and `api_key` parameters are ignored.
#[cfg(feature = "local-model")]
pub fn build_builtin_provider(model: String) -> Box<dyn agentzero_core::Provider> {
    Box::new(builtin::BuiltinProvider::new(model))
}

/// Build a Candle (in-process, pure Rust) local LLM provider.
///
/// Available only when compiled with the `candle` feature.
#[cfg(feature = "candle")]
pub fn build_candle_provider(
    config: candle_provider::CandleConfig,
) -> Box<dyn agentzero_core::Provider> {
    Box::new(candle_provider::CandleProvider::new(config))
}

/// Build a provider with privacy enforcement.
///
/// - `"local_only"` — rejects cloud providers entirely.
/// - `"encrypted"` / `"full"` — allows cloud providers (traffic goes through
///   Noise-encrypted transport). `"full"` auto-enables all privacy features
///   but does NOT restrict provider choice.
/// - `"off"` — no restrictions.
pub fn build_provider_with_privacy(
    kind: &str,
    base_url: String,
    api_key: String,
    model: String,
    transport: TransportConfig,
    privacy_mode: &str,
) -> anyhow::Result<Box<dyn agentzero_core::Provider>> {
    if privacy_mode == "local_only"
        && !agentzero_core::common::local_providers::is_local_provider(kind)
    {
        anyhow::bail!(
            "privacy mode 'local_only' requires a local provider, \
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
        #[cfg(feature = "local-model")]
        "builtin" => build_builtin_provider(model),
        #[cfg(not(feature = "local-model"))]
        "builtin" => {
            tracing::error!("provider 'builtin' requires the 'local-model' feature — falling back to OpenAI-compatible stub");
            Box::new(OpenAiCompatibleProvider::new(base_url, api_key, model))
        }
        #[cfg(feature = "candle")]
        "candle" => build_candle_provider(candle_provider::CandleConfig::default()),
        #[cfg(not(feature = "candle"))]
        "candle" => {
            tracing::error!("provider 'candle' requires the 'candle' feature — falling back to OpenAI-compatible stub");
            Box::new(OpenAiCompatibleProvider::new(base_url, api_key, model))
        }
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
    fn build_provider_with_privacy_allows_cloud_in_full_mode() {
        // "full" enables all privacy features but routes cloud traffic through
        // encrypted transport — it does NOT block cloud providers.
        let result = build_provider_with_privacy(
            "openai",
            "https://api.openai.com".to_string(),
            "sk-test".to_string(),
            "gpt-4o".to_string(),
            default_transport(),
            "full",
        );
        assert!(result.is_ok());
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
