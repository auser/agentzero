use std::sync::LazyLock;

use serde::Deserialize;

use crate::find_provider;

/// Embedded model catalog JSON, compiled into the binary.
const CATALOG_JSON: &str = include_str!("../data/model_catalog.json");

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ModelDescriptor {
    pub id: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(flatten)]
    pub capabilities: ModelCapabilities,
}

/// Capability flags for a model. Used by the agent loop to skip unsupported
/// features (e.g. don't send tool definitions to models that don't support
/// tool_use) and by `models status` to display what each model can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct ModelCapabilities {
    /// Model supports vision / image content blocks.
    #[serde(default)]
    pub vision: bool,
    /// Model supports tool use (function calling).
    #[serde(default = "default_true")]
    pub tool_use: bool,
    /// Model supports streaming responses.
    #[serde(default = "default_true")]
    pub streaming: bool,
    /// Maximum output tokens (0 = unknown / use provider default).
    #[serde(default)]
    pub max_output_tokens: u32,
}

fn default_true() -> bool {
    true
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            vision: false,
            tool_use: true,
            streaming: true,
            max_output_tokens: 0,
        }
    }
}

impl ModelCapabilities {
    pub const fn full(max_output_tokens: u32) -> Self {
        Self {
            vision: true,
            tool_use: true,
            streaming: true,
            max_output_tokens,
        }
    }

