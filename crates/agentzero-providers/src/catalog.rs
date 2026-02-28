#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderDescriptor {
    pub id: &'static str,
    pub description: &'static str,
    pub aliases: &'static [&'static str],
}

const PROVIDER_CATALOG: &[ProviderDescriptor] = &[
    ProviderDescriptor {
        id: "openrouter",
        description: "OpenRouter",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "anthropic",
        description: "Anthropic",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "openai",
        description: "OpenAI",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "openai-codex",
        description: "OpenAI Codex (OAuth)",
        aliases: &["openai_codex", "codex"],
    },
    ProviderDescriptor {
        id: "ollama",
        description: "Ollama [local]",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "gemini",
        description: "Google Gemini",
        aliases: &["google", "google-gemini"],
    },
    ProviderDescriptor {
        id: "venice",
        description: "Venice",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "vercel",
        description: "Vercel AI Gateway",
        aliases: &["vercel-ai"],
    },
    ProviderDescriptor {
        id: "cloudflare",
        description: "Cloudflare AI",
        aliases: &["cloudflare-ai"],
    },
    ProviderDescriptor {
        id: "moonshot",
        description: "Moonshot",
        aliases: &["kimi"],
    },
    ProviderDescriptor {
        id: "kimi-code",
        description: "Kimi Code",
        aliases: &["kimi_coding", "kimi_for_coding"],
    },
    ProviderDescriptor {
        id: "synthetic",
        description: "Synthetic",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "opencode",
        description: "OpenCode Zen",
        aliases: &["opencode-zen"],
    },
    ProviderDescriptor {
        id: "zai",
        description: "Z.AI",
        aliases: &["z.ai"],
    },
    ProviderDescriptor {
        id: "glm",
        description: "GLM (Zhipu)",
        aliases: &["zhipu"],
    },
    ProviderDescriptor {
        id: "minimax",
        description: "MiniMax",
        aliases: &[
            "minimax-intl",
            "minimax-io",
            "minimax-global",
            "minimax-cn",
            "minimaxi",
            "minimax-oauth",
            "minimax-oauth-cn",
            "minimax-portal",
            "minimax-portal-cn",
        ],
    },
    ProviderDescriptor {
        id: "bedrock",
        description: "Amazon Bedrock",
        aliases: &["aws-bedrock"],
    },
    ProviderDescriptor {
        id: "hunyuan",
        description: "Hunyuan (Tencent)",
        aliases: &["tencent"],
    },
    ProviderDescriptor {
        id: "qianfan",
        description: "Qianfan (Baidu)",
        aliases: &["baidu"],
    },
    ProviderDescriptor {
        id: "doubao",
        description: "Doubao (Volcengine)",
        aliases: &["volcengine", "ark", "doubao-cn"],
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
    },
    ProviderDescriptor {
        id: "groq",
        description: "Groq",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "mistral",
        description: "Mistral",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "xai",
        description: "xAI (Grok)",
        aliases: &["grok"],
    },
    ProviderDescriptor {
        id: "deepseek",
        description: "DeepSeek",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "together",
        description: "Together AI",
        aliases: &["together-ai"],
    },
    ProviderDescriptor {
        id: "fireworks",
        description: "Fireworks AI",
        aliases: &["fireworks-ai"],
    },
    ProviderDescriptor {
        id: "perplexity",
        description: "Perplexity",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "cohere",
        description: "Cohere",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "copilot",
        description: "GitHub Copilot",
        aliases: &["github-copilot"],
    },
    ProviderDescriptor {
        id: "lmstudio",
        description: "LM Studio [local]",
        aliases: &["lm-studio"],
    },
    ProviderDescriptor {
        id: "llamacpp",
        description: "llama.cpp server [local]",
        aliases: &["llama.cpp"],
    },
    ProviderDescriptor {
        id: "sglang",
        description: "SGLang [local]",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "vllm",
        description: "vLLM [local]",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "osaurus",
        description: "Osaurus [local]",
        aliases: &[],
    },
    ProviderDescriptor {
        id: "nvidia",
        description: "NVIDIA NIM",
        aliases: &["nvidia-nim", "build.nvidia.com"],
    },
    ProviderDescriptor {
        id: "ovhcloud",
        description: "OVHcloud AI Endpoints",
        aliases: &["ovh"],
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
}
