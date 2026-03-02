#[allow(unused_imports)]
use crate::transport::TransportErrorKind;
use crate::transport::{
    collect_headers, jittered_backoff, map_reqwest_error, map_status_error, parse_retry_after,
    read_response_body, should_retry_status, should_retry_transport, TransportError,
    TransportResponse, MAX_ATTEMPTS,
};
use agentzero_core::{ChatResult, Provider, ReasoningConfig};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::sleep;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 8192;

// ---------------------------------------------------------------------------
// Transport abstraction (Anthropic-specific payload)
// ---------------------------------------------------------------------------

#[async_trait]
trait HttpTransport: Send + Sync {
    async fn send(
        &self,
        url: &str,
        api_key: &str,
        payload: &MessagesRequest,
    ) -> Result<TransportResponse, TransportError>;
}

struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl HttpTransport for ReqwestTransport {
    async fn send(
        &self,
        url: &str,
        api_key: &str,
        payload: &MessagesRequest,
    ) -> Result<TransportResponse, TransportError> {
        let response = self
            .client
            .post(url)
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(payload)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let status = response.status().as_u16();
        let headers = collect_headers(&response);
        let body = response.text().await.map_err(read_response_body)?;

        Ok(TransportResponse {
            status,
            headers,
            body,
        })
    }
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct AnthropicProvider {
    base_url: String,
    api_key: String,
    model: String,
    transport: Arc<dyn HttpTransport>,
}

impl AnthropicProvider {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url,
            api_key,
            model,
            transport: Arc::new(ReqwestTransport::new()),
        }
    }

    #[cfg(test)]
    fn with_transport(
        base_url: String,
        api_key: String,
        model: String,
        transport: Arc<dyn HttpTransport>,
    ) -> Self {
        Self {
            base_url,
            api_key,
            model,
            transport,
        }
    }
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ThinkingConfig {
    #[serde(rename = "type")]
    kind: String,
    budget_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking {
        #[allow(dead_code)]
        thinking: String,
    },
    #[serde(other)]
    Other,
}

