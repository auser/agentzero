use crate::find_provider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelDescriptor {
    pub id: &'static str,
    pub is_default: bool,
}

const OPENROUTER_MODELS: &[ModelDescriptor] = &[
    ModelDescriptor {
        id: "anthropic/claude-3.5-sonnet",
        is_default: true,
    },
    ModelDescriptor {
        id: "openai/gpt-4o-mini",
        is_default: false,
    },
    ModelDescriptor {
        id: "google/gemini-1.5-pro",
        is_default: false,
    },
];

const OPENAI_MODELS: &[ModelDescriptor] = &[
    ModelDescriptor {
        id: "gpt-4o-mini",
        is_default: true,
    },
    ModelDescriptor {
        id: "gpt-4.1-mini",
        is_default: false,
    },
    ModelDescriptor {
        id: "gpt-4.1",
        is_default: false,
    },
];

const ANTHROPIC_MODELS: &[ModelDescriptor] = &[
    ModelDescriptor {
        id: "claude-3-5-sonnet-latest",
        is_default: true,
    },
    ModelDescriptor {
        id: "claude-3-5-haiku-latest",
        is_default: false,
    },
    ModelDescriptor {
        id: "claude-3-opus-latest",
        is_default: false,
    },
];

const GEMINI_MODELS: &[ModelDescriptor] = &[
    ModelDescriptor {
        id: "gemini-1.5-flash",
        is_default: true,
    },
    ModelDescriptor {
        id: "gemini-1.5-pro",
        is_default: false,
    },
    ModelDescriptor {
        id: "gemini-2.0-flash-exp",
        is_default: false,
    },
];

const OLLAMA_MODELS: &[ModelDescriptor] = &[
    ModelDescriptor {
        id: "llama3.1:8b",
        is_default: true,
    },
    ModelDescriptor {
        id: "qwen2.5:7b",
        is_default: false,
    },
    ModelDescriptor {
        id: "mistral:7b",
        is_default: false,
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
        "ollama" | "llamacpp" | "lmstudio" | "vllm" | "sglang" | "osaurus" => OLLAMA_MODELS,
        _ => OPENROUTER_MODELS,
    };
    Some((provider.id, models))
}

#[cfg(test)]
mod tests {
    use super::find_models_for_provider;

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
}
