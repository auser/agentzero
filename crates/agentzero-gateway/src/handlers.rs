use crate::auth::authorize_request;
use crate::models::{
    ChatCompletionsRequest, ChatCompletionsResponse, ChatRequest, ChatResponse, CompletionChoice,
    CompletionChoiceMessage, HealthResponse, ModelItem, ModelsResponse, PairRequest, PairResponse,
    PingRequest, PingResponse, WebhookResponse,
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
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

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

pub(crate) async fn metrics() -> impl IntoResponse {
    let payload = "# HELP agentzero_gateway_requests_total Total requests\n# TYPE agentzero_gateway_requests_total counter\nagentzero_gateway_requests_total 1\n";
    ([("content-type", "text/plain; version=0.0.4")], payload)
}

pub(crate) async fn pair(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    _body: Option<Json<PairRequest>>,
) -> Result<Json<PairResponse>, StatusCode> {
    let Some(expected_code) = state.pairing_code_valid() else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    let Some(code_header) = headers.get("X-Pairing-Code") else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let Ok(code) = code_header.to_str() else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    if code.trim() != expected_code {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let token = generate_session_token();
    if state.add_paired_token(token.clone()).is_err() {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
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
) -> Result<Json<PingResponse>, StatusCode> {
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
) -> Result<Json<WebhookResponse>, StatusCode> {
    authorize_request(&state, &headers, false)?;

    let Some(delivery) = state.channels.dispatch(&channel, payload).await else {
        return Err(StatusCode::NOT_FOUND);
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
) -> Result<Json<ChatResponse>, StatusCode> {
    authorize_request(&state, &headers, false)?;

    if let Some(reason) = check_perplexity(&req.message, &state.perplexity_filter) {
        tracing::warn!(reason = %reason, "gateway legacy_webhook blocked by perplexity filter");
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(Json(ChatResponse {
        message: format!("echo: {}", req.message),
        tokens_used_estimate: req.message.len() + req.context.len() * 8,
    }))
}

/// Build a `RunAgentRequest` from gateway state. Returns `SERVICE_UNAVAILABLE`
/// if the gateway was started without a config/workspace path.
fn build_agent_request(
    state: &GatewayState,
    message: String,
    model_override: Option<String>,
) -> Result<RunAgentRequest, StatusCode> {
    let config_path = state
        .config_path
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?
        .as_ref()
        .clone();
    let workspace_root = state
        .workspace_root
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?
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
) -> Result<Json<ChatResponse>, StatusCode> {
    authorize_request(&state, &headers, false)?;

    if let Some(reason) = check_perplexity(&req.message, &state.perplexity_filter) {
        tracing::warn!(reason = %reason, "gateway api_chat blocked by perplexity filter");
        return Err(StatusCode::BAD_REQUEST);
    }

    let agent_req = build_agent_request(&state, req.message, None)?;
    let output = run_agent_once(agent_req).await.map_err(|e| {
        tracing::error!(error = %e, "api_chat agent execution failed");
        StatusCode::INTERNAL_SERVER_ERROR
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
) -> Result<Response, StatusCode> {
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
        return Err(StatusCode::BAD_REQUEST);
    }

    let model_override = req.model;

    if req.stream {
        return v1_chat_completions_stream(&state, &last_user, model_override).await;
    }

    let agent_req = build_agent_request(&state, last_user, model_override)?;
    let output = run_agent_once(agent_req).await.map_err(|e| {
        tracing::error!(error = %e, "v1_chat_completions agent execution failed");
        StatusCode::INTERNAL_SERVER_ERROR
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
) -> Result<Response, StatusCode> {
    let agent_req = build_agent_request(state, message.to_string(), model_override)?;
    let execution = build_runtime_execution(agent_req).await.map_err(|e| {
        tracing::error!(error = %e, "v1_chat_completions_stream build failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let workspace_root = state
        .workspace_root
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?
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
) -> Result<Json<ModelsResponse>, StatusCode> {
    authorize_request(&state, &headers, false)?;

    Ok(Json(ModelsResponse {
        object: "list",
        data: vec![
            ModelItem {
                id: "gpt-4o-mini",
                object: "model",
                owned_by: "openai",
            },
            ModelItem {
                id: "claude-sonnet-4",
                object: "model",
                owned_by: "anthropic",
            },
        ],
    }))
}

pub(crate) async fn api_fallback(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(path): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    authorize_request(&state, &headers, true)?;

    Ok(Json(json!({
        "ok": true,
        "path": path,
    })))
}

pub(crate) async fn ws_chat(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    authorize_request(&state, &headers, true)?;
    let config_path = state
        .config_path
        .clone()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let workspace_root = state
        .workspace_root
        .clone()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(ws
        .on_upgrade(move |socket| handle_socket(socket, config_path, workspace_root))
        .into_response())
}

async fn handle_socket(
    mut socket: WebSocket,
    config_path: Arc<PathBuf>,
    workspace_root: Arc<PathBuf>,
) {
    while let Some(Ok(msg)) = socket.next().await {
        match msg {
            Message::Text(text) => {
                let req = RunAgentRequest {
                    workspace_root: workspace_root.as_ref().clone(),
                    config_path: config_path.as_ref().clone(),
                    message: text.to_string(),
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
                        continue;
                    }
                };
                let (mut rx, handle) = run_agent_streaming(
                    execution,
                    workspace_root.as_ref().clone(),
                    text.to_string(),
                );
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
            Message::Close(_) => break,
            _ => {}
        }
    }
}
