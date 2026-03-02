#[allow(unused_imports)]
use crate::transport::TransportErrorKind;
use crate::transport::{
    collect_headers, jittered_backoff, log_request, log_response, log_retry, map_reqwest_error,
    map_status_error, parse_retry_after, read_response_body, should_retry_status,
    should_retry_transport, CircuitBreaker, TransportConfig, TransportError, TransportResponse,
};
use agentzero_core::{ChatResult, Provider, ReasoningConfig, StreamChunk};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tokio::time::sleep;
use tracing::trace;

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
    fn new(config: &TransportConfig) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(config.timeout_ms))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
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
    circuit_breaker: Arc<CircuitBreaker>,
    config: TransportConfig,
}

impl AnthropicProvider {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        let config = TransportConfig::default();
        Self {
            transport: Arc::new(ReqwestTransport::new(&config)),
            circuit_breaker: Arc::new(CircuitBreaker::from_config(&config)),
            base_url,
            api_key,
            model,
            config,
        }
    }

    pub fn with_config(
        base_url: String,
        api_key: String,
        model: String,
        config: TransportConfig,
    ) -> Self {
        Self {
            transport: Arc::new(ReqwestTransport::new(&config)),
            circuit_breaker: Arc::new(CircuitBreaker::from_config(&config)),
            base_url,
            api_key,
            model,
            config,
        }
    }

    #[cfg(test)]
    fn with_transport(
        base_url: String,
        api_key: String,
        model: String,
        transport: Arc<dyn HttpTransport>,
    ) -> Self {
        let config = TransportConfig::default();
        Self {
            circuit_breaker: Arc::new(CircuitBreaker::from_config(&config)),
            base_url,
            api_key,
            model,
            transport,
            config,
        }
    }

    /// Circuit breaker state for health checks.
    pub fn circuit_state(&self) -> &'static str {
        self.circuit_breaker.state_label()
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
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Message {
    role: String,
    content: MessageContent,
}

/// Anthropic messages support both a plain string and an array of content blocks.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Blocks(Vec<InputContentBlock>),
}

/// Content blocks that can appear in user messages.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
enum InputContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: ImageSource },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ImageSource {
    #[serde(rename = "type")]
    kind: String,
    media_type: String,
    data: String,
}

#[derive(Debug, Serialize)]
struct ThinkingConfig {
    #[serde(rename = "type")]
    kind: String,
    budget_tokens: u32,
}