fn extract_text(response: &MessagesResponse) -> String {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn parse_output_text(body: &str) -> anyhow::Result<String> {
    let response: MessagesResponse =
        serde_json::from_str(body).context("failed to parse Anthropic response JSON")?;
    let text = extract_text(&response);
    if text.is_empty() {
        anyhow::bail!("Anthropic response contained no text content blocks");
    }
    Ok(text)
}

// ---------------------------------------------------------------------------
// Provider impl
// ---------------------------------------------------------------------------

#[async_trait]
impl Provider for AnthropicProvider {
    async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
        self.complete_with_reasoning(prompt, &ReasoningConfig::default())
            .await
    }

    async fn complete_with_reasoning(
        &self,
        prompt: &str,
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));

        let thinking = match reasoning.enabled {
            Some(true) => {
                let budget = reasoning
                    .level
                    .as_deref()
                    .and_then(|l| l.parse::<u32>().ok())
                    .unwrap_or(10000);
                Some(ThinkingConfig {
                    kind: "enabled".to_string(),
                    budget_tokens: budget,
                })
            }
            _ => None,
        };

        let max_tokens = if thinking.is_some() {
            // Anthropic requires max_tokens > budget_tokens when thinking is enabled.
            thinking
                .as_ref()
                .map(|t| t.budget_tokens.saturating_add(DEFAULT_MAX_TOKENS))
                .unwrap_or(DEFAULT_MAX_TOKENS)
        } else {
            DEFAULT_MAX_TOKENS
        };

        let payload = MessagesRequest {
            model: self.model.clone(),
            max_tokens,
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            thinking,
        };

        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 0..MAX_ATTEMPTS {
            match self.transport.send(&url, &self.api_key, &payload).await {
                Ok(response) => {
                    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
                    if !status.is_success() {
                        let mapped = map_status_error(status, &response.body);
                        if attempt + 1 < MAX_ATTEMPTS && should_retry_status(status) {
                            sleep(
                                parse_retry_after(&response.headers)
                                    .unwrap_or_else(|| jittered_backoff(attempt)),
                            )
                            .await;
                            last_error = Some(mapped);
                            continue;
                        }
                        return Err(mapped);
                    }

                    let output_text = parse_output_text(&response.body)
                        .with_context(|| "failed to parse Anthropic response".to_string())?;
                    return Ok(ChatResult { output_text });
                }
                Err(error) => {
                    let mapped = anyhow!("provider request failed: {}", error.message);
                    if attempt + 1 < MAX_ATTEMPTS && should_retry_transport(error.kind) {
                        sleep(jittered_backoff(attempt)).await;
                        last_error = Some(mapped);
                        continue;
                    }
                    return Err(mapped);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("provider request failed after retries")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct ScriptedTransport {
        calls: AtomicUsize,
        scripted: Mutex<VecDeque<Result<TransportResponse, TransportError>>>,
    }

    impl ScriptedTransport {
        fn new(scripted: Vec<Result<TransportResponse, TransportError>>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                scripted: Mutex::new(scripted.into()),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl HttpTransport for ScriptedTransport {
        async fn send(
            &self,
            _url: &str,
            _api_key: &str,
            _payload: &MessagesRequest,
        ) -> Result<TransportResponse, TransportError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.scripted
                .lock()
                .expect("script mutex should not be poisoned")
                .pop_front()
                .unwrap_or_else(|| {
                    Err(TransportError::new(
                        TransportErrorKind::Other,
                        "no scripted response available",
                    ))
                })
        }
    }

    fn ok_response(body: &str) -> Result<TransportResponse, TransportError> {
        Ok(TransportResponse {
            status: 200,
            headers: HashMap::new(),
            body: body.to_string(),
        })
    }

    fn status_response(status: u16, body: &str) -> Result<TransportResponse, TransportError> {
        Ok(TransportResponse {
            status,
            headers: HashMap::new(),
            body: body.to_string(),
        })
    }

    const SUCCESS_BODY: &str = r#"{"content":[{"type":"text","text":"Hello!"}],"model":"claude-sonnet-4-20250514","stop_reason":"end_turn"}"#;

    #[test]
    fn parse_output_text_extracts_text_block() {
        let text = parse_output_text(SUCCESS_BODY).expect("should parse");
        assert_eq!(text, "Hello!");
    }

    #[test]
    fn parse_output_text_concatenates_multiple_text_blocks() {
        let body =
            r#"{"content":[{"type":"text","text":"Hello "},{"type":"text","text":"world!"}]}"#;
        let text = parse_output_text(body).expect("should parse");
        assert_eq!(text, "Hello world!");
    }

    #[test]
    fn parse_output_text_skips_thinking_blocks() {
        let body = r#"{"content":[{"type":"thinking","thinking":"let me think..."},{"type":"text","text":"answer"}]}"#;
        let text = parse_output_text(body).expect("should parse");
        assert_eq!(text, "answer");
    }

    #[test]
    fn parse_output_text_fails_on_empty_content() {
        let body = r#"{"content":[]}"#;
        assert!(parse_output_text(body).is_err());
    }

    #[tokio::test]
    async fn complete_succeeds_on_mock_response() {
        let transport = Arc::new(ScriptedTransport::new(vec![ok_response(SUCCESS_BODY)]));
        let provider = AnthropicProvider::with_transport(
            "https://api.anthropic.com".to_string(),
            "sk-ant-test".to_string(),
            "claude-sonnet-4-20250514".to_string(),
            transport.clone(),
        );

        let result = provider.complete("hello").await.expect("should succeed");
        assert_eq!(result.output_text, "Hello!");
        assert_eq!(transport.calls(), 1);
    }

    #[tokio::test]
    async fn complete_fails_immediately_on_auth_error() {
        let transport = Arc::new(ScriptedTransport::new(vec![
            status_response(401, r#"{"error":{"message":"invalid x-api-key"}}"#),
            ok_response(SUCCESS_BODY),
        ]));
        let provider = AnthropicProvider::with_transport(
            "https://api.anthropic.com".to_string(),
            "bad-key".to_string(),
            "claude-sonnet-4-20250514".to_string(),
            transport.clone(),
        );

        let result = provider.complete("hello").await;
        assert!(result.is_err());
        assert!(result
            .expect_err("401 should fail")
            .to_string()
            .contains("auth error (401)"));
        assert_eq!(transport.calls(), 1);
    }

    #[tokio::test]
    async fn complete_retries_on_rate_limit() {
        let transport = Arc::new(ScriptedTransport::new(vec![
            status_response(429, r#"{"error":{"message":"rate limited"}}"#),
            status_response(429, r#"{"error":{"message":"rate limited"}}"#),
            status_response(429, r#"{"error":{"message":"rate limited"}}"#),
        ]));
        let provider = AnthropicProvider::with_transport(
            "https://api.anthropic.com".to_string(),
            "sk-ant-test".to_string(),
            "claude-sonnet-4-20250514".to_string(),
            transport.clone(),
        );

        let result = provider.complete("hello").await;
        assert!(result.is_err());
        assert!(result
            .expect_err("429 should fail after retries")
            .to_string()
            .contains("rate limited (429)"));
        assert_eq!(transport.calls(), 3);
    }

    #[tokio::test]
    async fn complete_retries_on_transport_error() {
        let transport = Arc::new(ScriptedTransport::new(vec![
            Err(TransportError::new(TransportErrorKind::Timeout, "timeout")),
            ok_response(SUCCESS_BODY),
        ]));
        let provider = AnthropicProvider::with_transport(
            "https://api.anthropic.com".to_string(),
            "sk-ant-test".to_string(),
            "claude-sonnet-4-20250514".to_string(),
            transport.clone(),
        );

        let result = provider
            .complete("hello")
            .await
            .expect("should retry and succeed");
        assert_eq!(result.output_text, "Hello!");
        assert_eq!(transport.calls(), 2);
    }
}
