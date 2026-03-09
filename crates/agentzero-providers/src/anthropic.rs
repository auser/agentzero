#[allow(unused_imports)]
use crate::transport::TransportErrorKind;
use crate::transport::{
    collect_headers, jittered_backoff, log_request, log_response, log_retry, map_reqwest_error,
    map_status_error, parse_retry_after, read_response_body, should_retry_status,
    should_retry_transport, CircuitBreaker, TransportConfig, TransportError, TransportResponse,
};
use agentzero_core::{
    ChatResult, ContentPart, ConversationMessage, Provider, ReasoningConfig, StopReason,
    StreamChunk, ToolCallDelta, ToolDefinition, ToolUseRequest,
};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tokio::time::sleep;
use tracing::{info_span, trace, Instrument};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 8192;
/// Beta flag required when authenticating with OAuth tokens.
const ANTHROPIC_OAUTH_BETA: &str = "oauth-2025-04-20";

// ---------------------------------------------------------------------------
// Transport abstraction (Anthropic-specific payload)
// ---------------------------------------------------------------------------

#[async_trait]
trait HttpTransport: Send + Sync {
    async fn send(
        &self,
        url: &str,
        api_key: &str,
        use_bearer_auth: bool,
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

/// Apply the appropriate auth header for the Anthropic API.
/// API keys use `x-api-key`; OAuth tokens use `Authorization: Bearer`.
fn apply_anthropic_auth(
    builder: reqwest::RequestBuilder,
    api_key: &str,
    use_bearer_auth: bool,
) -> reqwest::RequestBuilder {
    if use_bearer_auth {
        builder.bearer_auth(api_key)
    } else {
        builder.header("x-api-key", api_key)
    }
}

#[async_trait]
impl HttpTransport for ReqwestTransport {
    async fn send(
        &self,
        url: &str,
        api_key: &str,
        use_bearer_auth: bool,
        payload: &MessagesRequest,
    ) -> Result<TransportResponse, TransportError> {
        let request = self.client.post(url);
        let request = apply_anthropic_auth(request, api_key, use_bearer_auth);
        let mut request = request
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json");
        if use_bearer_auth {
            request = request.header("anthropic-beta", ANTHROPIC_OAUTH_BETA);
        }
        let response = request
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
    /// OAuth tokens use `Authorization: Bearer` header instead of `x-api-key`.
    /// Detected by checking if the key lacks the `sk-ant-` prefix.
    use_bearer_auth: bool,
}

impl AnthropicProvider {
    /// Returns true if the key appears to be an OAuth token rather than an API key.
    /// Anthropic OAuth tokens use the `sk-ant-oat` prefix ("OAuth Access Token"),
    /// while regular API keys use prefixes like `sk-ant-api`. OAuth tokens must
    /// be sent as `Authorization: Bearer` instead of `x-api-key`.
    fn is_oauth_token(key: &str) -> bool {
        key.starts_with("sk-ant-oat") || (!key.is_empty() && !key.starts_with("sk-ant-"))
    }

    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        let config = TransportConfig::default();
        let use_bearer_auth = Self::is_oauth_token(&api_key);
        Self {
            transport: Arc::new(ReqwestTransport::new(&config)),
            circuit_breaker: Arc::new(CircuitBreaker::from_config(&config)),
            base_url,
            api_key,
            model,
            config,
            use_bearer_auth,
        }
    }

    pub fn with_config(
        base_url: String,
        api_key: String,
        model: String,
        config: TransportConfig,
    ) -> Self {
        let use_bearer_auth = Self::is_oauth_token(&api_key);
        Self {
            transport: Arc::new(ReqwestTransport::new(&config)),
            circuit_breaker: Arc::new(CircuitBreaker::from_config(&config)),
            base_url,
            api_key,
            model,
            config,
            use_bearer_auth,
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
        let use_bearer_auth = Self::is_oauth_token(&api_key);
        Self {
            circuit_breaker: Arc::new(CircuitBreaker::from_config(&config)),
            base_url,
            api_key,
            model,
            transport,
            config,
            use_bearer_auth,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicToolDef>>,
}

#[derive(Debug, Serialize)]
struct AnthropicToolDef {
    name: String,
    description: String,
    input_schema: serde_json::Value,
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
        id: String,
        name: String,
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

fn extract_tool_calls(response: &MessagesResponse) -> Vec<ToolUseRequest> {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } => Some(ToolUseRequest {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn map_stop_reason(reason: Option<&str>) -> Option<StopReason> {
    match reason {
        Some("end_turn") => Some(StopReason::EndTurn),
        Some("tool_use") => Some(StopReason::ToolUse),
        Some("max_tokens") => Some(StopReason::MaxTokens),
        Some("stop_sequence") => Some(StopReason::StopSequence),
        Some(other) => Some(StopReason::Other(other.to_string())),
        None => None,
    }
}

fn parse_tool_response(body: &str) -> anyhow::Result<ChatResult> {
    let response: MessagesResponse =
        serde_json::from_str(body).context("failed to parse Anthropic response JSON")?;

    let (input_tokens, output_tokens) = response
        .usage
        .as_ref()
        .map(|u| (u.input_tokens, u.output_tokens))
        .unwrap_or((0, 0));

    if input_tokens > 0 || output_tokens > 0 {
        trace!(
            input_tokens,
            output_tokens,
            stop_reason = response.stop_reason.as_deref().unwrap_or("none"),
            "Anthropic tool response metadata"
        );
    }

    let text = extract_text(&response);
    let tool_calls = extract_tool_calls(&response);
    let stop_reason = map_stop_reason(response.stop_reason.as_deref());

    Ok(ChatResult {
        output_text: text,
        tool_calls,
        stop_reason,
        input_tokens,
        output_tokens,
    })
}

fn to_anthropic_tool(def: &ToolDefinition) -> AnthropicToolDef {
    AnthropicToolDef {
        name: def.name.clone(),
        description: def.description.clone(),
        input_schema: def.input_schema.clone(),
    }
}

/// Extract the system prompt from the first `ConversationMessage::System` in the list.
fn extract_system_from_messages(messages: &[ConversationMessage]) -> Option<String> {
    messages.iter().find_map(|msg| match msg {
        ConversationMessage::System { content } => Some(content.clone()),
        _ => None,
    })
}

fn to_anthropic_messages(messages: &[ConversationMessage]) -> Vec<Message> {
    messages
        .iter()
        .filter_map(|msg| match msg {
            // System messages are extracted separately for the Anthropic API payload.
            ConversationMessage::System { .. } => None,
            ConversationMessage::User { content, parts } => {
                let msg_content = if parts.is_empty() {
                    MessageContent::Text(content.clone())
                } else {
                    let mut blocks = vec![InputContentBlock::Text {
                        text: content.clone(),
                    }];
                    for part in parts {
                        match part {
                            ContentPart::Text { text } => {
                                blocks.push(InputContentBlock::Text { text: text.clone() });
                            }
                            ContentPart::Image { media_type, data } => {
                                blocks.push(InputContentBlock::Image {
                                    source: ImageSource {
                                        kind: "base64".to_string(),
                                        media_type: media_type.clone(),
                                        data: data.clone(),
                                    },
                                });
                            }
                        }
                    }
                    MessageContent::Blocks(blocks)
                };
                Some(Message {
                    role: "user".to_string(),
                    content: msg_content,
                })
            }
            ConversationMessage::Assistant {
                content,
                tool_calls,
            } => {
                let mut blocks: Vec<InputContentBlock> = Vec::new();
                if let Some(text) = content {
                    if !text.is_empty() {
                        blocks.push(InputContentBlock::Text { text: text.clone() });
                    }
                }
                for tc in tool_calls {
                    blocks.push(InputContentBlock::ToolUse {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        input: tc.input.clone(),
                    });
                }
                Some(Message {
                    role: "assistant".to_string(),
                    content: if blocks.is_empty() {
                        MessageContent::Text(String::new())
                    } else {
                        MessageContent::Blocks(blocks)
                    },
                })
            }
            ConversationMessage::ToolResult(result) => Some(Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![InputContentBlock::ToolResult {
                    tool_use_id: result.tool_use_id.clone(),
                    content: result.content.clone(),
                }]),
            }),
        })
        .collect()
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
        let span = info_span!("anthropic_complete", provider = "anthropic", model = %self.model);
        async {
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
                tools: None,
            };

            log_request("anthropic", &url, &self.model);
            let start = Instant::now();
            let max_retries = self.config.max_retries;

            let mut last_error: Option<anyhow::Error> = None;
            for attempt in 0..max_retries {
                match self
                    .transport
                    .send(&url, &self.api_key, self.use_bearer_auth, &payload)
                    .await
                {
                    Ok(response) => {
                        log_response(
                            "anthropic",
                            response.status,
                            response.body.len(),
                            start.elapsed(),
                        );
                        let status =
                            StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
                        if !status.is_success() {
                            self.circuit_breaker.record_failure().await;
                            let mapped = map_status_error(status, &response.body);
                            if attempt + 1 < max_retries && should_retry_status(status) {
                                log_retry(
                                    "anthropic",
                                    attempt,
                                    &format!("status {}", response.status),
                                );
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
                        return Ok(ChatResult {
                            output_text,
                            ..Default::default()
                        });
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
        .instrument(span)
        .await
    }

    async fn complete_streaming(
        &self,
        prompt: &str,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        let span = info_span!("anthropic_stream", provider = "anthropic", model = %self.model);
        async {
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
                tools: None,
            };

            log_request("anthropic", &url, &self.model);
            let start = Instant::now();

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(self.config.timeout_ms))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());

            let request = client.post(&url);
            let request = apply_anthropic_auth(request, &self.api_key, self.use_bearer_auth);
            let mut request = request
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json");
            if self.use_bearer_auth {
                request = request.header("anthropic-beta", ANTHROPIC_OAUTH_BETA);
            }
            let response = match request.json(&payload).send().await {
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
                        let _ = sender.send(StreamChunk {
                            delta,
                            done: false,
                            tool_call_delta: None,
                        });
                    }
                }
            }

            self.circuit_breaker.record_success();
            log_response("anthropic", 200, accumulated.len(), start.elapsed());

            let _ = sender.send(StreamChunk {
                delta: String::new(),
                done: true,
                tool_call_delta: None,
            });

            Ok(ChatResult {
                output_text: accumulated,
                ..Default::default()
            })
        }
        .instrument(span)
        .await
    }

    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        let span =
            info_span!("anthropic_complete_tools", provider = "anthropic", model = %self.model);
        async {
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
                thinking
                    .as_ref()
                    .map(|t| t.budget_tokens.saturating_add(DEFAULT_MAX_TOKENS))
                    .unwrap_or(DEFAULT_MAX_TOKENS)
            } else {
                DEFAULT_MAX_TOKENS
            };

            let anthropic_tools: Vec<AnthropicToolDef> =
                tools.iter().map(to_anthropic_tool).collect();
            let system = extract_system_from_messages(messages);

            let payload = MessagesRequest {
                model: self.model.clone(),
                max_tokens,
                messages: to_anthropic_messages(messages),
                system,
                thinking,
                stream: false,
                tools: if anthropic_tools.is_empty() {
                    None
                } else {
                    Some(anthropic_tools)
                },
            };

            log_request("anthropic", &url, &self.model);
            let start = Instant::now();
            let max_retries = self.config.max_retries;

            let mut last_error: Option<anyhow::Error> = None;
            for attempt in 0..max_retries {
                match self
                    .transport
                    .send(&url, &self.api_key, self.use_bearer_auth, &payload)
                    .await
                {
                    Ok(response) => {
                        log_response(
                            "anthropic",
                            response.status,
                            response.body.len(),
                            start.elapsed(),
                        );
                        let status =
                            StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
                        if !status.is_success() {
                            self.circuit_breaker.record_failure().await;
                            let mapped = map_status_error(status, &response.body);
                            if attempt + 1 < max_retries && should_retry_status(status) {
                                log_retry(
                                    "anthropic",
                                    attempt,
                                    &format!("status {}", response.status),
                                );
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
                        return parse_tool_response(&response.body)
                            .with_context(|| "failed to parse Anthropic tool response");
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
        .instrument(span)
        .await
    }

    async fn complete_streaming_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        let span =
            info_span!("anthropic_stream_tools", provider = "anthropic", model = %self.model);
        async {
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
                thinking
                    .as_ref()
                    .map(|t| t.budget_tokens.saturating_add(DEFAULT_MAX_TOKENS))
                    .unwrap_or(DEFAULT_MAX_TOKENS)
            } else {
                DEFAULT_MAX_TOKENS
            };

            let anthropic_tools: Vec<AnthropicToolDef> =
                tools.iter().map(to_anthropic_tool).collect();
            let system = extract_system_from_messages(messages);

            let payload = MessagesRequest {
                model: self.model.clone(),
                max_tokens,
                messages: to_anthropic_messages(messages),
                system,
                thinking,
                stream: true,
                tools: if anthropic_tools.is_empty() {
                    None
                } else {
                    Some(anthropic_tools)
                },
            };

            log_request("anthropic", &url, &self.model);
            let start = Instant::now();

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(self.config.timeout_ms))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());

            let request = client.post(&url);
            let request = apply_anthropic_auth(request, &self.api_key, self.use_bearer_auth);
            let mut request = request
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json");
            if self.use_bearer_auth {
                request = request.header("anthropic-beta", ANTHROPIC_OAUTH_BETA);
            }
            let response = match request.json(&payload).send().await {
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
            let mut accumulated_text = String::new();

            // Track in-flight tool calls by index.
            struct ToolCallAccum {
                id: String,
                name: String,
                arguments_json: String,
            }
            let mut tool_accumulators: Vec<(usize, ToolCallAccum)> = Vec::new();
            let mut final_stop_reason: Option<String> = None;
            let mut stream_input_tokens: u64 = 0;
            let mut stream_output_tokens: u64 = 0;

            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = chunk_result.context("error reading SSE chunk")?;
                buffer.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(event_end) = buffer.find("\n\n") {
                    let event_block = buffer[..event_end].to_string();
                    buffer = buffer[event_end + 2..].to_string();

                    match parse_sse_event(&event_block) {
                        SseEvent::TextDelta(text) => {
                            accumulated_text.push_str(&text);
                            let _ = sender.send(StreamChunk {
                                delta: text,
                                done: false,
                                tool_call_delta: None,
                            });
                        }
                        SseEvent::ToolUseStart { index, id, name } => {
                            let _ = sender.send(StreamChunk {
                                delta: String::new(),
                                done: false,
                                tool_call_delta: Some(ToolCallDelta {
                                    index,
                                    id: Some(id.clone()),
                                    name: Some(name.clone()),
                                    arguments_delta: String::new(),
                                }),
                            });
                            tool_accumulators.push((
                                index,
                                ToolCallAccum {
                                    id,
                                    name,
                                    arguments_json: String::new(),
                                },
                            ));
                        }
                        SseEvent::ToolUseInputDelta { index, delta } => {
                            if let Some((_, accum)) =
                                tool_accumulators.iter_mut().find(|(i, _)| *i == index)
                            {
                                accum.arguments_json.push_str(&delta);
                            }
                            let _ = sender.send(StreamChunk {
                                delta: String::new(),
                                done: false,
                                tool_call_delta: Some(ToolCallDelta {
                                    index,
                                    id: None,
                                    name: None,
                                    arguments_delta: delta,
                                }),
                            });
                        }
                        SseEvent::ContentBlockStop { .. } => {}
                        SseEvent::MessageStart { input_tokens } => {
                            stream_input_tokens = input_tokens;
                        }
                        SseEvent::MessageDelta {
                            stop_reason,
                            output_tokens,
                        } => {
                            final_stop_reason = stop_reason;
                            stream_output_tokens = output_tokens;
                        }
                        SseEvent::Other => {}
                    }
                }
            }

            self.circuit_breaker.record_success();
            log_response("anthropic", 200, accumulated_text.len(), start.elapsed());

            // Build final tool calls from accumulators.
            let tool_calls: Vec<ToolUseRequest> = tool_accumulators
                .into_iter()
                .map(|(_, accum)| {
                    let input = serde_json::from_str(&accum.arguments_json)
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                    ToolUseRequest {
                        id: accum.id,
                        name: accum.name,
                        input,
                    }
                })
                .collect();

            let stop_reason = map_stop_reason(final_stop_reason.as_deref());

            let _ = sender.send(StreamChunk {
                delta: String::new(),
                done: true,
                tool_call_delta: None,
            });

            Ok(ChatResult {
                output_text: accumulated_text,
                tool_calls,
                stop_reason,
                input_tokens: stream_input_tokens,
                output_tokens: stream_output_tokens,
            })
        }
        .instrument(span)
        .await
    }
}

/// Parsed SSE event from Anthropic streaming API.
#[derive(Debug, PartialEq)]
enum SseEvent {
    /// Text delta from a `content_block_delta` with `text_delta` type.
    TextDelta(String),
    /// Start of a tool_use content block.
    ToolUseStart {
        index: usize,
        id: String,
        name: String,
    },
    /// Incremental JSON fragment for a tool_use input.
    ToolUseInputDelta { index: usize, delta: String },
    /// End of a content block.
    ContentBlockStop { index: usize },
    /// Message start event carrying input token count.
    MessageStart { input_tokens: u64 },
    /// Final message delta with stop_reason and output token count.
    MessageDelta {
        stop_reason: Option<String>,
        output_tokens: u64,
    },
    /// Event we don't need to act on.
    Other,
}

/// Parse an Anthropic SSE event block into a structured `SseEvent`.
fn parse_sse_event(event_block: &str) -> SseEvent {
    let mut event_type = None;
    let mut data_line = None;
    for line in event_block.lines() {
        if let Some(rest) = line.strip_prefix("event: ") {
            event_type = Some(rest.trim());
        } else if let Some(rest) = line.strip_prefix("data: ") {
            data_line = Some(rest);
        }
    }

    let data = match data_line {
        Some(d) => d,
        None => return SseEvent::Other,
    };

    let value: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return SseEvent::Other,
    };

    match event_type {
        Some("content_block_start") => {
            let index = value.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            if let Some(block) = value.get("content_block") {
                if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    return SseEvent::ToolUseStart { index, id, name };
                }
            }
            SseEvent::Other
        }
        Some("content_block_delta") => {
            let index = value.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            if let Some(delta) = value.get("delta") {
                match delta.get("type").and_then(|v| v.as_str()) {
                    Some("text_delta") => {
                        if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                            return SseEvent::TextDelta(text.to_string());
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(partial) = delta.get("partial_json").and_then(|v| v.as_str()) {
                            return SseEvent::ToolUseInputDelta {
                                index,
                                delta: partial.to_string(),
                            };
                        }
                    }
                    _ => {}
                }
            }
            SseEvent::Other
        }
        Some("content_block_stop") => {
            let index = value.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            SseEvent::ContentBlockStop { index }
        }
        Some("message_start") => {
            let input_tokens = value
                .get("message")
                .and_then(|m| m.get("usage"))
                .and_then(|u| u.get("input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            SseEvent::MessageStart { input_tokens }
        }
        Some("message_delta") => {
            let stop_reason = value
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let output_tokens = value
                .get("usage")
                .and_then(|u| u.get("output_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            SseEvent::MessageDelta {
                stop_reason,
                output_tokens,
            }
        }
        _ => {
            // Fallback: try the old content_block_delta parse for events without event: prefix
            if let Some(event_type_field) = value.get("type").and_then(|v| v.as_str()) {
                if event_type_field == "content_block_delta" {
                    if let Some(delta) = value.get("delta") {
                        if delta.get("type").and_then(|v| v.as_str()) == Some("text_delta") {
                            if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                return SseEvent::TextDelta(text.to_string());
                            }
                        }
                    }
                }
            }
            SseEvent::Other
        }
    }
}

/// Parse a text delta from an Anthropic SSE event block (legacy wrapper).
fn parse_sse_text_delta(event_block: &str) -> Option<String> {
    match parse_sse_event(event_block) {
        SseEvent::TextDelta(text) => Some(text),
        _ => None,
    }
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
            _use_bearer_auth: bool,
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
            _use_bearer_auth: bool,
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
            use_bearer_auth: false,
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
            tools: None,
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
            tools: None,
        };
        let json = serde_json::to_value(&req).expect("should serialize");
        assert!(json.get("stream").is_none());
    }

    // --- Tool use conversion & parsing ---

    #[test]
    fn to_anthropic_tool_produces_correct_shape() {
        let def = ToolDefinition {
            name: "read_file".to_string(),
            description: "Read file contents".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        };
        let tool = to_anthropic_tool(&def);
        let json = serde_json::to_value(&tool).expect("should serialize");
        assert_eq!(json["name"], "read_file");
        assert_eq!(json["description"], "Read file contents");
        assert_eq!(json["input_schema"]["type"], "object");
    }

    #[test]
    fn to_anthropic_messages_maps_all_variants() {
        use agentzero_core::{ConversationMessage, ToolResultMessage, ToolUseRequest};

        let messages = vec![
            ConversationMessage::user("Use the tool.".to_string()),
            ConversationMessage::Assistant {
                content: Some("Sure, calling read_file.".to_string()),
                tool_calls: vec![ToolUseRequest {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "/tmp/test"}),
                }],
            },
            ConversationMessage::ToolResult(ToolResultMessage {
                tool_use_id: "call_1".to_string(),
                content: "file contents".to_string(),
                is_error: false,
            }),
        ];

        let result = to_anthropic_messages(&messages);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].role, "user");
        assert_eq!(result[1].role, "assistant");
        assert_eq!(result[2].role, "user"); // tool results sent as user messages in Anthropic
    }

    #[test]
    fn to_anthropic_messages_filters_out_system() {
        use agentzero_core::ConversationMessage;

        let messages = vec![
            ConversationMessage::System {
                content: "You are a helpful assistant.".to_string(),
            },
            ConversationMessage::user("Hello".to_string()),
        ];

        let result = to_anthropic_messages(&messages);
        // System message should be filtered out — Anthropic uses a separate `system` field.
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "user");
    }

    #[test]
    fn extract_system_from_messages_returns_system_content() {
        use agentzero_core::ConversationMessage;

        let messages = vec![
            ConversationMessage::System {
                content: "Be concise.".to_string(),
            },
            ConversationMessage::user("Hi".to_string()),
        ];
        let system = extract_system_from_messages(&messages);
        assert_eq!(system, Some("Be concise.".to_string()));
    }

    #[test]
    fn extract_system_from_messages_returns_none_when_absent() {
        use agentzero_core::ConversationMessage;

        let messages = vec![ConversationMessage::user("Hi".to_string())];
        let system = extract_system_from_messages(&messages);
        assert!(system.is_none());
    }

    #[test]
    fn parse_tool_response_text_only() {
        let body = r#"{
            "content": [{"type": "text", "text": "Hello world"}],
            "stop_reason": "end_turn"
        }"#;
        let result = parse_tool_response(body).expect("should parse");
        assert_eq!(result.output_text, "Hello world");
        assert!(result.tool_calls.is_empty());
        assert_eq!(result.stop_reason, Some(StopReason::EndTurn));
    }

    #[test]
    fn parse_tool_response_with_tool_use() {
        let body = r#"{
            "content": [
                {"type": "text", "text": "Let me search."},
                {"type": "tool_use", "id": "tu_1", "name": "web_search", "input": {"query": "rust"}}
            ],
            "stop_reason": "tool_use"
        }"#;
        let result = parse_tool_response(body).expect("should parse");
        assert_eq!(result.output_text, "Let me search.");
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].id, "tu_1");
        assert_eq!(result.tool_calls[0].name, "web_search");
        assert_eq!(result.tool_calls[0].input["query"], "rust");
        assert_eq!(result.stop_reason, Some(StopReason::ToolUse));
    }

    #[test]
    fn parse_tool_response_only_tool_use_no_text() {
        let body = r#"{
            "content": [
                {"type": "tool_use", "id": "tu_1", "name": "shell", "input": {"command": "ls"}}
            ],
            "stop_reason": "tool_use"
        }"#;
        let result = parse_tool_response(body).expect("should parse");
        assert!(result.output_text.is_empty());
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.stop_reason, Some(StopReason::ToolUse));
    }

    #[test]
    fn parse_tool_response_max_tokens() {
        let body = r#"{
            "content": [{"type": "text", "text": "partial..."}],
            "stop_reason": "max_tokens"
        }"#;
        let result = parse_tool_response(body).expect("should parse");
        assert_eq!(result.stop_reason, Some(StopReason::MaxTokens));
    }

    #[test]
    fn map_stop_reason_handles_all_variants() {
        assert_eq!(map_stop_reason(Some("end_turn")), Some(StopReason::EndTurn));
        assert_eq!(map_stop_reason(Some("tool_use")), Some(StopReason::ToolUse));
        assert_eq!(
            map_stop_reason(Some("max_tokens")),
            Some(StopReason::MaxTokens)
        );
        assert_eq!(
            map_stop_reason(Some("stop_sequence")),
            Some(StopReason::StopSequence)
        );
        assert_eq!(
            map_stop_reason(Some("custom")),
            Some(StopReason::Other("custom".to_string()))
        );
        assert_eq!(map_stop_reason(None), None);
    }

    #[test]
    fn request_includes_tools_when_present() {
        let tools = vec![AnthropicToolDef {
            name: "test".to_string(),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        }];
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 1024,
            messages: vec![],
            system: None,
            thinking: None,
            stream: false,
            tools: Some(tools),
        };
        let json = serde_json::to_value(&req).expect("should serialize");
        let tools_arr = json["tools"].as_array().expect("tools should be array");
        assert_eq!(tools_arr.len(), 1);
        assert_eq!(tools_arr[0]["name"], "test");
    }

    #[test]
    fn request_omits_tools_when_none() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 1024,
            messages: vec![],
            system: None,
            thinking: None,
            stream: false,
            tools: None,
        };
        let json = serde_json::to_value(&req).expect("should serialize");
        assert!(json.get("tools").is_none());
    }

    #[tokio::test]
    async fn complete_with_tools_returns_tool_calls() {
        let body = r#"{
            "content": [
                {"type": "text", "text": "Searching..."},
                {"type": "tool_use", "id": "tu_1", "name": "web_search", "input": {"q": "rust"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 100, "output_tokens": 50}
        }"#;
        let transport = Arc::new(ScriptedTransport::new(vec![ok_response(body)]));
        let provider = AnthropicProvider::with_transport(
            "https://example.invalid".to_string(),
            "sk-test".to_string(),
            "claude-sonnet-4-20250514".to_string(),
            transport.clone(),
        );

        let messages = vec![ConversationMessage::user("Search for rust".to_string())];
        let tools = vec![ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        }];

        let result = provider
            .complete_with_tools(&messages, &tools, &ReasoningConfig::default())
            .await
            .expect("should succeed");

        assert_eq!(result.output_text, "Searching...");
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].name, "web_search");
        assert_eq!(result.stop_reason, Some(StopReason::ToolUse));
    }

    #[tokio::test]
    async fn complete_with_tools_no_tool_calls() {
        let body = r#"{
            "content": [{"type": "text", "text": "No tools needed."}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        }"#;
        let transport = Arc::new(ScriptedTransport::new(vec![ok_response(body)]));
        let provider = AnthropicProvider::with_transport(
            "https://example.invalid".to_string(),
            "sk-test".to_string(),
            "claude-sonnet-4-20250514".to_string(),
            transport.clone(),
        );

        let messages = vec![ConversationMessage::user("Just chat".to_string())];

        let result = provider
            .complete_with_tools(&messages, &[], &ReasoningConfig::default())
            .await
            .expect("should succeed");

        assert_eq!(result.output_text, "No tools needed.");
        assert!(result.tool_calls.is_empty());
        assert_eq!(result.stop_reason, Some(StopReason::EndTurn));
    }

    // -----------------------------------------------------------------------
    // SSE event parser tests (streaming tool use)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_sse_event_text_delta() {
        let block = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}";
        assert_eq!(
            parse_sse_event(block),
            SseEvent::TextDelta("Hello".to_string())
        );
    }

    #[test]
    fn parse_sse_event_tool_use_start() {
        let block = "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_01\",\"name\":\"read_file\"}}";
        assert_eq!(
            parse_sse_event(block),
            SseEvent::ToolUseStart {
                index: 1,
                id: "toolu_01".to_string(),
                name: "read_file".to_string(),
            }
        );
    }

    #[test]
    fn parse_sse_event_input_json_delta() {
        let block = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"\"}}";
        assert_eq!(
            parse_sse_event(block),
            SseEvent::ToolUseInputDelta {
                index: 1,
                delta: "{\"path\":\"".to_string(),
            }
        );
    }

    #[test]
    fn parse_sse_event_content_block_stop() {
        let block =
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}";
        assert_eq!(
            parse_sse_event(block),
            SseEvent::ContentBlockStop { index: 1 }
        );
    }

    #[test]
    fn parse_sse_event_message_delta_stop_reason() {
        let block = "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":50}}";
        assert_eq!(
            parse_sse_event(block),
            SseEvent::MessageDelta {
                stop_reason: Some("tool_use".to_string()),
                output_tokens: 50,
            }
        );
    }

    #[test]
    fn parse_sse_event_message_delta_end_turn() {
        let block = "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}";
        assert_eq!(
            parse_sse_event(block),
            SseEvent::MessageDelta {
                stop_reason: Some("end_turn".to_string()),
                output_tokens: 0,
            }
        );
    }

    #[test]
    fn parse_sse_event_unknown_returns_other() {
        let block = "event: ping\ndata: {}";
        assert_eq!(parse_sse_event(block), SseEvent::Other);
    }

    #[test]
    fn parse_sse_event_no_data_returns_other() {
        let block = "event: message_start";
        assert_eq!(parse_sse_event(block), SseEvent::Other);
    }

    #[test]
    fn parse_sse_text_delta_backward_compat() {
        // The legacy wrapper should still work for text deltas without event: prefix
        let block =
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"world\"}}";
        assert_eq!(parse_sse_text_delta(block), Some("world".to_string()));
    }

    #[test]
    fn parse_sse_text_delta_returns_none_for_tool_use() {
        let block = "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"shell\"}}";
        assert_eq!(parse_sse_text_delta(block), None);
    }

    #[test]
    fn is_oauth_token_detects_oat_prefix() {
        assert!(AnthropicProvider::is_oauth_token("sk-ant-oat-abc123"));
    }

    #[test]
    fn is_oauth_token_rejects_api_key() {
        assert!(!AnthropicProvider::is_oauth_token("sk-ant-api03-abc123"));
    }

    #[test]
    fn is_oauth_token_detects_non_sk_ant_as_oauth() {
        assert!(AnthropicProvider::is_oauth_token("some-other-token"));
    }

    #[test]
    fn oauth_token_enables_bearer_auth() {
        let provider = AnthropicProvider::new(
            "https://api.anthropic.com".to_string(),
            "sk-ant-oat-abc123".to_string(),
            "claude-sonnet-4-6".to_string(),
        );
        assert!(provider.use_bearer_auth);
    }

    #[test]
    fn api_key_disables_bearer_auth() {
        let provider = AnthropicProvider::new(
            "https://api.anthropic.com".to_string(),
            "sk-ant-api03-abc123".to_string(),
            "claude-sonnet-4-6".to_string(),
        );
        assert!(!provider.use_bearer_auth);
    }
}