// --- Response types ---

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    #[allow(dead_code)]
    input_tokens: u64,
    #[allow(dead_code)]
    output_tokens: u64,
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
    #[serde(rename = "tool_use")]
    ToolUse {
        #[allow(dead_code)]
        id: String,
        #[allow(dead_code)]
        name: String,
        #[allow(dead_code)]
        input: serde_json::Value,
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

    if let Some(ref usage) = response.usage {
        trace!(
            input_tokens = usage.input_tokens,
            output_tokens = usage.output_tokens,
            stop_reason = response.stop_reason.as_deref().unwrap_or("none"),
            "Anthropic response metadata"
        );
    }

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
        // Check circuit breaker before sending.
        self.circuit_breaker.check().await?;

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

        // Extract system prompt if the user message starts with a system marker.
        // Otherwise, pass the entire prompt as a user message.
        let (system, user_text) = extract_system_prompt(prompt);

        let payload = MessagesRequest {
            model: self.model.clone(),
            max_tokens,
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Text(user_text.to_string()),
            }],
            system,
            thinking,
            stream: false,
        };

        log_request("anthropic", &url, &self.model);
        let start = Instant::now();
        let max_retries = self.config.max_retries;

        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 0..max_retries {
            match self.transport.send(&url, &self.api_key, &payload).await {
                Ok(response) => {
                    log_response(
                        "anthropic",
                        response.status,
                        response.body.len(),
                        start.elapsed(),
                    );
                    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
                    if !status.is_success() {
                        self.circuit_breaker.record_failure().await;
                        let mapped = map_status_error(status, &response.body);
                        if attempt + 1 < max_retries && should_retry_status(status) {
                            log_retry("anthropic", attempt, &format!("status {}", response.status));
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

                    self.circuit_breaker.record_success();
                    let output_text = parse_output_text(&response.body)
                        .with_context(|| "failed to parse Anthropic response".to_string())?;
                    return Ok(ChatResult { output_text });
                }
                Err(error) => {
                    self.circuit_breaker.record_failure().await;
                    let mapped = anyhow!("provider request failed: {}", error.message);
                    if attempt + 1 < max_retries && should_retry_transport(error.kind) {
                        log_retry("anthropic", attempt, &error.message);
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

    async fn complete_streaming(
        &self,
        prompt: &str,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        self.circuit_breaker.check().await?;

        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let (system, user_text) = extract_system_prompt(prompt);
        let payload = MessagesRequest {
            model: self.model.clone(),
            max_tokens: DEFAULT_MAX_TOKENS,
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Text(user_text.to_string()),
            }],
            system,
            thinking: None,
            stream: true,
        };

        log_request("anthropic", &url, &self.model);
        let start = Instant::now();

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(self.config.timeout_ms))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let response = match client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                self.circuit_breaker.record_failure().await;
                return Err(anyhow!("streaming request failed: {e}"));
            }
        };

        if !response.status().is_success() {
            self.circuit_breaker.record_failure().await;
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(map_status_error(status, &body));
        }

        let mut byte_stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut accumulated = String::new();

        while let Some(chunk_result) = byte_stream.next().await {
            let bytes = chunk_result.context("error reading SSE chunk")?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(event_end) = buffer.find("\n\n") {
                let event_block = buffer[..event_end].to_string();
                buffer = buffer[event_end + 2..].to_string();

                if let Some(delta) = parse_sse_text_delta(&event_block) {
                    accumulated.push_str(&delta);
                    let _ = sender.send(StreamChunk { delta, done: false });
                }
            }
        }

        self.circuit_breaker.record_success();
        log_response("anthropic", 200, accumulated.len(), start.elapsed());

        let _ = sender.send(StreamChunk {
            delta: String::new(),
            done: true,
        });

        Ok(ChatResult {
            output_text: accumulated,
        })
    }
}

