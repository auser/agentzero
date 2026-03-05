use crate::auth::authorize_request;
use crate::models::{
    ChatCompletionsRequest, ChatCompletionsResponse, ChatRequest, ChatResponse, CompletionChoice,
    CompletionChoiceMessage, GatewayError, HealthResponse, ModelItem, ModelsResponse, PairRequest,
    PairResponse, PingRequest, PingResponse, WebhookResponse,
};
use crate::state::GatewayState;
use crate::util::{generate_session_token, now_epoch_secs};
use agentzero_channels::pipeline::check_perplexity;
use agentzero_infra::runtime::{
    build_runtime_execution, run_agent_once, run_agent_streaming, RunAgentRequest,
};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::HeaderMap,
    response::{Html, IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, Instant};

pub(crate) async fn dashboard(State(state): State<GatewayState>) -> Html<String> {
    Html(format!(
        "<html><body><h1>{}</h1><p>OTP configured: {}</p></body></html>",
        state.service_name,
        !state.otp_secret.is_empty()
    ))
}

pub(crate) async fn health(State(state): State<GatewayState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: (*state.service_name).clone(),
    })
}

pub(crate) async fn metrics(State(state): State<GatewayState>) -> impl IntoResponse {
    let payload = state.prometheus_handle.render();
    ([("content-type", "text/plain; version=0.0.4")], payload)
}

pub(crate) async fn pair(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    _body: Option<Json<PairRequest>>,
) -> Result<Json<PairResponse>, GatewayError> {
    let Some(expected_code) = state.pairing_code_valid() else {
        return Err(GatewayError::AuthRequired);
    };

    let Some(code_header) = headers.get("X-Pairing-Code") else {
        return Err(GatewayError::AuthRequired);
    };
    let Ok(code) = code_header.to_str() else {
        return Err(GatewayError::AuthRequired);
    };
    if code.trim() != expected_code {
        return Err(GatewayError::AuthFailed);
    }

    let token = generate_session_token();
    if state.add_paired_token(token.clone()).is_err() {
        return Err(GatewayError::AgentExecutionFailed {
            message: "failed to persist pairing token".to_string(),
        });
    }

    Ok(Json(PairResponse {
        paired: true,
        token,
    }))
}

pub(crate) async fn ping(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<PingRequest>,
) -> Result<Json<PingResponse>, GatewayError> {
    authorize_request(&state, &headers, false)?;

    Ok(Json(PingResponse {
        ok: true,
        echo: req.message,
    }))
}

pub(crate) async fn webhook(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(channel): Path<String>,
    Json(payload): Json<Value>,
) -> Result<Json<WebhookResponse>, GatewayError> {
    authorize_request(&state, &headers, false)?;

    let Some(delivery) = state.channels.dispatch(&channel, payload).await else {
        return Err(GatewayError::NotFound {
            resource: format!("channel/{channel}"),
        });
    };

    Ok(Json(WebhookResponse {
        accepted: delivery.accepted,
        channel: delivery.channel,
        detail: delivery.detail,
    }))
}

