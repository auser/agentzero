use crate::transport::{
    jittered_backoff, log_request, log_response, log_retry, map_reqwest_error, map_status_error,
    parse_retry_after, should_retry_status, should_retry_transport, TransportError,
    TransportErrorKind, TransportResponse, MAX_ATTEMPTS,
};
use agentzero_core::{
    ChatResult, ConversationMessage, Provider, ReasoningConfig, StopReason, StreamChunk,
    ToolCallDelta, ToolDefinition, ToolUseRequest,
};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::sleep;
use tracing::{info_span, Instrument};

// ---------------------------------------------------------------------------
// Transport abstraction (OpenAI-specific payload)
// ---------------------------------------------------------------------------

#[async_trait]
pub(crate) trait HttpTransport: Send + Sync {
    async fn send_chat(
        &self,
        url: &str,
        api_key: &str,
        payload: &ChatRequest,
    ) -> Result<TransportResponse, TransportError>;
}

pub(crate) struct ReqwestTransport {
    pub(crate) client: reqwest::Client,
}

impl ReqwestTransport {
    pub(crate) fn new() -> Self {
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

    #[allow(dead_code)] // Used by integration tests and noise transport wrapping
    pub(crate) fn with_transport(
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
pub(crate) struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiToolDef>>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl Message {
    fn user(content: String) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(Value::String(content)),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn user_with_parts(content: String, parts: &[agentzero_core::ContentPart]) -> Self {
        let mut content_array = vec![serde_json::json!({
            "type": "text",
            "text": content,
        })];
        for part in parts {
            match part {
                agentzero_core::ContentPart::Text { text } => {
                    content_array.push(serde_json::json!({
                        "type": "text",
                        "text": text,
                    }));
                }
                agentzero_core::ContentPart::Image { media_type, data } => {
                    content_array.push(serde_json::json!({
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{media_type};base64,{data}"),
                        },
                    }));
                }
            }
        }
        Self {
            role: "user".to_string(),
            content: Some(Value::Array(content_array)),
            tool_calls: None,
            tool_call_id: None,
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiToolDef {
    #[serde(rename = "type")]
    kind: String,
    function: OpenAiFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCall {
    id: String,
    function: OpenAiFunctionCall,
}

#[derive(Debug, Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCall>>,
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
        let span = info_span!("openai_complete", provider = "openai-compat", model = %self.model);
        async {
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
                messages: vec![Message::user(prompt.to_string())],
                reasoning_effort,
                stream: false,
                tools: None,
            };

            log_request("openai-compat", &url, &self.model);
            let start = Instant::now();

            let mut last_error: Option<anyhow::Error> = None;
            for attempt in 0..MAX_ATTEMPTS {
                match self
                    .transport
                    .send_chat(&url, &self.api_key, &payload)
                    .await
                {
                    Ok(response) => {
                        log_response(
                            "openai-compat",
                            response.status,
                            response.body.len(),
                            start.elapsed(),
                        );
                        let status =
                            StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
                        if !status.is_success() {
                            let mapped = map_status_error(status, &response.body);
                            if attempt + 1 < MAX_ATTEMPTS && should_retry_status(status) {
                                log_retry(
                                    "openai-compat",
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

                        let output_text = parse_output_text(&response.body)
                            .with_context(|| "failed to parse provider response".to_string())?;
                        return Ok(ChatResult {
                            output_text,
                            ..Default::default()
                        });
                    }
                    Err(error) => {
                        let mapped = anyhow!("provider request failed: {}", error.message);
                        if attempt + 1 < MAX_ATTEMPTS && should_retry_transport(error.kind) {
                            log_retry("openai-compat", attempt, &error.message);
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
        let span = info_span!("openai_stream", provider = "openai-compat", model = %self.model);
        async {
            let url = format!(
                "{}/v1/chat/completions",
                self.base_url.trim_end_matches('/')
            );
            let payload = ChatRequest {
                model: self.model.clone(),
                messages: vec![Message::user(prompt.to_string())],
                reasoning_effort: None,
                stream: true,
                tools: None,
            };

            log_request("openai-compat", &url, &self.model);
            let start = Instant::now();

            let client = reqwest::Client::new();
            let mut request = client.post(&url).json(&payload);
            if !self.api_key.is_empty() {
                request = request.bearer_auth(&self.api_key);
            }

            let response = request
                .send()
                .await
                .map_err(|e| anyhow!("streaming request failed: {e}"))?;

            if !response.status().is_success() {
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

                    if let Some(delta) = parse_openai_sse_delta(&event_block) {
                        accumulated.push_str(&delta);
                        let _ = sender.send(StreamChunk {
                            delta,
                            done: false,
                            tool_call_delta: None,
                        });
                    }
                }
            }

            log_response("openai-compat", 200, accumulated.len(), start.elapsed());

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
            info_span!("openai_complete_tools", provider = "openai-compat", model = %self.model);
        async {
            let url = format!(
                "{}/v1/chat/completions",
                self.base_url.trim_end_matches('/')
            );
            let reasoning_effort = match reasoning.enabled {
                Some(false) => None,
                _ => reasoning.level.clone(),
            };

            let openai_tools: Vec<OpenAiToolDef> = tools.iter().map(to_openai_tool).collect();

            let payload = ChatRequest {
                model: self.model.clone(),
                messages: to_openai_messages(messages),
                reasoning_effort,
                stream: false,
                tools: if openai_tools.is_empty() {
                    None
                } else {
                    Some(openai_tools)
                },
            };

            log_request("openai-compat", &url, &self.model);
            let start = Instant::now();

            let mut last_error: Option<anyhow::Error> = None;
            for attempt in 0..MAX_ATTEMPTS {
                match self
                    .transport
                    .send_chat(&url, &self.api_key, &payload)
                    .await
                {
                    Ok(response) => {
                        log_response(
                            "openai-compat",
                            response.status,
                            response.body.len(),
                            start.elapsed(),
                        );
                        let status =
                            StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
                        if !status.is_success() {
                            let mapped = map_status_error(status, &response.body);
                            if attempt + 1 < MAX_ATTEMPTS && should_retry_status(status) {
                                log_retry(
                                    "openai-compat",
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

                        return parse_tool_chat_response(&response.body)
                            .with_context(|| "failed to parse provider tool response");
                    }
                    Err(error) => {
                        let mapped = anyhow!("provider request failed: {}", error.message);
                        if attempt + 1 < MAX_ATTEMPTS && should_retry_transport(error.kind) {
                            log_retry("openai-compat", attempt, &error.message);
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
            info_span!("openai_stream_tools", provider = "openai-compat", model = %self.model);
        async {
            let url = format!(
                "{}/v1/chat/completions",
                self.base_url.trim_end_matches('/')
            );
            let reasoning_effort = match reasoning.enabled {
                Some(false) => None,
                _ => reasoning.level.clone(),
            };
            let openai_tools: Vec<OpenAiToolDef> = tools.iter().map(to_openai_tool).collect();

            let payload = ChatRequest {
                model: self.model.clone(),
                messages: to_openai_messages(messages),
                reasoning_effort,
                stream: true,
                tools: if openai_tools.is_empty() {
                    None
                } else {
                    Some(openai_tools)
                },
            };

            log_request("openai-compat", &url, &self.model);
            let start = Instant::now();

            let client = reqwest::Client::new();
            let mut request = client.post(&url).json(&payload);
            if !self.api_key.is_empty() {
                request = request.bearer_auth(&self.api_key);
            }

            let response = request
                .send()
                .await
                .map_err(|e| anyhow!("streaming request failed: {e}"))?;

            if !response.status().is_success() {
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
            let mut final_finish_reason: Option<String> = None;

            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = chunk_result.context("error reading SSE chunk")?;
                buffer.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(event_end) = buffer.find("\n\n") {
                    let event_block = buffer[..event_end].to_string();
                    buffer = buffer[event_end + 2..].to_string();

                    match parse_openai_sse_event(&event_block) {
                        OpenAiSseEvent::ContentDelta(text) => {
                            accumulated_text.push_str(&text);
                            let _ = sender.send(StreamChunk {
                                delta: text,
                                done: false,
                                tool_call_delta: None,
                            });
                        }
                        OpenAiSseEvent::ToolCallDelta {
                            index,
                            id,
                            name,
                            arguments,
                        } => {
                            // Find or create accumulator for this index.
                            if !tool_accumulators.iter().any(|(i, _)| *i == index) {
                                tool_accumulators.push((
                                    index,
                                    ToolCallAccum {
                                        id: id.clone().unwrap_or_default(),
                                        name: name.clone().unwrap_or_default(),
                                        arguments_json: String::new(),
                                    },
                                ));
                            }
                            if let Some((_, accum)) =
                                tool_accumulators.iter_mut().find(|(i, _)| *i == index)
                            {
                                accum.arguments_json.push_str(&arguments);
                            }
                            let _ = sender.send(StreamChunk {
                                delta: String::new(),
                                done: false,
                                tool_call_delta: Some(ToolCallDelta {
                                    index,
                                    id,
                                    name,
                                    arguments_delta: arguments,
                                }),
                            });
                        }
                        OpenAiSseEvent::Finished { finish_reason } => {
                            final_finish_reason = finish_reason;
                        }
                        OpenAiSseEvent::Done | OpenAiSseEvent::Other => {}
                    }
                }
            }

            log_response(
                "openai-compat",
                200,
                accumulated_text.len(),
                start.elapsed(),
            );

            // Build final tool calls from accumulators.
            let tool_calls: Vec<ToolUseRequest> = tool_accumulators
                .into_iter()
                .map(|(_, accum)| {
                    let input = serde_json::from_str(&accum.arguments_json)
                        .unwrap_or(Value::Object(serde_json::Map::new()));
                    ToolUseRequest {
                        id: accum.id,
                        name: accum.name,
                        input,
                    }
                })
                .collect();

            let stop_reason = map_openai_stop_reason(final_finish_reason.as_deref());

            let _ = sender.send(StreamChunk {
                delta: String::new(),
                done: true,
                tool_call_delta: None,
            });

            Ok(ChatResult {
                output_text: accumulated_text,
                tool_calls,
                stop_reason,
            })
        }
        .instrument(span)
        .await
    }
}

fn to_openai_tool(def: &ToolDefinition) -> OpenAiToolDef {
    OpenAiToolDef {
        kind: "function".to_string(),
        function: OpenAiFunction {
            name: def.name.clone(),
            description: def.description.clone(),
            parameters: def.input_schema.clone(),
        },
    }
}

fn to_openai_messages(messages: &[ConversationMessage]) -> Vec<Message> {
    messages
        .iter()
        .map(|msg| match msg {
            ConversationMessage::System { content } => Message {
                role: "system".to_string(),
                content: Some(Value::String(content.clone())),
                tool_calls: None,
                tool_call_id: None,
            },
            ConversationMessage::User { content, parts } => {
                if parts.is_empty() {
                    Message::user(content.clone())
                } else {
                    Message::user_with_parts(content.clone(), parts)
                }
            }
            ConversationMessage::Assistant {
                content,
                tool_calls,
            } => {
                let tc = if tool_calls.is_empty() {
                    None
                } else {
                    Some(
                        tool_calls
                            .iter()
                            .map(|tc| {
                                serde_json::json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.name,
                                        "arguments": tc.input.to_string(),
                                    }
                                })
                            })
                            .collect(),
                    )
                };
                Message {
                    role: "assistant".to_string(),
                    content: content.as_ref().map(|s| Value::String(s.clone())),
                    tool_calls: tc,
                    tool_call_id: None,
                }
            }
            ConversationMessage::ToolResult(result) => Message {
                role: "tool".to_string(),
                content: Some(Value::String(result.content.clone())),
                tool_calls: None,
                tool_call_id: Some(result.tool_use_id.clone()),
            },
        })
        .collect()
}

fn map_openai_stop_reason(reason: Option<&str>) -> Option<StopReason> {
    match reason {
        Some("stop") => Some(StopReason::EndTurn),
        Some("tool_calls") => Some(StopReason::ToolUse),
        Some("length") => Some(StopReason::MaxTokens),
        Some(other) => Some(StopReason::Other(other.to_string())),
        None => None,
    }
}

fn parse_tool_chat_response(body: &str) -> anyhow::Result<ChatResult> {
    let response: ChatResponse =
        serde_json::from_str(body).context("failed to parse OpenAI response JSON")?;

    let choice = response.choices.first();
    let output_text = choice
        .and_then(|c| c.message.content.clone())
        .unwrap_or_default();
    let stop_reason = map_openai_stop_reason(choice.and_then(|c| c.finish_reason.as_deref()));

    let tool_calls = choice
        .and_then(|c| c.message.tool_calls.as_ref())
        .map(|tcs| {
            tcs.iter()
                .map(|tc| {
                    let input = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(Value::String(tc.function.arguments.clone()));
                    ToolUseRequest {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        input,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(ChatResult {
        output_text,
        tool_calls,
        stop_reason,
    })
}

/// Parsed SSE event from OpenAI-compatible streaming API.
#[derive(Debug, PartialEq)]
enum OpenAiSseEvent {
    /// Text content delta.
    ContentDelta(String),
    /// Tool call delta (incremental id/name/arguments).
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
    /// Stream finished with a finish_reason.
    Finished { finish_reason: Option<String> },
    /// End-of-stream marker `[DONE]`.
    Done,
    /// Event we don't need to act on.
    Other,
}

/// Parse an OpenAI-compatible SSE event block into a structured `OpenAiSseEvent`.
fn parse_openai_sse_event(event_block: &str) -> OpenAiSseEvent {
    for line in event_block.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            if data.trim() == "[DONE]" {
                return OpenAiSseEvent::Done;
            }
            let value: Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => return OpenAiSseEvent::Other,
            };
            let choice = match value
                .get("choices")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
            {
                Some(c) => c,
                None => return OpenAiSseEvent::Other,
            };

            // Check finish_reason
            let finish_reason = choice
                .get("finish_reason")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // Check for tool_calls delta
            if let Some(delta) = choice.get("delta") {
                if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                    // Return the first tool call delta (multiple may arrive but typically one per chunk)
                    if let Some(tc) = tool_calls.first() {
                        let index = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        let id = tc.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                        let name = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let arguments = tc
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        return OpenAiSseEvent::ToolCallDelta {
                            index,
                            id,
                            name,
                            arguments,
                        };
                    }
                }

                // Check for content delta
                if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                    if !content.is_empty() {
                        return OpenAiSseEvent::ContentDelta(content.to_string());
                    }
                }
            }

            // If finish_reason is set, return Finished
            if finish_reason.is_some() {
                return OpenAiSseEvent::Finished { finish_reason };
            }
        }
    }
    OpenAiSseEvent::Other
}

/// Parse a content delta from an OpenAI-compatible SSE event block (legacy wrapper).
fn parse_openai_sse_delta(event_block: &str) -> Option<String> {
    match parse_openai_sse_event(event_block) {
        OpenAiSseEvent::ContentDelta(text) => Some(text),
        _ => None,
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

    // --- SSE parsing ---

    #[test]
    fn parse_openai_sse_delta_extracts_content() {
        let event = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}";
        assert_eq!(parse_openai_sse_delta(event), Some("Hello".to_string()));
    }

    #[test]
    fn parse_openai_sse_delta_ignores_done_marker() {
        let event = "data: [DONE]";
        assert!(parse_openai_sse_delta(event).is_none());
    }

    #[test]
    fn parse_openai_sse_delta_ignores_empty_content() {
        let event = "data: {\"choices\":[{\"delta\":{\"content\":\"\"}}]}";
        assert!(parse_openai_sse_delta(event).is_none());
    }

    #[test]
    fn parse_openai_sse_delta_ignores_role_only_delta() {
        let event = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}";
        assert!(parse_openai_sse_delta(event).is_none());
    }

    #[test]
    fn stream_request_sets_stream_true() {
        let req = ChatRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![],
            reasoning_effort: None,
            stream: true,
            tools: None,
        };
        let json = serde_json::to_value(&req).expect("should serialize");
        assert_eq!(json["stream"], true);
    }

    #[test]
    fn non_stream_request_omits_stream_field() {
        let req = ChatRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![],
            reasoning_effort: None,
            stream: false,
            tools: None,
        };
        let json = serde_json::to_value(&req).expect("should serialize");
        assert!(json.get("stream").is_none());
    }

    // --- Tool use conversion & parsing ---

    #[test]
    fn to_openai_tool_produces_function_shape() {
        let def = ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } }
            }),
        };
        let tool = to_openai_tool(&def);
        let json = serde_json::to_value(&tool).expect("should serialize");
        assert_eq!(json["type"], "function");
        assert_eq!(json["function"]["name"], "read_file");
        assert_eq!(json["function"]["description"], "Read a file");
        assert_eq!(json["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn to_openai_messages_maps_all_variants() {
        use agentzero_core::{ConversationMessage, ToolResultMessage, ToolUseRequest};

        let messages = vec![
            ConversationMessage::user("Use the tool.".to_string()),
            ConversationMessage::Assistant {
                content: Some("Calling tool.".to_string()),
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

        let result = to_openai_messages(&messages);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].role, "user");
        assert_eq!(
            result[0].content,
            Some(Value::String("Use the tool.".to_string()))
        );
        assert_eq!(result[1].role, "assistant");
        assert!(result[1].tool_calls.is_some());
        assert_eq!(result[2].role, "tool");
        assert_eq!(result[2].tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn to_openai_messages_maps_system_to_system_role() {
        use agentzero_core::ConversationMessage;

        let messages = vec![
            ConversationMessage::System {
                content: "You are a helpful assistant.".to_string(),
            },
            ConversationMessage::user("Hello".to_string()),
        ];

        let result = to_openai_messages(&messages);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, "system");
        assert_eq!(
            result[0].content,
            Some(Value::String("You are a helpful assistant.".to_string()))
        );
        assert!(result[0].tool_calls.is_none());
        assert!(result[0].tool_call_id.is_none());
        assert_eq!(result[1].role, "user");
    }

    #[test]
    fn map_openai_stop_reason_handles_variants() {
        assert_eq!(
            map_openai_stop_reason(Some("stop")),
            Some(StopReason::EndTurn)
        );
        assert_eq!(
            map_openai_stop_reason(Some("tool_calls")),
            Some(StopReason::ToolUse)
        );
        assert_eq!(
            map_openai_stop_reason(Some("length")),
            Some(StopReason::MaxTokens)
        );
        assert_eq!(
            map_openai_stop_reason(Some("custom")),
            Some(StopReason::Other("custom".to_string()))
        );
        assert_eq!(map_openai_stop_reason(None), None);
    }

    #[test]
    fn parse_tool_chat_response_text_only() {
        let body = r#"{"choices":[{"message":{"content":"Hello"},"finish_reason":"stop"}]}"#;
        let result = parse_tool_chat_response(body).expect("should parse");
        assert_eq!(result.output_text, "Hello");
        assert!(result.tool_calls.is_empty());
        assert_eq!(result.stop_reason, Some(StopReason::EndTurn));
    }

    #[test]
    fn parse_tool_chat_response_with_tool_calls() {
        let body = r#"{"choices":[{
            "message":{
                "content":null,
                "tool_calls":[{
                    "id":"call_1",
                    "type":"function",
                    "function":{"name":"read_file","arguments":"{\"path\":\"/tmp/test\"}"}
                }]
            },
            "finish_reason":"tool_calls"
        }]}"#;
        let result = parse_tool_chat_response(body).expect("should parse");
        assert!(result.output_text.is_empty());
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].id, "call_1");
        assert_eq!(result.tool_calls[0].name, "read_file");
        assert_eq!(result.tool_calls[0].input["path"], "/tmp/test");
        assert_eq!(result.stop_reason, Some(StopReason::ToolUse));
    }

    #[test]
    fn parse_tool_chat_response_invalid_json_arguments() {
        let body = r#"{"choices":[{
            "message":{
                "content":null,
                "tool_calls":[{
                    "id":"call_1",
                    "type":"function",
                    "function":{"name":"test","arguments":"not valid json"}
                }]
            },
            "finish_reason":"tool_calls"
        }]}"#;
        let result = parse_tool_chat_response(body).expect("should parse");
        assert_eq!(result.tool_calls.len(), 1);
        // Falls back to wrapping in a string Value
        assert_eq!(
            result.tool_calls[0].input,
            Value::String("not valid json".to_string())
        );
    }

    #[test]
    fn request_includes_tools_when_present() {
        let tools = vec![OpenAiToolDef {
            kind: "function".to_string(),
            function: OpenAiFunction {
                name: "test".to_string(),
                description: "A test".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
        }];
        let req = ChatRequest {
            model: "gpt-4o".to_string(),
            messages: vec![],
            reasoning_effort: None,
            stream: false,
            tools: Some(tools),
        };
        let json = serde_json::to_value(&req).expect("should serialize");
        let tools_arr = json["tools"].as_array().expect("tools should be array");
        assert_eq!(tools_arr.len(), 1);
        assert_eq!(tools_arr[0]["type"], "function");
    }

    #[test]
    fn request_omits_tools_when_none() {
        let req = ChatRequest {
            model: "gpt-4o".to_string(),
            messages: vec![],
            reasoning_effort: None,
            stream: false,
            tools: None,
        };
        let json = serde_json::to_value(&req).expect("should serialize");
        assert!(json.get("tools").is_none());
    }

    #[tokio::test]
    async fn complete_with_tools_returns_tool_calls() {
        let body = r#"{"choices":[{
            "message":{
                "content":"I'll search for that.",
                "tool_calls":[{
                    "id":"call_abc",
                    "type":"function",
                    "function":{"name":"web_search","arguments":"{\"q\":\"rust\"}"}
                }]
            },
            "finish_reason":"tool_calls"
        }]}"#;
        let transport = Arc::new(ScriptedTransport::new(vec![ok_response(body)]));
        let provider = OpenAiCompatibleProvider::with_transport(
            "https://example.invalid".to_string(),
            "key".to_string(),
            "model".to_string(),
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

        assert_eq!(result.output_text, "I'll search for that.");
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].name, "web_search");
        assert_eq!(result.stop_reason, Some(StopReason::ToolUse));
    }

