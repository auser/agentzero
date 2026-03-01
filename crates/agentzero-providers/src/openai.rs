use agentzero_core::{ChatResult, Provider, ReasoningConfig};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

const MAX_ATTEMPTS: usize = 3;
const BASE_BACKOFF_MS: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransportErrorKind {
    Timeout,
    Connect,
    Request,
    Body,
    Other,
}

#[derive(Debug)]
struct TransportError {
    kind: TransportErrorKind,
    message: String,
}

impl TransportError {
    fn new(kind: TransportErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

struct TransportResponse {
    status: u16,
    headers: HashMap<String, String>,
    body: String,
}

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
        let response = self
            .client
            .post(url)
            .bearer_auth(api_key)
            .json(payload)
            .send()
            .await
            .map_err(map_reqwest_error)?;

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

fn map_reqwest_error(err: reqwest::Error) -> TransportError {
    let kind = if err.is_timeout() {
        TransportErrorKind::Timeout
    } else if err.is_connect() {
        TransportErrorKind::Connect
    } else if err.is_request() {
        TransportErrorKind::Request
    } else if err.is_body() {
        TransportErrorKind::Body
    } else {
        TransportErrorKind::Other
    };
    TransportError::new(kind, err.to_string())
}

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

fn jittered_backoff(attempt_index: usize) -> Duration {
    let exp = BASE_BACKOFF_MS.saturating_mul(1_u64 << attempt_index.min(10));
    let jitter = ((attempt_index as u64).saturating_mul(37)) % 53;
    Duration::from_millis(exp.saturating_add(jitter))
}

fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn should_retry_transport(kind: TransportErrorKind) -> bool {
    matches!(
        kind,
        TransportErrorKind::Timeout
            | TransportErrorKind::Connect
            | TransportErrorKind::Request
            | TransportErrorKind::Body
    )
}

fn parse_retry_after(headers: &HashMap<String, String>) -> Option<Duration> {
    let retry_after = headers.get("retry-after")?.trim();
    let seconds = retry_after.parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds))
}

fn extract_error_message(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
}

fn map_status_error(status: StatusCode, body: &str) -> anyhow::Error {
    let message = extract_error_message(body).unwrap_or_else(|| "no error message".to_string());
    match status {
        StatusCode::UNAUTHORIZED => anyhow!("provider auth error (401): {message}"),
        StatusCode::TOO_MANY_REQUESTS => anyhow!("provider rate limited (429): {message}"),
        s if s.is_server_error() => anyhow!("provider server error ({}): {message}", s.as_u16()),
        _ => anyhow!("provider request failed ({}): {message}", status.as_u16()),
    }
}

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

    #[test]
    fn status_mapping_produces_specific_categories() {
        let auth = map_status_error(
            StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"bad key"}}"#,
        );
        assert!(auth.to_string().contains("auth error (401)"));

        let rate = map_status_error(
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"message":"slow down"}}"#,
        );
        assert!(rate.to_string().contains("rate limited (429)"));

        let server = map_status_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"error":{"message":"boom"}}"#,
        );
        assert!(server.to_string().contains("server error (500)"));
    }

    #[test]
    fn retry_policy_only_retries_transient_statuses() {
        assert!(should_retry_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(should_retry_status(StatusCode::BAD_GATEWAY));
        assert!(!should_retry_status(StatusCode::UNAUTHORIZED));
        assert!(!should_retry_status(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn backoff_increases_with_attempts() {
        let first = jittered_backoff(0);
        let second = jittered_backoff(1);
        let third = jittered_backoff(2);
        assert!(second > first);
        assert!(third > second);
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
}
