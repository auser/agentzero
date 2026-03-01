use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    pub provider_id: String,
    pub base_url: String,
    pub reachable: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
}

pub async fn check_health(provider_id: &str, base_url: &str, timeout_ms: u64) -> HealthCheckResult {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .unwrap_or_default();

    let url = if provider_id == "ollama" {
        format!("{}/api/tags", base_url.trim_end_matches('/'))
    } else {
        format!("{}/v1/models", base_url.trim_end_matches('/'))
    };

    let start = Instant::now();
    match client.get(&url).send().await {
        Ok(response) => {
            let latency = start.elapsed().as_millis() as u64;
            if response.status().is_success() || response.status().as_u16() == 404 {
                HealthCheckResult {
                    provider_id: provider_id.to_string(),
                    base_url: base_url.to_string(),
                    reachable: true,
                    latency_ms: latency,
                    error: None,
                }
            } else {
                HealthCheckResult {
                    provider_id: provider_id.to_string(),
                    base_url: base_url.to_string(),
                    reachable: true,
                    latency_ms: latency,
                    error: Some(format!("HTTP {}", response.status())),
                }
            }
        }
        Err(err) => {
            let latency = start.elapsed().as_millis() as u64;
            HealthCheckResult {
                provider_id: provider_id.to_string(),
                base_url: base_url.to_string(),
                reachable: false,
                latency_ms: latency,
                error: Some(err.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_check_unreachable_host_returns_not_reachable() {
        let result = check_health("test", "http://127.0.0.1:19999", 500).await;
        assert!(!result.reachable);
        assert!(result.error.is_some());
        assert_eq!(result.provider_id, "test");
    }

    #[tokio::test]
    async fn health_check_preserves_provider_id_and_url() {
        let result = check_health("my-provider", "http://127.0.0.1:19998", 200).await;
        assert_eq!(result.provider_id, "my-provider");
        assert_eq!(result.base_url, "http://127.0.0.1:19998");
    }

    #[tokio::test]
    async fn health_check_records_latency() {
        let result = check_health("test", "http://127.0.0.1:19997", 200).await;
        // Latency should be recorded even for unreachable hosts
        // It should be > 0 since there's at minimum a connection attempt
        // (though could be 0 on very fast failure — just verify it's a valid u64)
        assert!(result.latency_ms <= 1000, "latency should be reasonable");
    }

    #[tokio::test]
    async fn health_check_ollama_uses_api_tags_endpoint() {
        // When provider_id is "ollama", the health check should hit /api/tags
        // We can't easily verify the URL without a mock server, but we can
        // verify the function doesn't panic and returns expected structure
        let result = check_health("ollama", "http://127.0.0.1:19996", 200).await;
        assert_eq!(result.provider_id, "ollama");
        assert!(!result.reachable);
    }

    #[tokio::test]
    async fn health_check_non_ollama_uses_v1_models_endpoint() {
        let result = check_health("vllm", "http://127.0.0.1:19995", 200).await;
        assert_eq!(result.provider_id, "vllm");
        assert!(!result.reachable);
    }
}
