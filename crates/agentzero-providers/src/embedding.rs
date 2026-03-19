//! API-backed embedding provider for OpenAI-compatible endpoints.
//!
//! Calls `/v1/embeddings` with the configured model and returns the
//! embedding vector. Works with OpenAI, Azure OpenAI, and any compatible API.

use agentzero_core::embedding::EmbeddingProvider;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Embedding provider that calls an OpenAI-compatible `/v1/embeddings` endpoint.
pub struct ApiEmbeddingProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    dimensions: usize,
}

impl ApiEmbeddingProvider {
    pub fn new(base_url: &str, api_key: &str, model: &str, dimensions: usize) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            dimensions,
        }
    }
}

#[derive(Serialize)]
struct EmbeddingRequest {
    input: String,
    model: String,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingProvider for ApiEmbeddingProvider {
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let url = format!("{}/v1/embeddings", self.base_url);
        let body = EmbeddingRequest {
            input: text.to_string(),
            model: self.model.clone(),
        };

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("embedding request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "(no body)".to_string());
            return Err(anyhow::anyhow!("embedding API returned {status}: {body}"));
        }

        let parsed: EmbeddingResponse = resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("failed to parse embedding response: {e}"))?;

        parsed
            .data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| anyhow::anyhow!("embedding response contained no data"))
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::embedding::EmbeddingProvider;

    #[test]
    fn api_embedding_provider_dimensions() {
        let provider = ApiEmbeddingProvider::new(
            "https://api.openai.com",
            "test-key",
            "text-embedding-3-small",
            1536,
        );
        assert_eq!(provider.dimensions(), 1536);
    }

    #[test]
    fn api_embedding_provider_base_url_trimmed() {
        let provider = ApiEmbeddingProvider::new(
            "https://api.openai.com/",
            "test-key",
            "text-embedding-3-small",
            1536,
        );
        assert_eq!(provider.base_url, "https://api.openai.com");
    }

    #[tokio::test]
    async fn embed_returns_error_on_invalid_url() {
        let provider = ApiEmbeddingProvider::new(
            "http://localhost:1",
            "test-key",
            "text-embedding-3-small",
            1536,
        );
        let err = provider
            .embed("hello")
            .await
            .expect_err("should fail on invalid URL");
        assert!(
            err.to_string().contains("request failed"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn embed_parses_mock_response() {
        use std::net::TcpListener;

        // Start a mock server
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);

        let mock_response = serde_json::json!({
            "object": "list",
            "data": [{
                "object": "embedding",
                "index": 0,
                "embedding": [0.1, 0.2, 0.3, 0.4]
            }],
            "model": "text-embedding-3-small",
            "usage": {"prompt_tokens": 5, "total_tokens": 5}
        });

        let resp_body = serde_json::to_string(&mock_response).expect("serialize");
        let resp_body_clone = resp_body.clone();

        // Spawn a minimal HTTP server
        let server = tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
                .await
                .expect("bind");
            let (mut stream, _) = listener.accept().await.expect("accept");
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            // Read request (just drain it)
            let mut buf = vec![0u8; 4096];
            let _ = stream.read(&mut buf).await;

            // Write response
            let http_response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                resp_body_clone.len(),
                resp_body_clone
            );
            stream
                .write_all(http_response.as_bytes())
                .await
                .expect("write");
            stream.flush().await.expect("flush");
        });

        // Small delay for server to start
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let provider = ApiEmbeddingProvider::new(
            &format!("http://127.0.0.1:{port}"),
            "test-key",
            "text-embedding-3-small",
            4,
        );

        let embedding = provider.embed("hello world").await.expect("should succeed");
        assert_eq!(embedding, vec![0.1, 0.2, 0.3, 0.4]);

        server.abort();
    }
}
