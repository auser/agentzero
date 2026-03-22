use crate::find_provider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelDescriptor {
    pub id: &'static str,
    pub is_default: bool,
    pub capabilities: ModelCapabilities,
}

/// Capability flags for a model. Used by the agent loop to skip unsupported
/// features (e.g. don't send tool definitions to models that don't support
/// tool_use) and by `models status` to display what each model can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelCapabilities {
    /// Model supports vision / image content blocks.
    pub vision: bool,
    /// Model supports tool use (function calling).
    pub tool_use: bool,
    /// Model supports streaming responses.
    pub streaming: bool,
    /// Maximum output tokens (0 = unknown / use provider default).
    pub max_output_tokens: u32,
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
    const fn full(max_output_tokens: u32) -> Self {
        Self {
            vision: true,
            tool_use: true,
            streaming: true,
            max_output_tokens,
        }
    }

    const fn local() -> Self {
        Self {
            vision: false,
            tool_use: true,
            streaming: true,
            max_output_tokens: 0,
        }
    }
}

const OPENROUTER_MODELS: &[ModelDescriptor] = &[
    ModelDescriptor {
        id: "anthropic/claude-3.5-sonnet",
        is_default: true,
        capabilities: ModelCapabilities::full(8192),
    },
    ModelDescriptor {
        id: "openai/gpt-4o-mini",
        is_default: false,
        capabilities: ModelCapabilities::full(16384),
    },
    ModelDescriptor {
        id: "google/gemini-1.5-pro",
        is_default: false,
        capabilities: ModelCapabilities::full(8192),
    },
];

const OPENAI_MODELS: &[ModelDescriptor] = &[
    ModelDescriptor {
        id: "gpt-4o-mini",
        is_default: true,
        capabilities: ModelCapabilities::full(16384),
    },
    ModelDescriptor {
        id: "gpt-4.1-mini",
        is_default: false,
        capabilities: ModelCapabilities::full(32768),
    },
    ModelDescriptor {
        id: "gpt-4.1",
        is_default: false,
        capabilities: ModelCapabilities::full(32768),
    },
];

const ANTHROPIC_MODELS: &[ModelDescriptor] = &[
    ModelDescriptor {
        id: "claude-sonnet-4-20250514",
        is_default: true,
        capabilities: ModelCapabilities::full(8192),
    },
    ModelDescriptor {
        id: "claude-haiku-4-20250414",
        is_default: false,
        capabilities: ModelCapabilities::full(8192),
    },
    ModelDescriptor {
        id: "claude-opus-4-20250514",
        is_default: false,
        capabilities: ModelCapabilities::full(8192),
    },
];

const GEMINI_MODELS: &[ModelDescriptor] = &[
    ModelDescriptor {
        id: "gemini-1.5-flash",
        is_default: true,
        capabilities: ModelCapabilities::full(8192),
    },
    ModelDescriptor {
        id: "gemini-1.5-pro",
        is_default: false,
        capabilities: ModelCapabilities::full(8192),
    },
    ModelDescriptor {
        id: "gemini-2.0-flash-exp",
        is_default: false,
        capabilities: ModelCapabilities::full(8192),
    },
];

const BUILTIN_MODELS: &[ModelDescriptor] = &[ModelDescriptor {
    id: "qwen2.5-coder-3b",
    is_default: true,
    capabilities: ModelCapabilities {
        vision: false,
        tool_use: true,
        streaming: true,
        max_output_tokens: 2048,
    },
}];

const OLLAMA_MODELS: &[ModelDescriptor] = &[
    ModelDescriptor {
        id: "llama3.1:8b",
        is_default: true,
        capabilities: ModelCapabilities::local(),
    },
    ModelDescriptor {
        id: "qwen2.5:7b",
        is_default: false,
        capabilities: ModelCapabilities::local(),
    },
    ModelDescriptor {
        id: "mistral:7b",
        is_default: false,
        capabilities: ModelCapabilities::local(),
    },
];

pub fn find_models_for_provider(
    provider_or_alias: &str,
) -> Option<(&'static str, &'static [ModelDescriptor])> {
    let provider = find_provider(provider_or_alias)?;
    let models = match provider.id {
        "openrouter" => OPENROUTER_MODELS,
        "openai" | "openai-codex" | "copilot" => OPENAI_MODELS,
        "anthropic" => ANTHROPIC_MODELS,
        "gemini" => GEMINI_MODELS,
        "builtin" => BUILTIN_MODELS,
        "ollama" | "llamacpp" | "lmstudio" | "vllm" | "sglang" | "osaurus" => OLLAMA_MODELS,
        _ => OPENROUTER_MODELS,
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
}