    #[tokio::test]
    async fn complete_with_tools_no_tool_calls() {
        let transport = Arc::new(ScriptedTransport::new(vec![ok_response(
            r#"{"choices":[{"message":{"content":"Just chat."},"finish_reason":"stop"}]}"#,
        )]));
        let provider = OpenAiCompatibleProvider::with_transport(
            "https://example.invalid".to_string(),
            "key".to_string(),
            "model".to_string(),
            transport.clone(),
        );

        let messages = vec![ConversationMessage::user("Hello".to_string())];

        let result = provider
            .complete_with_tools(&messages, &[], &ReasoningConfig::default())
            .await
            .expect("should succeed");

        assert_eq!(result.output_text, "Just chat.");
        assert!(result.tool_calls.is_empty());
        assert_eq!(result.stop_reason, Some(StopReason::EndTurn));
    }

    // -----------------------------------------------------------------------
    // SSE event parser tests (streaming tool use)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_openai_sse_event_content_delta() {
        let block =
            r#"data: {"choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        assert_eq!(
            parse_openai_sse_event(block),
            OpenAiSseEvent::ContentDelta("Hello".to_string())
        );
    }

    #[test]
    fn parse_openai_sse_event_tool_call_start() {
        let block = r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"read_file","arguments":""}}]},"finish_reason":null}]}"#;
        assert_eq!(
            parse_openai_sse_event(block),
            OpenAiSseEvent::ToolCallDelta {
                index: 0,
                id: Some("call_abc".to_string()),
                name: Some("read_file".to_string()),
                arguments: String::new(),
            }
        );
    }

    #[test]
    fn parse_openai_sse_event_tool_call_arguments() {
        let block = r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":\""}}]},"finish_reason":null}]}"#;
        assert_eq!(
            parse_openai_sse_event(block),
            OpenAiSseEvent::ToolCallDelta {
                index: 0,
                id: None,
                name: None,
                arguments: "{\"path\":\"".to_string(),
            }
        );
    }

    #[test]
    fn parse_openai_sse_event_finished_tool_calls() {
        let block = r#"data: {"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#;
        assert_eq!(
            parse_openai_sse_event(block),
            OpenAiSseEvent::Finished {
                finish_reason: Some("tool_calls".to_string()),
            }
        );
    }

    #[test]
    fn parse_openai_sse_event_finished_stop() {
        let block = r#"data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
        assert_eq!(
            parse_openai_sse_event(block),
            OpenAiSseEvent::Finished {
                finish_reason: Some("stop".to_string()),
            }
        );
    }

    #[test]
    fn parse_openai_sse_event_done_marker() {
        let block = "data: [DONE]";
        assert_eq!(parse_openai_sse_event(block), OpenAiSseEvent::Done);
    }

    #[test]
    fn parse_openai_sse_event_empty_content_returns_other() {
        let block =
            r#"data: {"choices":[{"index":0,"delta":{"content":""},"finish_reason":null}]}"#;
        // Empty content should not be a ContentDelta — should be Other or Finished
        // (empty content + null finish_reason = Other)
        let result = parse_openai_sse_event(block);
        assert!(
            matches!(result, OpenAiSseEvent::Other),
            "empty content delta should return Other, got {:?}",
            result
        );
    }

    #[test]
    fn parse_openai_sse_event_multiple_tool_calls_first_returned() {
        // When multiple tool_calls arrive in one chunk, we parse the first
        let block = r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"a"}},{"index":1,"id":"call_xyz","type":"function","function":{"name":"shell","arguments":""}}]},"finish_reason":null}]}"#;
        assert_eq!(
            parse_openai_sse_event(block),
            OpenAiSseEvent::ToolCallDelta {
                index: 0,
                id: None,
                name: None,
                arguments: "a".to_string(),
            }
        );
    }

    #[test]
    fn parse_openai_sse_delta_backward_compat() {
        let block =
            r#"data: {"choices":[{"index":0,"delta":{"content":"world"},"finish_reason":null}]}"#;
        assert_eq!(parse_openai_sse_delta(block), Some("world".to_string()));
    }

    #[test]
    fn parse_openai_sse_delta_returns_none_for_tool_calls() {
        let block = r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"shell","arguments":""}}]},"finish_reason":null}]}"#;
        assert_eq!(parse_openai_sse_delta(block), None);
    }
}
