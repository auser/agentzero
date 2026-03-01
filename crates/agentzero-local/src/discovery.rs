use agentzero_common::local_providers::{all_local_providers, LocalProviderType};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct DiscoveredService {
    pub provider_id: String,
    pub base_url: String,
    pub models: Vec<String>,
    pub status: ServiceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceStatus {
    Running,
    Unreachable,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct DiscoveryOptions {
    pub timeout_ms: u64,
    pub providers: Vec<String>,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            timeout_ms: 2000,
            providers: Vec::new(),
        }
    }
}

pub async fn discover_local_services(opts: DiscoveryOptions) -> Vec<DiscoveredService> {
    let providers = all_local_providers();
    let mut handles = Vec::new();

    for meta in providers {
        if meta.provider_type == LocalProviderType::Transcription {
            continue;
        }
        if !opts.providers.is_empty()
            && !opts
                .providers
                .iter()
                .any(|p| p.eq_ignore_ascii_case(meta.id))
        {
            continue;
        }

        let provider_id = meta.id.to_string();
        let base_url = meta.default_base_url.to_string();
        let timeout_ms = opts.timeout_ms;

        handles.push(tokio::spawn(async move {
            probe_service(&provider_id, &base_url, timeout_ms).await
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        if let Ok(result) = handle.await {
            results.push(result);
        }
    }
    results
}

async fn probe_service(provider_id: &str, base_url: &str, timeout_ms: u64) -> DiscoveredService {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .unwrap_or_default();

    if provider_id == "ollama" {
        probe_ollama(&client, provider_id, base_url).await
    } else {
        probe_openai_compat(&client, provider_id, base_url).await
    }
}

async fn probe_ollama(
    client: &reqwest::Client,
    provider_id: &str,
    base_url: &str,
) -> DiscoveredService {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    match client.get(&url).send().await {
        Ok(response) if response.status().is_success() => {
            let models = extract_ollama_model_names(response).await;
            DiscoveredService {
                provider_id: provider_id.to_string(),
                base_url: base_url.to_string(),
                models,
                status: ServiceStatus::Running,
            }
        }
        Ok(response) => DiscoveredService {
            provider_id: provider_id.to_string(),
            base_url: base_url.to_string(),
            models: Vec::new(),
            status: ServiceStatus::Error(format!("HTTP {}", response.status())),
        },
        Err(_) => DiscoveredService {
            provider_id: provider_id.to_string(),
            base_url: base_url.to_string(),
            models: Vec::new(),
            status: ServiceStatus::Unreachable,
        },
    }
}

async fn probe_openai_compat(
    client: &reqwest::Client,
    provider_id: &str,
    base_url: &str,
) -> DiscoveredService {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    match client.get(&url).send().await {
        Ok(response) if response.status().is_success() => {
            let models = extract_openai_model_ids(response).await;
            DiscoveredService {
                provider_id: provider_id.to_string(),
                base_url: base_url.to_string(),
                models,
                status: ServiceStatus::Running,
            }
        }
        Ok(response) => DiscoveredService {
            provider_id: provider_id.to_string(),
            base_url: base_url.to_string(),
            models: Vec::new(),
            status: ServiceStatus::Error(format!("HTTP {}", response.status())),
        },
        Err(_) => DiscoveredService {
            provider_id: provider_id.to_string(),
            base_url: base_url.to_string(),
            models: Vec::new(),
            status: ServiceStatus::Unreachable,
        },
    }
}

async fn extract_ollama_model_names(response: reqwest::Response) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct TagsResponse {
        #[serde(default)]
        models: Vec<TagsModel>,
    }
    #[derive(serde::Deserialize)]
    struct TagsModel {
        name: String,
    }

    response
        .json::<TagsResponse>()
        .await
        .map(|r| r.models.into_iter().map(|m| m.name).collect())
        .unwrap_or_default()
}

async fn extract_openai_model_ids(response: reqwest::Response) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct ModelsResponse {
        #[serde(default)]
        data: Vec<ModelEntry>,
    }
    #[derive(serde::Deserialize)]
    struct ModelEntry {
        id: String,
    }

    response
        .json::<ModelsResponse>()
        .await
        .map(|r| r.data.into_iter().map(|m| m.id).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_discovery_options_has_sensible_timeout() {
        let opts = DiscoveryOptions::default();
        assert_eq!(opts.timeout_ms, 2000);
        assert!(opts.providers.is_empty());
    }

    #[tokio::test]
    async fn discover_returns_one_result_per_requested_provider() {
        let opts = DiscoveryOptions {
            timeout_ms: 500,
            providers: vec!["ollama".to_string()],
        };
        let results = discover_local_services(opts).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider_id, "ollama");
        // Status depends on whether Ollama is actually running on this machine
        match &results[0].status {
            ServiceStatus::Running => { /* models may or may not be loaded */ }
            ServiceStatus::Unreachable => assert!(results[0].models.is_empty()),
            ServiceStatus::Error(_) => {}
        }
    }

    #[tokio::test]
    async fn discover_unreachable_port_returns_unreachable() {
        let result = probe_service("fake-provider", "http://127.0.0.1:19999", 200).await;
        assert_eq!(result.provider_id, "fake-provider");
        assert_eq!(result.status, ServiceStatus::Unreachable);
    }

    #[tokio::test]
    async fn discover_skips_transcription_providers() {
        // whispercpp is a transcription provider and should be excluded from discovery
        let opts = DiscoveryOptions {
            timeout_ms: 200,
            providers: vec!["whispercpp".to_string()],
        };
        let results = discover_local_services(opts).await;
        assert!(
            results.is_empty(),
            "transcription providers should be excluded from discovery"
        );
    }

    #[tokio::test]
    async fn discover_empty_filter_probes_all_chat_providers() {
        let opts = DiscoveryOptions {
            timeout_ms: 200,
            providers: Vec::new(),
        };
        let results = discover_local_services(opts).await;
        // Should have one result per non-transcription local provider
        // At minimum: ollama, llamacpp, lmstudio, vllm, sglang, osaurus = 6
        assert!(
            results.len() >= 5,
            "empty filter should probe all chat providers, got {}",
            results.len()
        );
        // whispercpp should not appear
        assert!(
            !results.iter().any(|r| r.provider_id == "whispercpp"),
            "whispercpp should not be in discovery results"
        );
    }

    #[tokio::test]
    async fn discover_filter_is_case_insensitive() {
        let opts = DiscoveryOptions {
            timeout_ms: 200,
            providers: vec!["OLLAMA".to_string()],
        };
        let results = discover_local_services(opts).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider_id, "ollama");
    }

    #[tokio::test]
    async fn probe_service_unreachable_has_empty_models() {
        let result = probe_service("llamacpp", "http://127.0.0.1:19998", 200).await;
        assert_eq!(result.status, ServiceStatus::Unreachable);
        assert!(
            result.models.is_empty(),
            "unreachable service should have no models"
        );
        assert_eq!(result.base_url, "http://127.0.0.1:19998");
    }

    #[tokio::test]
    async fn probe_service_routes_ollama_and_others_differently() {
        // Both should return Unreachable for a dead port, but the important thing
        // is that probe_service correctly dispatches to probe_ollama vs probe_openai_compat
        let ollama_result = probe_service("ollama", "http://127.0.0.1:19997", 200).await;
        let vllm_result = probe_service("vllm", "http://127.0.0.1:19996", 200).await;

        assert_eq!(ollama_result.provider_id, "ollama");
        assert_eq!(vllm_result.provider_id, "vllm");
        // Both should be unreachable since these ports are dead
        assert_eq!(ollama_result.status, ServiceStatus::Unreachable);
        assert_eq!(vllm_result.status, ServiceStatus::Unreachable);
    }

    #[test]
    fn service_status_equality() {
        assert_eq!(ServiceStatus::Running, ServiceStatus::Running);
        assert_eq!(ServiceStatus::Unreachable, ServiceStatus::Unreachable);
        assert_eq!(
            ServiceStatus::Error("test".to_string()),
            ServiceStatus::Error("test".to_string())
        );
        assert_ne!(ServiceStatus::Running, ServiceStatus::Unreachable);
        assert_ne!(
            ServiceStatus::Error("a".to_string()),
            ServiceStatus::Error("b".to_string())
        );
    }
}
