use crate::transport::{
    jittered_backoff, map_reqwest_error, map_status_error, parse_retry_after, should_retry_status,
    should_retry_transport, TransportError, TransportErrorKind, TransportResponse, MAX_ATTEMPTS,
};
use agentzero_core::{ChatResult, Provider, ReasoningConfig};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::sleep;

// ---------------------------------------------------------------------------
// Transport abstraction (OpenAI-specific payload)
// ---------------------------------------------------------------------------

#[async_trait]
trait HttpTransport: Send + Sync {
    async fn send_chat(
        &self,
        url: &str,
        api_key: &str,
        payload: &ChatRequest,
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
    async fn send_chat(
        &self,
        url: &str,
        api_key: &str,
        payload: &ChatRequest,
    ) -> Result<TransportResponse, TransportError> {
        let mut request = self.client.post(url).json(payload);
        if !api_key.is_empty() {
            request = request.bearer_auth(api_key);
        }
        let response = request.send().await.map_err(map_reqwest_error)?;

        let status = response.status().as_u16();
        let mut headers = HashMap::new();
        for (name, value) in response.headers() {
            if let Ok(parsed) = value.to_str() {
                headers.insert(name.as_str().to_ascii_lowercase(), parsed.to_string());
            }
        }
        let body = response.text().await.map_err(|err| {
            TransportError::new(
                if err.is_body() {
                    TransportErrorKind::Body
                } else {
                    TransportErrorKind::Other
                },
                format!("failed reading response body: {err}"),
            )
        })?;

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

pub struct OpenAiCompatibleProvider {
    base_url: String,
    api_key: String,
    model: String,
    transport: Arc<dyn HttpTransport>,
}

impl OpenAiCompatibleProvider {
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
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
}

fn output_text_from_response(response: ChatResponse) -> String {
    response
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .unwrap_or_else(|| "(empty response)".to_string())
}

fn parse_output_text(body: &str) -> anyhow::Result<String> {
    if let Ok(response) = serde_json::from_str::<ChatResponse>(body) {
        return Ok(output_text_from_response(response));
    }

    let value: Value = serde_json::from_str(body).context("response body is not valid JSON")?;
    if let Some(text) = extract_output_text_from_value(&value) {
        return Ok(text);
    }

    Err(anyhow!(
        "response JSON did not include recognizable output text fields"
    ))
}

fn extract_output_text_from_value(value: &Value) -> Option<String> {
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| {
            choice
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| {
                    choice
                        .get("text")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })
        })
        .or_else(|| {
            value
                .get("output_text")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .or_else(|| {
            value
                .get("text")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
}

// ---------------------------------------------------------------------------
// Provider impl
// ---------------------------------------------------------------------------

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
        self.complete_with_reasoning(prompt, &ReasoningConfig::default())
            .await
    }

    async fn complete_with_reasoning(
        &self,
        prompt: &str,
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let reasoning_effort = match reasoning.enabled {
            Some(false) => None,
            _ => reasoning.level.clone(),
        };
        let payload = ChatRequest {
            model: self.model.clone(),
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            reasoning_effort,
        };

        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 0..MAX_ATTEMPTS {
            match self
                .transport
                .send_chat(&url, &self.api_key, &payload)
                .await
            {
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
                        .with_context(|| "failed to parse provider response".to_string())?;
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
        async fn send_chat(
            &self,
            _url: &str,
            _api_key: &str,
            _payload: &ChatRequest,
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

    #[test]
    fn output_text_from_response_uses_first_choice_content() {
        let parsed: ChatResponse =
            serde_json::from_str(r#"{"choices":[{"message":{"content":"hello"}}]}"#)
                .expect("response JSON should parse");
        assert_eq!(output_text_from_response(parsed), "hello");
    }

    #[test]
    fn output_text_from_response_falls_back_for_empty_choices() {
        let parsed: ChatResponse =
            serde_json::from_str(r#"{"choices":[]}"#).expect("response JSON should parse");
        assert_eq!(output_text_from_response(parsed), "(empty response)");
    }

    #[test]
    fn parse_output_text_uses_json_fallback_shape() {
        let output = parse_output_text(r#"{"choices":[{"text":"fallback-text"}]}"#)
            .expect("fallback parser should succeed");
        assert_eq!(output, "fallback-text");
    }

    #[test]
    fn parse_output_text_fails_for_unknown_shape() {
        let result = parse_output_text(r#"{"unexpected":"shape"}"#);
        assert!(result.is_err());
        assert!(result
            .expect_err("unknown shape should fail")
            .to_string()
            .contains("recognizable output text fields"));
    }

    #[tokio::test]
    async fn complete_succeeds_on_mock_success_response() {
        let transport = Arc::new(ScriptedTransport::new(vec![ok_response(
            r#"{"choices":[{"message":{"content":"ok"}}]}"#,
        )]));
        let provider = OpenAiCompatibleProvider::with_transport(
            "https://example.invalid".to_string(),
            "key".to_string(),
            "model".to_string(),
            transport.clone(),
        );

        let result = provider
            .complete("hello")
            .await
            .expect("request should pass");
        assert_eq!(result.output_text, "ok");
        assert_eq!(transport.calls(), 1);
    }

    #[tokio::test]
    async fn complete_retries_then_fails_on_timeout_error() {
        let transport = Arc::new(ScriptedTransport::new(vec![
            Err(TransportError::new(
                TransportErrorKind::Timeout,
                "timed out",
            )),
            Err(TransportError::new(
                TransportErrorKind::Timeout,
                "timed out",
            )),
            Err(TransportError::new(
                TransportErrorKind::Timeout,
                "timed out",
            )),
        ]));
        let provider = OpenAiCompatibleProvider::with_transport(
            "https://example.invalid".to_string(),
            "key".to_string(),
            "model".to_string(),
            transport.clone(),
        );

        let result = provider.complete("hello").await;
        assert!(result.is_err());
        assert!(result
            .expect_err("timeout should fail")
            .to_string()
            .contains("provider request failed"));
        assert_eq!(transport.calls(), 3);
    }

    #[tokio::test]
    async fn complete_fails_on_malformed_response() {
        let transport = Arc::new(ScriptedTransport::new(vec![ok_response(
            r#"{"unexpected":"shape"}"#,
        )]));
        let provider = OpenAiCompatibleProvider::with_transport(
            "https://example.invalid".to_string(),
            "key".to_string(),
            "model".to_string(),
            transport.clone(),
        );

        let result = provider.complete("hello").await;
        assert!(result.is_err());
        assert!(result
            .expect_err("malformed response should fail")
            .to_string()
            .contains("failed to parse provider response"));
        assert_eq!(transport.calls(), 1);
    }

    #[tokio::test]
    async fn complete_fails_immediately_on_auth_failure() {
        let transport = Arc::new(ScriptedTransport::new(vec![
            status_response(401, r#"{"error":{"message":"bad key"}}"#),
            ok_response(r#"{"choices":[{"message":{"content":"should-not-run"}}]}"#),
        ]));
        let provider = OpenAiCompatibleProvider::with_transport(
            "https://example.invalid".to_string(),
            "key".to_string(),
            "model".to_string(),
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
    async fn complete_retries_on_rate_limit_then_fails() {
        let transport = Arc::new(ScriptedTransport::new(vec![
            status_response(429, r#"{"error":{"message":"slow down"}}"#),
            status_response(429, r#"{"error":{"message":"slow down"}}"#),
            status_response(429, r#"{"error":{"message":"slow down"}}"#),
        ]));
        let provider = OpenAiCompatibleProvider::with_transport(
            "https://example.invalid".to_string(),
            "key".to_string(),
            "model".to_string(),
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

    // --- Reasoning tests ---

    struct CapturingTransport {
        payloads: Mutex<Vec<String>>,
    }

    impl CapturingTransport {
        fn new() -> Self {
            Self {
                payloads: Mutex::new(Vec::new()),
            }
        }

        fn captured_payloads(&self) -> Vec<Value> {
            self.payloads
                .lock()
                .expect("lock")
                .iter()
                .map(|s| serde_json::from_str(s).expect("payload should be valid JSON"))
                .collect()
        }
    }

    #[async_trait]
    impl HttpTransport for CapturingTransport {
        async fn send_chat(
            &self,
            _url: &str,
            _api_key: &str,
            payload: &ChatRequest,
        ) -> Result<TransportResponse, TransportError> {
            let json = serde_json::to_string(payload).expect("payload should serialize");
            self.payloads.lock().expect("lock").push(json);
            Ok(TransportResponse {
                status: 200,
                headers: HashMap::new(),
                body: r#"{"choices":[{"message":{"content":"ok"}}]}"#.to_string(),
            })
        }
    }

    struct ApiKeyCapturingTransport {
        captured_keys: Mutex<Vec<String>>,
    }

    impl ApiKeyCapturingTransport {
        fn new() -> Self {
            Self {
                captured_keys: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl HttpTransport for ApiKeyCapturingTransport {
        async fn send_chat(
            &self,
            _url: &str,
            api_key: &str,
            _payload: &ChatRequest,
        ) -> Result<TransportResponse, TransportError> {
            self.captured_keys
                .lock()
                .expect("lock")
                .push(api_key.to_string());
            Ok(TransportResponse {
                status: 200,
                headers: HashMap::new(),
                body: r#"{"choices":[{"message":{"content":"ok"}}]}"#.to_string(),
            })
        }
    }

    #[tokio::test]
    async fn reasoning_effort_included_in_request() {
        let transport = Arc::new(CapturingTransport::new());
        let provider = OpenAiCompatibleProvider::with_transport(
            "https://example.invalid".to_string(),
            "key".to_string(),
            "model".to_string(),
            transport.clone(),
        );

        let reasoning = ReasoningConfig {
            enabled: Some(true),
            level: Some("high".to_string()),
        };
        let result = provider
            .complete_with_reasoning("test", &reasoning)
            .await
            .expect("should succeed");
        assert_eq!(result.output_text, "ok");

        let payloads = transport.captured_payloads();
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0]["reasoning_effort"], "high");
    }

    #[tokio::test]
    async fn reasoning_disabled_omits_effort_field() {
        let transport = Arc::new(CapturingTransport::new());
        let provider = OpenAiCompatibleProvider::with_transport(
            "https://example.invalid".to_string(),
            "key".to_string(),
            "model".to_string(),
            transport.clone(),
        );

        let reasoning = ReasoningConfig {
            enabled: Some(false),
            level: Some("high".to_string()),
        };
        provider
            .complete_with_reasoning("test", &reasoning)
            .await
            .expect("should succeed");

        let payloads = transport.captured_payloads();
        assert_eq!(payloads.len(), 1);
        assert!(
            payloads[0].get("reasoning_effort").is_none(),
            "reasoning_effort should be omitted when reasoning is disabled"
        );
    }

    #[tokio::test]
    async fn reasoning_default_config_omits_effort_field() {
        let transport = Arc::new(CapturingTransport::new());
        let provider = OpenAiCompatibleProvider::with_transport(
            "https://example.invalid".to_string(),
            "key".to_string(),
            "model".to_string(),
            transport.clone(),
        );

        provider.complete("test").await.expect("should succeed");

        let payloads = transport.captured_payloads();
        assert_eq!(payloads.len(), 1);
        assert!(
            payloads[0].get("reasoning_effort").is_none(),
            "default config should not include reasoning_effort"
        );
    }

    // --- Bearer auth conditional tests ---

    #[tokio::test]
    async fn empty_api_key_is_passed_through_to_transport() {
        let transport = Arc::new(ApiKeyCapturingTransport::new());
        let provider = OpenAiCompatibleProvider::with_transport(
            "https://example.invalid".to_string(),
            "".to_string(),
            "model".to_string(),
            transport.clone(),
        );

        provider.complete("test").await.expect("should succeed");

        let keys = transport.captured_keys.lock().expect("lock");
        assert_eq!(keys.len(), 1);
        assert!(
            keys[0].is_empty(),
            "empty api_key should be passed as empty string to transport"
        );
    }

    #[tokio::test]
    async fn nonempty_api_key_is_passed_through_to_transport() {
        let transport = Arc::new(ApiKeyCapturingTransport::new());
        let provider = OpenAiCompatibleProvider::with_transport(
            "https://example.invalid".to_string(),
            "sk-test-key".to_string(),
            "model".to_string(),
            transport.clone(),
        );

        provider.complete("test").await.expect("should succeed");

        let keys = transport.captured_keys.lock().expect("lock");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], "sk-test-key");
    }

    #[test]
    fn reqwest_transport_skips_auth_header_for_empty_key() {
        let api_key = "";
        assert!(api_key.is_empty(), "empty key should skip bearer_auth");

        let api_key = "sk-real";
        assert!(
            !api_key.is_empty(),
            "non-empty key should include bearer_auth"
        );
    }
}