pub(crate) async fn legacy_webhook(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, GatewayError> {
    authorize_request(&state, &headers, false)?;

    if let Some(reason) = check_perplexity(&req.message, &state.perplexity_filter) {
        tracing::warn!(reason = %reason, "gateway legacy_webhook blocked by perplexity filter");
        return Err(GatewayError::BadRequest {
            message: format!("blocked by perplexity filter: {reason}"),
        });
    }

    Ok(Json(ChatResponse {
        message: format!("echo: {}", req.message),
        tokens_used_estimate: req.message.len() + req.context.len() * 8,
    }))
}

/// Build a `RunAgentRequest` from gateway state. Returns `AgentUnavailable`
/// if the gateway was started without a config/workspace path.
fn build_agent_request(
    state: &GatewayState,
    message: String,
    model_override: Option<String>,
) -> Result<RunAgentRequest, GatewayError> {
    let config_path = state
        .config_path
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?
        .as_ref()
        .clone();
    let workspace_root = state
        .workspace_root
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?
        .as_ref()
        .clone();
    Ok(RunAgentRequest {
        workspace_root,
        config_path,
        message,
        provider_override: None,
        model_override,
        profile_override: None,
        extra_tools: vec![],
    })
}

pub(crate) async fn api_chat(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, GatewayError> {
    authorize_request(&state, &headers, false)?;

    if let Some(reason) = check_perplexity(&req.message, &state.perplexity_filter) {
        tracing::warn!(reason = %reason, "gateway api_chat blocked by perplexity filter");
        return Err(GatewayError::BadRequest {
            message: format!("blocked by perplexity filter: {reason}"),
        });
    }

    let agent_req = build_agent_request(&state, req.message, None)?;
    let output = run_agent_once(agent_req).await.map_err(|e| {
        tracing::error!(error = %e, "api_chat agent execution failed");
        GatewayError::AgentExecutionFailed {
            message: e.to_string(),
        }
    })?;

    Ok(Json(ChatResponse {
        message: output.response_text,
        tokens_used_estimate: 0,
    }))
}

pub(crate) async fn v1_chat_completions(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<ChatCompletionsRequest>,
) -> Result<Response, GatewayError> {
    authorize_request(&state, &headers, false)?;

    let last_user = req
        .messages
        .iter()
        .rev()
        .find(|msg| msg.role == "user")
        .map(|msg| msg.content.clone())
        .unwrap_or_else(|| "hello".to_string());

    if let Some(reason) = check_perplexity(&last_user, &state.perplexity_filter) {
        tracing::warn!(reason = %reason, "gateway chat_completions blocked by perplexity filter");
        return Err(GatewayError::BadRequest {
            message: format!("blocked by perplexity filter: {reason}"),
        });
    }

    let model_override = req.model;

    if req.stream {
        return v1_chat_completions_stream(&state, &last_user, model_override).await;
    }

    let agent_req = build_agent_request(&state, last_user, model_override)?;
    let output = run_agent_once(agent_req).await.map_err(|e| {
        tracing::error!(error = %e, "v1_chat_completions agent execution failed");
        GatewayError::AgentExecutionFailed {
            message: e.to_string(),
        }
    })?;

    Ok(Json(ChatCompletionsResponse {
        id: format!("chatcmpl-{}", now_epoch_secs()),
        object: "chat.completion",
        choices: vec![CompletionChoice {
            index: 0,
            message: CompletionChoiceMessage {
                role: "assistant",
                content: output.response_text,
            },
            finish_reason: "stop",
        }],
    })
    .into_response())
}

/// SSE streaming variant of v1/chat/completions (OpenAI-compatible).
async fn v1_chat_completions_stream(
    state: &GatewayState,
    message: &str,
    model_override: Option<String>,
) -> Result<Response, GatewayError> {
    let agent_req = build_agent_request(state, message.to_string(), model_override)?;
    let execution = build_runtime_execution(agent_req).await.map_err(|e| {
        tracing::error!(error = %e, "v1_chat_completions_stream build failed");
        GatewayError::AgentExecutionFailed {
            message: e.to_string(),
        }
    })?;

    let workspace_root = state
        .workspace_root
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?
        .as_ref()
        .clone();

    let (mut rx, _handle) = run_agent_streaming(execution, workspace_root, message.to_string());
    let id = format!("chatcmpl-{}", now_epoch_secs());

    let stream = async_stream::stream! {
        while let Some(chunk) = rx.recv().await {
            if chunk.done {
                let data = json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": "stop"
                    }]
                });
                yield Ok::<_, std::convert::Infallible>(
                    axum::response::sse::Event::default().data(data.to_string())
                );
                yield Ok(axum::response::sse::Event::default().data("[DONE]"));
                break;
            }
            if !chunk.delta.is_empty() {
                let data = json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "choices": [{
                        "index": 0,
                        "delta": {"role": "assistant", "content": chunk.delta},
                        "finish_reason": null
                    }]
                });
                yield Ok::<_, std::convert::Infallible>(
                    axum::response::sse::Event::default().data(data.to_string())
                );
            }
        }
    };

    Ok(axum::response::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response())
}

