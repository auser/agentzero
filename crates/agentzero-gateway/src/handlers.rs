use crate::auth::authorize_request;
use crate::models::{
    ChatCompletionsRequest, ChatCompletionsResponse, ChatRequest, ChatResponse, CompletionChoice,
    CompletionChoiceMessage, HealthResponse, ModelItem, ModelsResponse, PairRequest, PairResponse,
    PingRequest, PingResponse, WebhookResponse,
};
use crate::state::GatewayState;
use crate::util::{generate_session_token, now_epoch_secs};
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
    let Some(expected_code) = state.pairing_code.as_deref() else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    let Some(code_header) = headers.get("X-Pairing-Code") else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let Ok(code) = code_header.to_str() else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    if code.trim() != expected_code.as_str() {
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

    let Some(delivery) = state.channels.dispatch(&channel, payload) else {
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

    Ok(Json(ChatResponse {
        message: format!("echo: {}", req.message),
        tokens_used_estimate: req.message.len() + req.context.len() * 8,
    }))
}

pub(crate) async fn api_chat(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, StatusCode> {
    authorize_request(&state, &headers, false)?;

    Ok(Json(ChatResponse {
        message: format!("agent reply: {}", req.message),
        tokens_used_estimate: req.message.len() + req.context.len() * 8,
    }))
}

pub(crate) async fn v1_chat_completions(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<ChatCompletionsRequest>,
) -> Result<Json<ChatCompletionsResponse>, StatusCode> {
    authorize_request(&state, &headers, false)?;

    let last_user = req
        .messages
        .iter()
        .rev()
        .find(|msg| msg.role == "user")
        .map(|msg| msg.content.clone())
        .unwrap_or_else(|| "hello".to_string());
    let model = req.model.unwrap_or_else(|| "gpt-4o-mini".to_string());

    Ok(Json(ChatCompletionsResponse {
        id: format!("chatcmpl-{}", now_epoch_secs()),
        object: "chat.completion",
        choices: vec![CompletionChoice {
            index: 0,
            message: CompletionChoiceMessage {
                role: "assistant",
                content: format!("({model}) {last_user}"),
            },
            finish_reason: "stop",
        }],
    }))
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
    Ok(ws.on_upgrade(handle_socket).into_response())
}

async fn handle_socket(mut socket: WebSocket) {
    while let Some(Ok(msg)) = socket.next().await {
        match msg {
            Message::Text(text) => {
                let _ = socket.send(Message::Text(format!("echo: {text}"))).await;
            }
            Message::Binary(data) => {
                let _ = socket.send(Message::Binary(data)).await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}
