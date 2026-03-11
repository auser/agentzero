#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderDescriptor {
    pub id: &'static str,
    pub description: &'static str,
    pub aliases: &'static [&'static str],
    /// Default base URL for this provider (without `/v1` — the code appends it).
    /// `None` means the user must configure `base_url` explicitly.
    pub default_base_url: Option<&'static str>,
}

const PROVIDER_CATALOG: &[ProviderDescriptor] = &[
    ProviderDescriptor {
        id: "openrouter",
        description: "OpenRouter",
        aliases: &[],
        default_base_url: Some("https://openrouter.ai/api"),
    },
    ProviderDescriptor {
        id: "anthropic",
        description: "Anthropic",
        aliases: &[],
        default_base_url: Some("https://api.anthropic.com"),
    },
    ProviderDescriptor {
        id: "openai",
        description: "OpenAI",
        aliases: &[],
        default_base_url: Some("https://api.openai.com"),
    },
    ProviderDescriptor {
        id: "openai-codex",
        description: "OpenAI Codex (OAuth)",
        aliases: &["openai_codex", "codex"],
        default_base_url: Some("https://api.openai.com"),
    },
    ProviderDescriptor {
        id: "ollama",
        description: "Ollama [local]",
        aliases: &[],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "gemini",
        description: "Google Gemini",
        aliases: &["google", "google-gemini"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "venice",
        description: "Venice",
        aliases: &[],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "vercel",
        description: "Vercel AI Gateway",
        aliases: &["vercel-ai"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "cloudflare",
        description: "Cloudflare AI",
        aliases: &["cloudflare-ai"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "moonshot",
        description: "Moonshot",
        aliases: &["kimi"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "kimi-code",
        description: "Kimi Code",
        aliases: &["kimi_coding", "kimi_for_coding"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "synthetic",
        description: "Synthetic",
        aliases: &[],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "opencode",
        description: "OpenCode Zen",
        aliases: &["opencode-zen"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "zai",
        description: "Z.AI",
        aliases: &["z.ai"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "glm",
        description: "GLM (Zhipu)",
        aliases: &["zhipu"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "bedrock",
        description: "Amazon Bedrock",
        aliases: &["aws-bedrock"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "hunyuan",
        description: "Hunyuan (Tencent)",
        aliases: &["tencent"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "qianfan",
        description: "Qianfan (Baidu)",
        aliases: &["baidu"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "doubao",
        description: "Doubao (Volcengine)",
        aliases: &["volcengine", "ark", "doubao-cn"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "qwen",
        description: "Qwen (DashScope / Qwen Code OAuth)",
        aliases: &[
            "dashscope",
            "qwen-intl",
            "dashscope-intl",
            "qwen-us",
            "dashscope-us",
            "qwen-coding-plan",
            "qwen-code",
            "qwen-oauth",
            "qwen_oauth",
        ],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "groq",
        description: "Groq",
        aliases: &[],
        default_base_url: Some("https://api.groq.com/openai"),
    },
    ProviderDescriptor {
        id: "mistral",
        description: "Mistral",
        aliases: &[],
        default_base_url: Some("https://api.mistral.ai"),
    },
    ProviderDescriptor {
        id: "xai",
        description: "xAI (Grok)",
        aliases: &["grok"],
        default_base_url: Some("https://api.x.ai"),
    },
    ProviderDescriptor {
        id: "deepseek",
        description: "DeepSeek",
        aliases: &[],
        default_base_url: Some("https://api.deepseek.com"),
    },
    ProviderDescriptor {
        id: "together",
        description: "Together AI",
        aliases: &["together-ai"],
        default_base_url: Some("https://api.together.xyz"),
    },
    ProviderDescriptor {
        id: "fireworks",
        description: "Fireworks AI",
        aliases: &["fireworks-ai"],
        default_base_url: Some("https://api.fireworks.ai/inference"),
    },
    ProviderDescriptor {
        id: "perplexity",
        description: "Perplexity",
        aliases: &[],
        default_base_url: Some("https://api.perplexity.ai"),
    },
    ProviderDescriptor {
        id: "cohere",
        description: "Cohere",
        aliases: &[],
        default_base_url: Some("https://api.cohere.com"),
    },
    ProviderDescriptor {
        id: "copilot",
        description: "GitHub Copilot",
        aliases: &["github-copilot"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "lmstudio",
        description: "LM Studio [local]",
        aliases: &["lm-studio"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "llamacpp",
        description: "llama.cpp server [local]",
        aliases: &["llama.cpp"],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "sglang",
        description: "SGLang [local]",
        aliases: &[],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "vllm",
        description: "vLLM [local]",
        aliases: &[],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "osaurus",
        description: "Osaurus [local]",
        aliases: &[],
        default_base_url: None,
    },
    ProviderDescriptor {
        id: "nvidia",
        description: "NVIDIA NIM",
        aliases: &["nvidia-nim", "build.nvidia.com"],
        default_base_url: Some("https://integrate.api.nvidia.com"),
    },
    ProviderDescriptor {
        id: "ovhcloud",
        description: "OVHcloud AI Endpoints",
        aliases: &["ovh"],
        default_base_url: None,
    },
];

pub fn supported_providers() -> &'static [ProviderDescriptor] {
    PROVIDER_CATALOG
}

pub fn find_provider(id_or_alias: &str) -> Option<&'static ProviderDescriptor> {
    let needle = id_or_alias.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return None;
    }
    PROVIDER_CATALOG.iter().find(|provider| {
        provider.id.eq(needle.as_str())
            || provider
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(&needle))
    })
}

#[cfg(test)]
mod tests {
    use super::find_provider;

    #[test]
    fn find_provider_resolves_primary_id_and_alias() {
        let by_id = find_provider("openrouter").expect("provider id should resolve");
        assert_eq!(by_id.id, "openrouter");

        let by_alias = find_provider("github-copilot").expect("provider alias should resolve");
        assert_eq!(by_alias.id, "copilot");
    }

    #[test]
    fn find_provider_returns_none_for_unknown_provider() {
        assert!(find_provider("unknown-provider").is_none());
    }

    #[test]
    fn known_cloud_providers_have_default_base_url() {
        for id in [
            "openrouter",
            "openai",
            "openai-codex",
            "anthropic",
            "groq",
            "mistral",
            "deepseek",
        ] {
            let p = find_provider(id).unwrap_or_else(|| panic!("{id} should be in catalog"));
            assert!(
                p.default_base_url.is_some(),
                "{id} should have a default_base_url"
            );
        }
    }

    #[test]
    fn local_providers_have_no_default_base_url() {
        for id in ["ollama", "llamacpp", "lmstudio", "vllm", "sglang"] {
            let p = find_provider(id).unwrap_or_else(|| panic!("{id} should be in catalog"));
            assert!(
                p.default_base_url.is_none(),
                "{id} should NOT have a default_base_url"
            );
        }
    }
}