    pub const fn local() -> Self {
        Self {
            vision: false,
            tool_use: true,
            streaming: true,
            max_output_tokens: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Parsed catalog (lazily initialized from embedded JSON)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ProviderModels {
    openrouter: Vec<ModelDescriptor>,
    openai: Vec<ModelDescriptor>,
    anthropic: Vec<ModelDescriptor>,
    gemini: Vec<ModelDescriptor>,
    local_gguf: Vec<ModelDescriptor>,
    ollama: Vec<ModelDescriptor>,
}

#[derive(Deserialize)]
struct Catalog {
    providers: ProviderModels,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(CATALOG_JSON).expect("embedded model_catalog.json is valid")
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn find_models_for_provider(
    provider_or_alias: &str,
) -> Option<(&'static str, &'static [ModelDescriptor])> {
    let provider = find_provider(provider_or_alias)?;
    let catalog = &*CATALOG;
    let models: &[ModelDescriptor] = match provider.id {
        "openrouter" => &catalog.providers.openrouter,
        "openai" | "openai-codex" | "copilot" => &catalog.providers.openai,
        "anthropic" => &catalog.providers.anthropic,
        "gemini" => &catalog.providers.gemini,
        "candle" | "builtin" => &catalog.providers.local_gguf,
        "ollama" | "llamacpp" | "lmstudio" | "vllm" | "sglang" | "osaurus" => {
            &catalog.providers.ollama
        }
        _ => &catalog.providers.openrouter,
    };
    Some((provider.id, models))
}

/// Look up capabilities for a specific model on a specific provider.
/// Returns `None` if the provider or model is not in the catalog.
pub fn model_capabilities(provider: &str, model: &str) -> Option<ModelCapabilities> {
    let (_, models) = find_models_for_provider(provider)?;
    models
        .iter()
        .find(|m| m.id == model)
        .map(|m| m.capabilities)
}

/// Check if a provider is known to support a given model.
/// Returns `true` if the provider is unknown (permissive for custom providers)
/// or if the model is found in the provider's catalog.
pub fn provider_supports_model(provider: &str, model: &str) -> bool {
    match find_models_for_provider(provider) {
        Some((_, models)) => models.iter().any(|m| m.id == model),
        None => true, // Unknown provider -- allow any model
    }
}

/// Compute a lightweight fingerprint of provider config for cache invalidation.
/// When the fingerprint changes, any cached model list should be refreshed.
pub fn provider_config_fingerprint(kind: &str, base_url: &str, model: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    kind.hash(&mut hasher);
    base_url.hash(&mut hasher);
    model.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::{
        find_models_for_provider, model_capabilities, provider_config_fingerprint,
        provider_supports_model, ModelCapabilities,
    };

    #[test]
    fn find_models_resolves_alias_success_path() {
        let (provider, models) =
            find_models_for_provider("github-copilot").expect("alias should resolve");
        assert_eq!(provider, "copilot");
        assert!(!models.is_empty());
    }

    #[test]
    fn find_models_returns_none_for_unknown_provider_negative_path() {
        assert!(find_models_for_provider("missing-provider").is_none());
    }

    #[test]
    fn anthropic_models_have_full_capabilities() {
        let (_, models) = find_models_for_provider("anthropic").expect("anthropic should resolve");
        for model in models {
            assert!(model.capabilities.vision, "{} should have vision", model.id);
            assert!(
                model.capabilities.tool_use,
                "{} should have tool_use",
                model.id
            );
            assert!(
                model.capabilities.streaming,
                "{} should have streaming",
                model.id
            );
            assert!(
                model.capabilities.max_output_tokens > 0,
                "{} should have max_output_tokens",
                model.id
            );
        }
    }

    #[test]
    fn local_models_have_limited_vision_but_support_tools() {
        let (_, models) = find_models_for_provider("ollama").expect("ollama should resolve");
        for model in models {
            assert!(
                !model.capabilities.vision,
                "{} should not have vision",
                model.id
            );
            assert!(
                model.capabilities.tool_use,
                "{} should have tool_use",
                model.id
            );
        }
    }

    #[test]
    fn builtin_models_have_tool_use_and_no_vision() {
        let (_, models) = find_models_for_provider("builtin").expect("builtin should resolve");
        assert!(models.len() > 1, "builtin should have multiple models");
        for model in models {
            assert!(
                !model.capabilities.vision,
                "{} should not have vision",
                model.id
            );
            assert!(
                model.capabilities.tool_use,
                "{} should have tool_use",
                model.id
            );
        }
    }

    #[cfg(any(feature = "local-model", feature = "candle"))]
    #[test]
    fn every_builtin_model_has_gguf_registry_entry() {
        let (_, models) = find_models_for_provider("builtin").expect("builtin should resolve");
        for model in models {
            assert!(
                crate::model_manager::resolve_model(&model.id).is_some(),
                "builtin model '{}' missing from GGUF registry",
                model.id
            );
        }
    }

    #[test]
    fn model_capabilities_returns_some_for_known_model() {
        let caps =
            model_capabilities("anthropic", "claude-sonnet-4-20250514").expect("should find");
        assert!(caps.vision);
        assert!(caps.tool_use);
    }

    #[test]
    fn model_capabilities_returns_none_for_unknown_model() {
        assert!(model_capabilities("anthropic", "unknown-model").is_none());
    }

    #[test]
    fn model_capabilities_returns_none_for_unknown_provider() {
        assert!(model_capabilities("unknown-provider", "claude-sonnet-4-20250514").is_none());
    }

    #[test]
    fn default_capabilities_enable_tool_use() {
        let caps = ModelCapabilities::default();
        assert!(!caps.vision);
        assert!(caps.tool_use);
        assert!(caps.streaming);
        assert_eq!(caps.max_output_tokens, 0);
    }

    // --- Cache invalidation ---

    #[test]
    fn config_fingerprint_changes_with_provider() {
        let fp1 = provider_config_fingerprint("openai", "https://api.openai.com", "gpt-4o-mini");
        let fp2 = provider_config_fingerprint("anthropic", "https://api.openai.com", "gpt-4o-mini");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn config_fingerprint_changes_with_model() {
        let fp1 = provider_config_fingerprint("openai", "https://api.openai.com", "gpt-4o-mini");
        let fp2 = provider_config_fingerprint("openai", "https://api.openai.com", "gpt-4.1");
        assert_ne!(fp1, fp2);
    }

    // --- provider_supports_model ---

    #[test]
    fn provider_supports_model_known_model() {
        assert!(provider_supports_model(
            "anthropic",
            "claude-sonnet-4-20250514"
        ));
    }

    #[test]
    fn provider_supports_model_unknown_model_on_known_provider() {
        assert!(!provider_supports_model("anthropic", "nonexistent-model"));
    }

    #[test]
    fn provider_supports_model_unknown_provider_is_permissive() {
        assert!(provider_supports_model("my-custom-provider", "any-model"));
    }

    #[test]
    fn config_fingerprint_stable_for_same_config() {
        let fp1 = provider_config_fingerprint("openai", "https://api.openai.com", "gpt-4o-mini");
        let fp2 = provider_config_fingerprint("openai", "https://api.openai.com", "gpt-4o-mini");
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn catalog_json_parses_successfully() {
        // Force initialization of the lazy catalog
        let _ = find_models_for_provider("openai");
    }
}
