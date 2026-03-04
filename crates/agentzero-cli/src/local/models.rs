use anyhow::Context;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct LiveModel {
    pub id: String,
    pub size_bytes: Option<u64>,
    pub family: Option<String>,
    pub quantization: Option<String>,
}

pub async fn list_models(
    provider_id: &str,
    base_url: &str,
    timeout_ms: u64,
) -> anyhow::Result<Vec<LiveModel>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .context("failed to build HTTP client")?;

    if provider_id == "ollama" {
        list_ollama_models(&client, base_url).await
    } else {
        list_openai_models(&client, base_url).await
    }
}

async fn list_ollama_models(
    client: &reqwest::Client,
    base_url: &str,
) -> anyhow::Result<Vec<LiveModel>> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let response = client
        .get(&url)
        .send()
        .await
        .context("failed to connect to Ollama")?;

    if !response.status().is_success() {
        anyhow::bail!("Ollama returned HTTP {} from {}", response.status(), url);
    }

    let body: OllamaTagsResponse = response
        .json()
        .await
        .context("failed to parse Ollama /api/tags response")?;

    Ok(body
        .models
        .into_iter()
        .map(|m| LiveModel {
            id: m.name,
            size_bytes: Some(m.size),
            family: m.details.as_ref().map(|d| d.family.clone()),
            quantization: m.details.as_ref().map(|d| d.quantization_level.clone()),
        })
        .collect())
}

async fn list_openai_models(
    client: &reqwest::Client,
    base_url: &str,
) -> anyhow::Result<Vec<LiveModel>> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let response = client
        .get(&url)
        .send()
        .await
        .context("failed to connect to local model server")?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Local model server returned HTTP {} from {}",
            response.status(),
            url
        );
    }

    let body: OpenAiModelsResponse = response
        .json()
        .await
        .context("failed to parse /v1/models response")?;

    Ok(body
        .data
        .into_iter()
        .map(|m| LiveModel {
            id: m.id,
            size_bytes: None,
            family: None,
            quantization: None,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
    #[serde(default)]
    size: u64,
    details: Option<OllamaModelDetails>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelDetails {
    #[serde(default)]
    family: String,
    #[serde(default)]
    quantization_level: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    #[serde(default)]
    data: Vec<OpenAiModel>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModel {
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ollama_tags_response() {
        let json = r#"{"models":[{"name":"llama3.1:8b","size":4661224676,"details":{"family":"llama","quantization_level":"Q4_0"}},{"name":"qwen2.5:7b","size":4355834880,"details":{"family":"qwen2","quantization_level":"Q4_K_M"}}]}"#;
        let parsed: OllamaTagsResponse = serde_json::from_str(json).expect("should parse");
        assert_eq!(parsed.models.len(), 2);
        assert_eq!(parsed.models[0].name, "llama3.1:8b");
        assert_eq!(parsed.models[0].size, 4661224676);
        assert_eq!(
            parsed.models[0].details.as_ref().map(|d| d.family.as_str()),
            Some("llama")
        );
    }

    #[test]
    fn parse_openai_models_response() {
        let json = r#"{"object":"list","data":[{"id":"mistral:7b","object":"model"},{"id":"phi-3","object":"model"}]}"#;
        let parsed: OpenAiModelsResponse = serde_json::from_str(json).expect("should parse");
        assert_eq!(parsed.data.len(), 2);
        assert_eq!(parsed.data[0].id, "mistral:7b");
    }

    #[test]
    fn parse_empty_ollama_response() {
        let json = r#"{"models":[]}"#;
        let parsed: OllamaTagsResponse = serde_json::from_str(json).expect("should parse");
        assert!(parsed.models.is_empty());
    }

    #[test]
    fn parse_empty_openai_models_response() {
        let json = r#"{"object":"list","data":[]}"#;
        let parsed: OpenAiModelsResponse = serde_json::from_str(json).expect("should parse");
        assert!(parsed.data.is_empty());
    }

    #[test]
    fn parse_ollama_model_without_details() {
        let json = r#"{"models":[{"name":"custom-model","size":0}]}"#;
        let parsed: OllamaTagsResponse = serde_json::from_str(json).expect("should parse");
        assert_eq!(parsed.models.len(), 1);
        assert_eq!(parsed.models[0].name, "custom-model");
        assert!(parsed.models[0].details.is_none());
    }

    #[test]
    fn parse_ollama_model_without_size_defaults_to_zero() {
        let json = r#"{"models":[{"name":"tiny"}]}"#;
        let parsed: OllamaTagsResponse = serde_json::from_str(json).expect("should parse");
        assert_eq!(parsed.models[0].size, 0);
    }

    #[test]
    fn parse_openai_response_ignores_extra_fields() {
        let json = r#"{"object":"list","data":[{"id":"model-1","object":"model","created":1234567890,"owned_by":"user"}]}"#;
        let parsed: OpenAiModelsResponse = serde_json::from_str(json).expect("should parse");
        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].id, "model-1");
    }

    #[test]
    fn parse_ollama_response_missing_models_key_defaults_empty() {
        let json = r#"{}"#;
        let parsed: OllamaTagsResponse = serde_json::from_str(json).expect("should parse");
        assert!(parsed.models.is_empty());
    }

    #[test]
    fn parse_openai_response_missing_data_key_defaults_empty() {
        let json = r#"{}"#;
        let parsed: OpenAiModelsResponse = serde_json::from_str(json).expect("should parse");
        assert!(parsed.data.is_empty());
    }

    #[test]
    fn live_model_from_ollama_includes_metadata() {
        let json = r#"{"models":[{"name":"llama3.1:8b","size":4661224676,"details":{"family":"llama","quantization_level":"Q4_0"}}]}"#;
        let parsed: OllamaTagsResponse = serde_json::from_str(json).expect("should parse");
        let model = &parsed.models[0];
        let live = LiveModel {
            id: model.name.clone(),
            size_bytes: Some(model.size),
            family: model.details.as_ref().map(|d| d.family.clone()),
            quantization: model.details.as_ref().map(|d| d.quantization_level.clone()),
        };

        assert_eq!(live.id, "llama3.1:8b");
        assert_eq!(live.size_bytes, Some(4661224676));
        assert_eq!(live.family.as_deref(), Some("llama"));
        assert_eq!(live.quantization.as_deref(), Some("Q4_0"));
    }

    #[test]
    fn live_model_from_openai_has_no_metadata() {
        let live = LiveModel {
            id: "mistral:7b".to_string(),
            size_bytes: None,
            family: None,
            quantization: None,
        };

        assert_eq!(live.id, "mistral:7b");
        assert!(live.size_bytes.is_none());
        assert!(live.family.is_none());
        assert!(live.quantization.is_none());
    }

    #[tokio::test]
    async fn list_models_unreachable_ollama_returns_error() {
        let result = list_models("ollama", "http://127.0.0.1:19994", 200).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Ollama") || err.contains("connect"),
            "error should mention Ollama or connection: {err}"
        );
    }

    #[tokio::test]
    async fn list_models_unreachable_openai_compat_returns_error() {
        let result = list_models("vllm", "http://127.0.0.1:19993", 200).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("local model server") || err.contains("connect"),
            "error should mention local server or connection: {err}"
        );
    }
}
