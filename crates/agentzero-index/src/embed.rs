use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::chunk::TextChunk;

#[derive(Debug, Error)]
pub enum EmbedError {
    #[error("embedding request failed: {0}")]
    RequestFailed(String),
    #[error("unexpected response: {0}")]
    BadResponse(String),
}

/// A chunk with its computed embedding vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedChunk {
    pub chunk: TextChunk,
    pub embedding: Vec<f32>,
}

/// Generates embedding vectors from text.
#[allow(async_fn_in_trait)]
pub trait Embedder: Send + Sync {
    async fn embed_texts(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;
    fn model_name(&self) -> &str;
}

/// Embedder that calls Ollama's `/api/embed` endpoint.
pub struct OllamaEmbedder {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaEmbedder {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            client: reqwest::Client::new(),
        }
    }
}

impl Default for OllamaEmbedder {
    fn default() -> Self {
        Self::new("http://localhost:11434", "nomic-embed-text")
    }
}

#[derive(Serialize)]
struct OllamaEmbedRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
}

#[derive(Deserialize)]
struct OllamaEmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

impl Embedder for OllamaEmbedder {
    async fn embed_texts(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let url = format!("{}/api/embed", self.base_url);
        let body = OllamaEmbedRequest {
            model: &self.model,
            input: texts.to_vec(),
        };

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| EmbedError::RequestFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "(unreadable body)".into());
            return Err(EmbedError::RequestFailed(format!(
                "HTTP {status}: {body}"
            )));
        }

        let parsed: OllamaEmbedResponse = resp
            .json()
            .await
            .map_err(|e| EmbedError::BadResponse(e.to_string()))?;

        if parsed.embeddings.len() != texts.len() {
            return Err(EmbedError::BadResponse(format!(
                "expected {} embeddings, got {}",
                texts.len(),
                parsed.embeddings.len()
            )));
        }

        Ok(parsed.embeddings)
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
