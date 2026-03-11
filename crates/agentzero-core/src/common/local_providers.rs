#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalProviderType {
    ChatCompletion,
    Transcription,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalProviderMeta {
    pub id: &'static str,
    pub default_port: u16,
    pub default_base_url: &'static str,
    pub models_endpoint: Option<&'static str>,
    pub supports_pull: bool,
    pub provider_type: LocalProviderType,
}

const LOCAL_PROVIDERS: &[LocalProviderMeta] = &[
    LocalProviderMeta {
        id: "builtin",
        default_port: 0,
        default_base_url: "",
        models_endpoint: None,
        supports_pull: false,
        provider_type: LocalProviderType::ChatCompletion,
    },
    LocalProviderMeta {
        id: "ollama",
        default_port: 11434,
        default_base_url: "http://localhost:11434",
        models_endpoint: Some("/api/tags"),
        supports_pull: true,
        provider_type: LocalProviderType::ChatCompletion,
    },
    LocalProviderMeta {
        id: "llamacpp",
        default_port: 8080,
        default_base_url: "http://localhost:8080",
        models_endpoint: Some("/v1/models"),
        supports_pull: false,
        provider_type: LocalProviderType::ChatCompletion,
    },
    LocalProviderMeta {
        id: "lmstudio",
        default_port: 1234,
        default_base_url: "http://localhost:1234",
        models_endpoint: Some("/v1/models"),
        supports_pull: false,
        provider_type: LocalProviderType::ChatCompletion,
    },
    LocalProviderMeta {
        id: "vllm",
        default_port: 8000,
        default_base_url: "http://localhost:8000",
        models_endpoint: Some("/v1/models"),
        supports_pull: false,
        provider_type: LocalProviderType::ChatCompletion,
    },
    LocalProviderMeta {
        id: "sglang",
        default_port: 30000,
        default_base_url: "http://localhost:30000",
        models_endpoint: Some("/v1/models"),
        supports_pull: false,
        provider_type: LocalProviderType::ChatCompletion,
    },
    LocalProviderMeta {
        id: "osaurus",
        default_port: 8080,
        default_base_url: "http://localhost:8080",
        models_endpoint: Some("/v1/models"),
        supports_pull: false,
        provider_type: LocalProviderType::ChatCompletion,
    },
    LocalProviderMeta {
        id: "whispercpp",
        default_port: 8080,
        default_base_url: "http://localhost:8080",
        models_endpoint: None,
        supports_pull: false,
        provider_type: LocalProviderType::Transcription,
    },
];

pub fn is_local_provider(id: &str) -> bool {
    let needle = id.trim().to_ascii_lowercase();
    LOCAL_PROVIDERS.iter().any(|p| p.id == needle)
}

pub fn local_provider_meta(id: &str) -> Option<&'static LocalProviderMeta> {
    let needle = id.trim().to_ascii_lowercase();
    LOCAL_PROVIDERS.iter().find(|p| p.id == needle)
}

pub fn all_local_providers() -> &'static [LocalProviderMeta] {
    LOCAL_PROVIDERS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_local_provider_recognises_ollama() {
        assert!(is_local_provider("ollama"));
        assert!(is_local_provider("Ollama"));
        assert!(is_local_provider("  ollama  "));
    }

    #[test]
    fn is_local_provider_rejects_cloud_providers() {
        assert!(!is_local_provider("openrouter"));
        assert!(!is_local_provider("openai"));
        assert!(!is_local_provider("anthropic"));
    }

    #[test]
    fn local_provider_meta_returns_correct_defaults() {
        let meta = local_provider_meta("ollama").expect("ollama should be found");
        assert_eq!(meta.default_port, 11434);
        assert_eq!(meta.default_base_url, "http://localhost:11434");
        assert!(meta.supports_pull);
    }

    #[test]
    fn local_provider_meta_returns_none_for_unknown() {
        assert!(local_provider_meta("openai").is_none());
        assert!(local_provider_meta("").is_none());
    }

    #[test]
    fn all_local_providers_contains_expected_count() {
        assert_eq!(all_local_providers().len(), 8);
    }

    #[test]
    fn builtin_is_local_provider() {
        assert!(is_local_provider("builtin"));
    }

    #[test]
    fn whispercpp_is_transcription_type() {
        let meta = local_provider_meta("whispercpp").expect("whispercpp should be found");
        assert_eq!(meta.provider_type, LocalProviderType::Transcription);
        assert!(meta.models_endpoint.is_none());
    }
}