pub(crate) async fn v1_models(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<ModelsResponse>, GatewayError> {
    authorize_request(&state, &headers, false)?;

    let mut models = Vec::new();
    for provider in agentzero_providers::supported_providers() {
        if let Some((_pid, provider_models)) =
            agentzero_providers::find_models_for_provider(provider.id)
        {
            for model in provider_models {
                models.push(ModelItem {
                    id: model.id.to_string(),
                    object: "model",
                    owned_by: provider.id.to_string(),
                });
            }
        }
    }

    Ok(Json(ModelsResponse {
        object: "list",
        data: models,
    }))
}

pub(crate) async fn api_fallback(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(path): Path<String>,
) -> Result<Json<Value>, GatewayError> {
    authorize_request(&state, &headers, true)?;

    Ok(Json(json!({
        "ok": true,
        "path": path,
    })))
}

/// WebSocket heartbeat interval (ping every 30s).
const WS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
/// Close WebSocket if no pong received within this duration.
const WS_PONG_TIMEOUT: Duration = Duration::from_secs(60);
/// Close WebSocket if no client message received within this duration.
const WS_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

pub(crate) async fn ws_chat(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, GatewayError> {
    authorize_request(&state, &headers, true)?;
    let config_path = state
        .config_path
        .clone()
        .ok_or(GatewayError::AgentUnavailable)?;
    let workspace_root = state
        .workspace_root
        .clone()
        .ok_or(GatewayError::AgentUnavailable)?;
    crate::gateway_metrics::record_ws_connection();
    Ok(ws
        .on_upgrade(move |socket| handle_socket(socket, config_path, workspace_root))
        .into_response())
}

async fn handle_socket(
    mut socket: WebSocket,
    config_path: Arc<PathBuf>,
    workspace_root: Arc<PathBuf>,
) {
    let mut heartbeat = interval(WS_HEARTBEAT_INTERVAL);
    heartbeat.tick().await; // consume the immediate first tick
    let mut last_pong = Instant::now();
    let mut last_activity = Instant::now();

    loop {
        tokio::select! {
            msg = socket.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        last_activity = Instant::now();
                        last_pong = Instant::now(); // text counts as proof of life
                        handle_text_message(
                            &mut socket,
                            &config_path,
                            &workspace_root,
                            text.to_string(),
                        )
                        .await;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        last_pong = Instant::now();
                    }
                    Some(Ok(Message::Binary(_))) => {
                        let _ = socket
                            .send(Message::Text(
                                json!({"type": "error", "message": "binary frames not supported"})
                                    .to_string(),
                            ))
                            .await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        // Axum auto-responds with Pong, but update activity.
                        last_activity = Instant::now();
                        let _ = socket.send(Message::Pong(data)).await;
                    }
                    Some(Err(_)) => break,
                }
            }
            _ = heartbeat.tick() => {
                // Check pong timeout.
                if last_pong.elapsed() > WS_PONG_TIMEOUT {
                    tracing::warn!("WebSocket pong timeout, closing connection");
                    let _ = socket.send(Message::Close(None)).await;
                    break;
                }
                // Check idle timeout.
                if last_activity.elapsed() > WS_IDLE_TIMEOUT {
                    tracing::info!("WebSocket idle timeout, closing connection");
                    let _ = socket.send(Message::Close(None)).await;
                    break;
                }
                // Send heartbeat ping.
                if socket.send(Message::Ping(vec![1, 2, 3, 4])).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Process a single text message from the WebSocket client.
async fn handle_text_message(
    socket: &mut WebSocket,
    config_path: &Arc<PathBuf>,
    workspace_root: &Arc<PathBuf>,
    text: String,
) {
    let req = RunAgentRequest {
        workspace_root: workspace_root.as_ref().clone(),
        config_path: config_path.as_ref().clone(),
        message: text.clone(),
        provider_override: None,
        model_override: None,
        profile_override: None,
        extra_tools: vec![],
    };
    let execution = match build_runtime_execution(req).await {
        Ok(exec) => exec,
        Err(e) => {
            let _ = socket
                .send(Message::Text(
                    json!({"type": "error", "message": e.to_string()}).to_string(),
                ))
                .await;
            return;
        }
    };
    let (mut rx, handle) = run_agent_streaming(execution, workspace_root.as_ref().clone(), text);
    while let Some(chunk) = rx.recv().await {
        if !chunk.delta.is_empty() {
            let frame = json!({
                "type": "delta",
                "delta": chunk.delta,
            });
            if socket.send(Message::Text(frame.to_string())).await.is_err() {
                break;
            }
        }
        if chunk.done {
            break;
        }
    }
    let _ = socket
        .send(Message::Text(json!({"type": "done"}).to_string()))
        .await;
    let _ = handle.await;
}