/// Parse a text delta from an Anthropic SSE event block.
/// Anthropic SSE format: `event: content_block_delta\ndata: {"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}`
fn parse_sse_text_delta(event_block: &str) -> Option<String> {
    let mut data_line = None;
    for line in event_block.lines() {
        if let Some(rest) = line.strip_prefix("data: ") {
            data_line = Some(rest);
        }
    }
    let data = data_line?;
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    if value.get("type")?.as_str()? == "content_block_delta" {
        let delta = value.get("delta")?;
        if delta.get("type")?.as_str()? == "text_delta" {
            return delta.get("text")?.as_str().map(|s| s.to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// System prompt extraction
// ---------------------------------------------------------------------------

/// Extract a system prompt from the user message. If the message begins with
/// `<system>...</system>` tags, the content within the tags is extracted as
/// the system prompt, and the remainder is the user message.
fn extract_system_prompt(prompt: &str) -> (Option<String>, &str) {
    let trimmed = prompt.trim_start();
    if let Some(after_open) = trimmed.strip_prefix("<system>") {
        if let Some(close_idx) = after_open.find("</system>") {
            let system_text = after_open[..close_idx].trim();
            let remainder = after_open[close_idx + "</system>".len()..].trim_start();
            if system_text.is_empty() {
                return (None, remainder);
            }
            return (Some(system_text.to_string()), remainder);
        }
    }
    (None, prompt)
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

    /// Transport that captures the payload for assertion.
    struct CapturingTransport {
        payloads: Mutex<Vec<String>>,
    }

    impl CapturingTransport {
        fn new() -> Self {
            Self {
                payloads: Mutex::new(Vec::new()),
            }
        }

        fn captured_payloads(&self) -> Vec<serde_json::Value> {
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
        async fn send(
            &self,
            _url: &str,
            _api_key: &str,
            payload: &MessagesRequest,
        ) -> Result<TransportResponse, TransportError> {
            let json = serde_json::to_string(payload).expect("payload should serialize");
            self.payloads.lock().expect("lock").push(json);
            Ok(TransportResponse {
                status: 200,
                headers: HashMap::new(),
                body: SUCCESS_BODY.to_string(),
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

    const SUCCESS_BODY: &str = r#"{"content":[{"type":"text","text":"Hello!"}],"model":"claude-sonnet-4-20250514","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5}}"#;

    // --- parse tests ---

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
    fn parse_output_text_skips_tool_use_blocks() {
        let body = r#"{"content":[{"type":"tool_use","id":"tu_1","name":"search","input":{"q":"test"}},{"type":"text","text":"result"}]}"#;
        let text = parse_output_text(body).expect("should parse");
        assert_eq!(text, "result");
    }

    #[test]
    fn parse_output_text_fails_on_empty_content() {
        let body = r#"{"content":[]}"#;
        assert!(parse_output_text(body).is_err());
    }

    #[test]
    fn parse_output_text_with_usage_and_stop_reason() {
        let body = r#"{"content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}"#;
        let text = parse_output_text(body).expect("should parse");
        assert_eq!(text, "ok");
    }

    // --- system prompt extraction ---

    #[test]
    fn extract_system_prompt_with_tags() {
        let (sys, user) = extract_system_prompt("<system>You are helpful.</system>What is 2+2?");
        assert_eq!(sys, Some("You are helpful.".to_string()));
        assert_eq!(user, "What is 2+2?");
    }

    #[test]
    fn extract_system_prompt_without_tags() {
        let (sys, user) = extract_system_prompt("What is 2+2?");
        assert!(sys.is_none());
        assert_eq!(user, "What is 2+2?");
    }

    #[test]
    fn extract_system_prompt_empty_system_tag() {
        let (sys, user) = extract_system_prompt("<system></system>What is 2+2?");
        assert!(sys.is_none());
        assert_eq!(user, "What is 2+2?");
    }

    #[test]
    fn extract_system_prompt_with_whitespace() {
        let (sys, user) = extract_system_prompt("  <system> Be concise. </system>  Hello");
        assert_eq!(sys, Some("Be concise.".to_string()));
        assert_eq!(user, "Hello");
    }

    // --- provider integration tests ---

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
            status_response(
                401,
                r#"{"error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
            ),
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
        let err = result.expect_err("401 should fail").to_string();
        assert!(err.contains("auth error (401)"));
        assert!(err.contains("[authentication_error]"));
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

    #[tokio::test]
    async fn system_prompt_sent_as_top_level_field() {
        let transport = Arc::new(CapturingTransport::new());
        let provider = AnthropicProvider::with_transport(
            "https://api.anthropic.com".to_string(),
            "sk-ant-test".to_string(),
            "claude-sonnet-4-20250514".to_string(),
            transport.clone(),
        );

        provider
            .complete("<system>You are a calculator.</system>What is 2+2?")
            .await
            .expect("should succeed");

        let payloads = transport.captured_payloads();
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0]["system"], "You are a calculator.");
        assert_eq!(payloads[0]["messages"][0]["content"], "What is 2+2?");
    }

    #[tokio::test]
    async fn no_system_prompt_omits_field() {
        let transport = Arc::new(CapturingTransport::new());
        let provider = AnthropicProvider::with_transport(
            "https://api.anthropic.com".to_string(),
            "sk-ant-test".to_string(),
            "claude-sonnet-4-20250514".to_string(),
            transport.clone(),
        );

        provider.complete("hello").await.expect("should succeed");

        let payloads = transport.captured_payloads();
        assert_eq!(payloads.len(), 1);
        assert!(
            payloads[0].get("system").is_none(),
            "system field should be omitted when no system prompt"
        );
    }

    #[tokio::test]
    async fn circuit_breaker_blocks_after_consecutive_failures() {
        // Use a low threshold so we can trip it in 2 failures.
        let config = TransportConfig {
            circuit_breaker_threshold: 2,
            ..TransportConfig::default()
        };

        let transport = Arc::new(ScriptedTransport::new(vec![
            status_response(500, r#"{"error":{"message":"internal"}}"#),
            status_response(500, r#"{"error":{"message":"internal"}}"#),
            status_response(500, r#"{"error":{"message":"internal"}}"#),
            status_response(500, r#"{"error":{"message":"internal"}}"#),
            status_response(500, r#"{"error":{"message":"internal"}}"#),
            status_response(500, r#"{"error":{"message":"internal"}}"#),
        ]));

        let cb = Arc::new(CircuitBreaker::from_config(&config));
        let provider = AnthropicProvider {
            base_url: "https://api.anthropic.com".to_string(),
            api_key: "sk-ant-test".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            transport: transport.clone(),
            circuit_breaker: cb.clone(),
            config: config.clone(),
        };

        // First call: 3 retries, all fail → circuit opens (3 failures > threshold of 2).
        let result1 = provider.complete("hello").await;
        assert!(result1.is_err());
        assert_eq!(cb.state_label(), "open");

        // Second call: circuit breaker rejects immediately.
        let result2 = provider.complete("hello").await;
        assert!(result2.is_err());
        assert!(result2
            .unwrap_err()
            .to_string()
            .contains("circuit breaker open"));
        // No additional transport calls — blocked by breaker.
        assert_eq!(transport.calls(), 3);
    }

    #[tokio::test]
    async fn circuit_breaker_resets_after_success() {
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

        let result = provider.complete("hello").await.expect("should recover");
        assert_eq!(result.output_text, "Hello!");
        assert_eq!(provider.circuit_state(), "closed");
    }

    // --- Content block parsing ---

    #[test]
    fn parse_response_with_tool_use_block() {
        let body = r#"{
            "content": [
                {"type": "tool_use", "id": "tu_1", "name": "web_search", "input": {"query": "test"}},
                {"type": "text", "text": "Here are the results."}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 50, "output_tokens": 25}
        }"#;
        let text = parse_output_text(body).expect("should parse");
        assert_eq!(text, "Here are the results.");
    }

    #[test]
    fn parse_response_only_tool_use_fails() {
        let body = r#"{
            "content": [
                {"type": "tool_use", "id": "tu_1", "name": "web_search", "input": {"query": "test"}}
            ],
            "stop_reason": "tool_use"
        }"#;
        let result = parse_output_text(body);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no text content blocks"));
    }

    // --- Message content types ---

    #[test]
    fn message_content_serializes_text_as_string() {
        let msg = Message {
            role: "user".to_string(),
            content: MessageContent::Text("hello".to_string()),
        };
        let json = serde_json::to_value(&msg).expect("should serialize");
        assert_eq!(json["content"], "hello");
    }

    #[test]
    fn message_content_serializes_blocks_as_array() {
        let msg = Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![
                InputContentBlock::Text {
                    text: "Describe this image:".to_string(),
                },
                InputContentBlock::Image {
                    source: ImageSource {
                        kind: "base64".to_string(),
                        media_type: "image/png".to_string(),
                        data: "iVBOR...".to_string(),
                    },
                },
            ]),
        };
        let json = serde_json::to_value(&msg).expect("should serialize");
        let blocks = json["content"].as_array().expect("should be array");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["source"]["type"], "base64");
    }

    #[test]
    fn input_content_block_tool_result_serializes() {
        let block = InputContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: "42".to_string(),
        };
        let json = serde_json::to_value(&block).expect("should serialize");
        assert_eq!(json["type"], "tool_result");
        assert_eq!(json["tool_use_id"], "tu_1");
        assert_eq!(json["content"], "42");
    }

    // --- SSE parsing ---

    #[test]
    fn parse_sse_text_delta_extracts_content() {
        let event = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}";
        let delta = parse_sse_text_delta(event);
        assert_eq!(delta, Some("Hello".to_string()));
    }

    #[test]
    fn parse_sse_text_delta_ignores_non_delta_events() {
        let event = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\"}}";
        assert!(parse_sse_text_delta(event).is_none());
    }

    #[test]
    fn parse_sse_text_delta_ignores_thinking_delta() {
        let event = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"reasoning\"}}";
        assert!(parse_sse_text_delta(event).is_none());
    }

    #[test]
    fn stream_request_sets_stream_true() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 1024,
            messages: vec![],
            system: None,
            thinking: None,
            stream: true,
        };
        let json = serde_json::to_value(&req).expect("should serialize");
        assert_eq!(json["stream"], true);
    }

    #[test]
    fn non_stream_request_omits_stream_field() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 1024,
            messages: vec![],
            system: None,
            thinking: None,
            stream: false,
        };
        let json = serde_json::to_value(&req).expect("should serialize");
        assert!(json.get("stream").is_none());
    }
}
