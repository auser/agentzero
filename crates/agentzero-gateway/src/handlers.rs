use crate::api_keys::Scope;
use crate::auth::{authorize_request, authorize_with_scope};
use crate::models::{
    AgentDetailResponse, AgentListResponse, ApiFallbackResponse, ApprovalsListResponse,
    AsyncSubmitRequest, AsyncSubmitResponse, CancelQuery, CancelResponse, ChatCompletionsRequest,
    ChatCompletionsResponse, ChatRequest, ChatResponse, CompletionChoice, CompletionChoiceMessage,
    ConfigResponse, ConfigSection, CreateAgentRequest, CreateAgentResponse, EstopResponse,
    EventItem, EventListResponse, EventStreamQuery, GatewayError, HealthResponse, JobListItem,
    JobListQuery, JobListResponse, JobStatusResponse, LivenessResponse, MemoryForgetRequest,
    MemoryForgetResponse, MemoryListItem, MemoryListQuery, MemoryListResponse, MemoryRecallRequest,
    ModelItem, ModelsResponse, PairRequest, PairResponse, PingRequest, PingResponse, ReadyResponse,
    ToolSummary, ToolsResponse, TranscriptResponse, UpdateAgentRequest, WebhookPayload,
    WebhookQuery, WebhookResponse, WsRunQuery,
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
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub(crate) async fn health_ready(State(state): State<GatewayState>) -> Json<ReadyResponse> {
    let mut checks_failed = Vec::new();

    // Check memory store availability when config is loaded.
    if state.memory_store.is_none() && state.config_path.is_some() {
        checks_failed.push("memory_store".to_string());
    }

    let ready = checks_failed.is_empty();
    Json(ReadyResponse {
        ready,
        service: (*state.service_name).clone(),
        version: env!("CARGO_PKG_VERSION"),
        checks_failed,
    })
}

/// GET /health/live — liveness probe that verifies the tokio runtime is responsive.
pub(crate) async fn health_live() -> Json<LivenessResponse> {
    // Spawn a trivial task and confirm it completes within 1 second.
    // If the runtime is deadlocked or overloaded, this will time out.
    let alive = tokio::time::timeout(Duration::from_secs(1), tokio::spawn(async { 42 }))
        .await
        .is_ok();
    Json(LivenessResponse { alive })
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
        crate::audit::audit(
            crate::audit::AuditEvent::PairFailure,
            "invalid pairing code",
            "",
            "/pair",
        );
        return Err(GatewayError::AuthFailed);
    }

    let token = generate_session_token();
    if state.add_paired_token(token.clone()).is_err() {
        return Err(GatewayError::AgentExecutionFailed {
            message: "failed to persist pairing token".to_string(),
        });
    }

    crate::audit::audit(
        crate::audit::AuditEvent::PairSuccess,
        "pairing code exchanged for token",
        "",
        "/pair",
    );

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
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    Ok(Json(PingResponse {
        ok: true,
        echo: req.message,
    }))
}

pub(crate) async fn webhook(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(channel): Path<String>,
    query: axum::extract::Query<WebhookQuery>,
    Json(payload): Json<WebhookPayload>,
) -> Result<Json<WebhookResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    if !is_valid_channel_name(&channel) {
        return Err(GatewayError::BadRequest {
            message: format!(
                "invalid channel name '{channel}': must be 1-64 chars, alphanumeric/hyphen/underscore only"
            ),
        });
    }

    // If agent_id is provided, validate that the agent exists.
    if let Some(ref agent_id) = query.agent_id {
        validate_agent_exists(&state, agent_id)?;
        tracing::info!(
            channel = %channel,
            agent_id = %agent_id,
            "webhook targeting specific agent"
        );
    }

    let Some(delivery) = state.channels.dispatch(&channel, payload.inner).await else {
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

/// POST /v1/hooks/:channel/:agent_id — webhook with agent targeting (convenience route).
pub(crate) async fn webhook_with_agent(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path((channel, agent_id)): Path<(String, String)>,
    Json(payload): Json<WebhookPayload>,
) -> Result<Json<WebhookResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    if !is_valid_channel_name(&channel) {
        return Err(GatewayError::BadRequest {
            message: format!(
                "invalid channel name '{channel}': must be 1-64 chars, alphanumeric/hyphen/underscore only"
            ),
        });
    }

    validate_agent_exists(&state, &agent_id)?;

    tracing::info!(
        channel = %channel,
        agent_id = %agent_id,
        "webhook targeting specific agent"
    );

    let Some(delivery) = state.channels.dispatch(&channel, payload.inner).await else {
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

/// Validate that an agent exists in either the dynamic store or presence store.
fn validate_agent_exists(state: &GatewayState, agent_id: &str) -> Result<(), GatewayError> {
    // Check dynamic store.
    if let Some(store) = &state.agent_store {
        if store.get(agent_id).is_some() {
            return Ok(());
        }
    }
    // Check static agents via presence store.
    // Note: presence store is async but we can't await here in a sync fn.
    // For now, accept if agent_store found it; otherwise reject for dynamic agents.
    // Static agents route through the normal webhook path without targeting.
    Err(GatewayError::NotFound {
        resource: format!("agent/{agent_id}"),
    })
}

pub(crate) async fn legacy_webhook(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    if let Some(reason) = check_perplexity(&req.message, &state.effective_perplexity_filter()) {
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
        conversation_id: None,
        agent_store: None,
    })
}

pub(crate) async fn api_chat(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    if let Some(reason) = check_perplexity(&req.message, &state.effective_perplexity_filter()) {
        tracing::warn!(reason = %reason, "gateway api_chat blocked by perplexity filter");
        return Err(GatewayError::BadRequest {
            message: format!("blocked by perplexity filter: {reason}"),
        });
    }

    // When the swarm is enabled, route through the gateway channel so
    // pipelines (e.g. research-to-brief) can handle the request.
    if let Some(ref gw_channel) = state.gateway_channel {
        let response = gw_channel
            .submit(req.message, Duration::from_secs(600))
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "api_chat pipeline execution failed");
                GatewayError::AgentExecutionFailed {
                    message: e.to_string(),
                }
            })?;

        return Ok(Json(ChatResponse {
            message: response,
            tokens_used_estimate: 0,
        }));
    }

    // Fallback: single-agent execution (no swarm).
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
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    let last_user = req
        .messages
        .iter()
        .rev()
        .find(|msg| msg.role == "user")
        .map(|msg| msg.content.clone())
        .unwrap_or_else(|| "hello".to_string());

    if let Some(reason) = check_perplexity(&last_user, &state.effective_perplexity_filter()) {
        tracing::warn!(reason = %reason, "gateway chat_completions blocked by perplexity filter");
        return Err(GatewayError::BadRequest {
            message: format!("blocked by perplexity filter: {reason}"),
        });
    }

    let model_override = req.model;

    if req.stream {
        return v1_chat_completions_stream(&state, &last_user, model_override).await;
    }

    // Route through swarm pipeline when available.
    if let Some(ref gw_channel) = state.gateway_channel {
        let response = gw_channel
            .submit(last_user, Duration::from_secs(600))
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "v1_chat_completions pipeline execution failed");
                GatewayError::AgentExecutionFailed {
                    message: e.to_string(),
                }
            })?;

        return Ok(Json(ChatCompletionsResponse {
            id: format!("chatcmpl-{}", now_epoch_secs()),
            object: "chat.completion",
            choices: vec![CompletionChoice {
                index: 0,
                message: CompletionChoiceMessage {
                    role: "assistant",
                    content: response,
                },
                finish_reason: "stop",
            }],
        })
        .into_response());
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
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

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
) -> Result<Json<ApiFallbackResponse>, GatewayError> {
    authorize_request(&state, &headers, true)?;

    Ok(Json(ApiFallbackResponse { ok: true, path }))
}

/// WebSocket heartbeat interval (ping every 30s).
const WS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
/// Close WebSocket if no pong received within this duration.
const WS_PONG_TIMEOUT: Duration = Duration::from_secs(60);
/// Close WebSocket if no client message received within this duration.
const WS_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// Maximum WebSocket message size (2 MB).
pub(crate) const WS_MAX_MESSAGE_SIZE: usize = 2 * 1024 * 1024;

pub(crate) async fn ws_chat(
    State(state): State<GatewayState>,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
    mut headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, GatewayError> {
    // Browser WebSocket API cannot set custom headers, so accept the token
    // as a query parameter and inject it into the headers for auth.
    if !headers.contains_key(axum::http::header::AUTHORIZATION) {
        if let Some(token) = query.get("token") {
            if let Ok(val) = format!("Bearer {token}").parse() {
                headers.insert(axum::http::header::AUTHORIZATION, val);
            }
        }
    }
    authorize_with_scope(&state, &headers, true, &Scope::RunsWrite)?;
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
        .max_message_size(WS_MAX_MESSAGE_SIZE)
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
        conversation_id: None,
        agent_store: None,
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

// ---------------------------------------------------------------------------
// Async job submission: /v1/runs
// ---------------------------------------------------------------------------

/// POST /v1/runs — submit an async agent job. Returns 202 Accepted with a run_id.
///
/// Supports four queue modes via the `mode` field:
/// - `steer` (default): Route to a single agent.
/// - `followup`: Append to an existing run's conversation (requires `run_id`).
/// - `collect`: Fan-out to all available agents, merge responses.
/// - `interrupt`: Cancel active runs in the lane, then submit new job.
pub(crate) async fn async_submit(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<AsyncSubmitRequest>,
) -> Result<Response, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    let job_store = state
        .job_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;

    if let Some(reason) = check_perplexity(&req.message, &state.effective_perplexity_filter()) {
        tracing::warn!(reason = %reason, "async_submit blocked by perplexity filter");
        return Err(GatewayError::BadRequest {
            message: format!("blocked by perplexity filter: {reason}"),
        });
    }

    let mode = req.mode.as_deref().unwrap_or("steer");
    let lane = agentzero_core::Lane::Main;

    match mode {
        "followup" => {
            // Followup mode: append to existing run's conversation.
            let existing_run_id = req.run_id.as_deref().ok_or(GatewayError::BadRequest {
                message: "followup mode requires a `run_id` field".to_string(),
            })?;
            let target_run_id = agentzero_core::RunId(existing_run_id.to_string());

            // Verify the target run exists.
            if job_store.get(&target_run_id).await.is_none() {
                return Err(GatewayError::NotFound {
                    resource: format!("run {existing_run_id}"),
                });
            }

            // Submit a new run linked to the original conversation.
            let run_id = job_store.submit("agent".to_string(), lane, None).await;

            let mut agent_req = build_agent_request(&state, req.message, req.model)?;
            agent_req.conversation_id = Some(existing_run_id.to_string());

            let store = job_store.clone();
            let rid = run_id.clone();
            tokio::spawn(async move {
                store
                    .update_status(&rid, agentzero_core::JobStatus::Running)
                    .await;
                match run_agent_once(agent_req).await {
                    Ok(output) => {
                        store
                            .update_status(
                                &rid,
                                agentzero_core::JobStatus::Completed {
                                    result: output.response_text,
                                },
                            )
                            .await;
                    }
                    Err(e) => {
                        store
                            .update_status(
                                &rid,
                                agentzero_core::JobStatus::Failed {
                                    error: e.to_string(),
                                },
                            )
                            .await;
                    }
                }
            });

            let resp = AsyncSubmitResponse {
                run_id: run_id.0.clone(),
                accepted_at: chrono_now_iso(),
            };
            Ok((axum::http::StatusCode::ACCEPTED, Json(resp)).into_response())
        }

        "collect" => {
            // Collect mode: fan-out to multiple agents, merge all responses.
            let run_id = job_store.submit("agent".to_string(), lane, None).await;

            // Capture the fields we need to rebuild requests per agent.
            let message = req.message.clone();
            let model = req.model.clone();
            let state_clone = state.clone();
            let store = job_store.clone();
            let rid = run_id.clone();
            let collect_count = 3usize; // fan-out to N parallel copies

            tokio::spawn(async move {
                store
                    .update_status(&rid, agentzero_core::JobStatus::Running)
                    .await;

                // Spawn parallel agent invocations, each building its own request.
                let mut handles = Vec::with_capacity(collect_count);
                for _ in 0..collect_count {
                    let msg = message.clone();
                    let mdl = model.clone();
                    let st = state_clone.clone();
                    handles.push(tokio::spawn(async move {
                        let req = match build_agent_request(&st, msg, mdl) {
                            Ok(r) => r,
                            Err(e) => return Err(anyhow::anyhow!("{e:?}")),
                        };
                        run_agent_once(req).await
                    }));
                }

                // Collect all results.
                let mut results = Vec::with_capacity(collect_count);
                for handle in handles {
                    match handle.await {
                        Ok(Ok(output)) => results.push(output.response_text),
                        Ok(Err(e)) => results.push(format!("[error] {e}")),
                        Err(e) => results.push(format!("[join error] {e}")),
                    }
                }

                // Merge results into a single response.
                let merged = results
                    .iter()
                    .enumerate()
                    .map(|(i, r)| format!("--- Agent {} ---\n{}", i + 1, r))
                    .collect::<Vec<_>>()
                    .join("\n\n");

                store
                    .update_status(
                        &rid,
                        agentzero_core::JobStatus::Completed { result: merged },
                    )
                    .await;
            });

            let resp = AsyncSubmitResponse {
                run_id: run_id.0.clone(),
                accepted_at: chrono_now_iso(),
            };
            Ok((axum::http::StatusCode::ACCEPTED, Json(resp)).into_response())
        }

        "interrupt" => {
            // Interrupt mode: cancel all active runs, then submit new job.
            let active_runs = job_store.list_all(None).await;
            for job in &active_runs {
                if !job.status.is_terminal() {
                    job_store
                        .update_status(&job.run_id, agentzero_core::JobStatus::Cancelled)
                        .await;
                }
            }

            let run_id = job_store.submit("agent".to_string(), lane, None).await;

            let agent_req = build_agent_request(&state, req.message, req.model)?;
            let store = job_store.clone();
            let rid = run_id.clone();
            tokio::spawn(async move {
                store
                    .update_status(&rid, agentzero_core::JobStatus::Running)
                    .await;
                match run_agent_once(agent_req).await {
                    Ok(output) => {
                        store
                            .update_status(
                                &rid,
                                agentzero_core::JobStatus::Completed {
                                    result: output.response_text,
                                },
                            )
                            .await;
                    }
                    Err(e) => {
                        store
                            .update_status(
                                &rid,
                                agentzero_core::JobStatus::Failed {
                                    error: e.to_string(),
                                },
                            )
                            .await;
                    }
                }
            });

            let resp = AsyncSubmitResponse {
                run_id: run_id.0.clone(),
                accepted_at: chrono_now_iso(),
            };
            Ok((axum::http::StatusCode::ACCEPTED, Json(resp)).into_response())
        }

        _ => {
            // Steer mode (default): single agent submission.
            let run_id = job_store.submit("agent".to_string(), lane, None).await;

            let agent_req = build_agent_request(&state, req.message, req.model)?;
            let store = job_store.clone();
            let rid = run_id.clone();
            tokio::spawn(async move {
                store
                    .update_status(&rid, agentzero_core::JobStatus::Running)
                    .await;
                match run_agent_once(agent_req).await {
                    Ok(output) => {
                        store
                            .update_status(
                                &rid,
                                agentzero_core::JobStatus::Completed {
                                    result: output.response_text,
                                },
                            )
                            .await;
                    }
                    Err(e) => {
                        store
                            .update_status(
                                &rid,
                                agentzero_core::JobStatus::Failed {
                                    error: e.to_string(),
                                },
                            )
                            .await;
                    }
                }
            });

            let resp = AsyncSubmitResponse {
                run_id: run_id.0.clone(),
                accepted_at: chrono_now_iso(),
            };
            Ok((axum::http::StatusCode::ACCEPTED, Json(resp)).into_response())
        }
    }
}

/// GET /v1/runs/:run_id — poll job status.
pub(crate) async fn job_status(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(run_id_str): Path<String>,
) -> Result<Json<JobStatusResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let job_store = state
        .job_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;

    let run_id = agentzero_core::RunId(run_id_str.clone());
    let record = job_store.get(&run_id).await.ok_or(GatewayError::NotFound {
        resource: format!("run {run_id_str}"),
    })?;

    let (status_str, result, error) = match &record.status {
        agentzero_core::JobStatus::Pending => ("pending", None, None),
        agentzero_core::JobStatus::Running => ("running", None, None),
        agentzero_core::JobStatus::Completed { result } => {
            ("completed", Some(result.clone()), None)
        }
        agentzero_core::JobStatus::Failed { error } => ("failed", None, Some(error.clone())),
        agentzero_core::JobStatus::Cancelled => ("cancelled", None, None),
    };

    Ok(Json(JobStatusResponse {
        run_id: run_id_str,
        status: status_str.to_string(),
        agent_id: record.agent_id,
        result,
        error,
    }))
}

/// GET /v1/runs/:run_id/result — get completed result (or 404/202).
pub(crate) async fn job_result(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(run_id_str): Path<String>,
) -> Result<Response, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let job_store = state
        .job_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;

    let run_id = agentzero_core::RunId(run_id_str.clone());
    let record = job_store.get(&run_id).await.ok_or(GatewayError::NotFound {
        resource: format!("run {run_id_str}"),
    })?;

    match record.status {
        agentzero_core::JobStatus::Completed { result } => Ok(Json(json!({
            "run_id": run_id_str,
            "status": "completed",
            "result": result,
        }))
        .into_response()),
        agentzero_core::JobStatus::Failed { error } => Ok(Json(json!({
            "run_id": run_id_str,
            "status": "failed",
            "error": error,
        }))
        .into_response()),
        _ => {
            // Still running — return 202 Accepted.
            Ok((
                axum::http::StatusCode::ACCEPTED,
                Json(json!({
                    "run_id": run_id_str,
                    "status": "pending",
                })),
            )
                .into_response())
        }
    }
}

fn chrono_now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{secs}")
}

// ---------------------------------------------------------------------------
// Job management: cancel, list, events
// ---------------------------------------------------------------------------

/// DELETE /v1/runs/:run_id — cancel a pending or running job.
pub(crate) async fn job_cancel(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(run_id_str): Path<String>,
    axum::extract::Query(query): axum::extract::Query<CancelQuery>,
) -> Result<Json<CancelResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsManage)?;

    let job_store = state
        .job_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;
    let run_id = agentzero_core::RunId(run_id_str.clone());

    if job_store.get(&run_id).await.is_none() {
        return Err(GatewayError::NotFound {
            resource: format!("run {run_id_str}"),
        });
    }

    if query.cascade.unwrap_or(false) {
        let cancelled_ids = job_store.cascade_cancel(&run_id).await;
        Ok(Json(CancelResponse {
            run_id: run_id_str,
            cancelled: !cancelled_ids.is_empty(),
            cascade_count: Some(cancelled_ids.len()),
            cancelled_ids: Some(
                cancelled_ids
                    .iter()
                    .map(|id| id.as_str().to_string())
                    .collect(),
            ),
        }))
    } else {
        let cancelled = job_store.cancel(&run_id).await;
        Ok(Json(CancelResponse {
            run_id: run_id_str,
            cancelled,
            cascade_count: None,
            cancelled_ids: None,
        }))
    }
}

/// GET /v1/runs — list all jobs, optionally filtered by status query param.
pub(crate) async fn job_list(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    query: axum::extract::Query<JobListQuery>,
) -> Result<Json<JobListResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let job_store = state
        .job_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;
    let jobs = job_store.list_all(query.status.as_deref()).await;

    let items: Vec<JobListItem> = jobs
        .iter()
        .map(|r| {
            let (status_str, result, error) = match &r.status {
                agentzero_core::JobStatus::Pending => ("pending", None, None),
                agentzero_core::JobStatus::Running => ("running", None, None),
                agentzero_core::JobStatus::Completed { result } => {
                    ("completed", Some(result.clone()), None)
                }
                agentzero_core::JobStatus::Failed { error } => {
                    ("failed", None, Some(error.clone()))
                }
                agentzero_core::JobStatus::Cancelled => ("cancelled", None, None),
            };
            JobListItem {
                run_id: r.run_id.0.clone(),
                status: status_str,
                agent_id: r.agent_id.clone(),
                result,
                error,
                tokens_used: r.tokens_used,
                cost_microdollars: r.cost_microdollars,
            }
        })
        .collect();

    let total = items.len();
    Ok(Json(JobListResponse {
        object: "list",
        data: items,
        total,
    }))
}

/// GET /v1/runs/:run_id/events — stream job events as newline-delimited JSON.
///
/// Returns the status transitions for a job as a sequence of events.
/// If the job is still running, returns events so far.
pub(crate) async fn job_events(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(run_id_str): Path<String>,
) -> Result<Json<EventListResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let job_store = state
        .job_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;
    let run_id = agentzero_core::RunId(run_id_str.clone());

    if job_store.get(&run_id).await.is_none() {
        return Err(GatewayError::NotFound {
            resource: format!("run {run_id_str}"),
        });
    }

    // Use the persistent event log instead of reconstructing from state.
    let log_events = job_store.get_events(&run_id).await;
    let events: Vec<EventItem> = log_events
        .iter()
        .map(|e| {
            use agentzero_orchestrator::EventKind;
            match &e.kind {
                EventKind::Created => EventItem {
                    event_type: "created",
                    run_id: run_id_str.clone(),
                    tool: None,
                    result: None,
                    error: None,
                },
                EventKind::Running => EventItem {
                    event_type: "running",
                    run_id: run_id_str.clone(),
                    tool: None,
                    result: None,
                    error: None,
                },
                EventKind::ToolCall { name } => EventItem {
                    event_type: "tool_call",
                    run_id: run_id_str.clone(),
                    tool: Some(name.clone()),
                    result: None,
                    error: None,
                },
                EventKind::ToolResult { name } => EventItem {
                    event_type: "tool_result",
                    run_id: run_id_str.clone(),
                    tool: Some(name.clone()),
                    result: None,
                    error: None,
                },
                EventKind::Completed { summary } => EventItem {
                    event_type: "completed",
                    run_id: run_id_str.clone(),
                    tool: None,
                    result: Some(summary.clone()),
                    error: None,
                },
                EventKind::Failed { error } => EventItem {
                    event_type: "failed",
                    run_id: run_id_str.clone(),
                    tool: None,
                    result: None,
                    error: Some(error.clone()),
                },
                EventKind::Cancelled => EventItem {
                    event_type: "cancelled",
                    run_id: run_id_str.clone(),
                    tool: None,
                    result: None,
                    error: None,
                },
            }
        })
        .collect();

    let total = events.len();
    Ok(Json(EventListResponse {
        object: "list",
        run_id: run_id_str,
        events,
        total,
    }))
}

/// GET /v1/runs/:run_id/transcript — retrieve full conversation transcript for a run.
pub(crate) async fn job_transcript(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(run_id_str): Path<String>,
) -> Result<Json<TranscriptResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let memory_store = state
        .memory_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;

    // The agent runtime uses run_id as conversation_id.
    let entries = memory_store
        .recent_for_conversation(&run_id_str, 1000)
        .await
        .map_err(|e| GatewayError::AgentExecutionFailed {
            message: format!("failed to retrieve transcript: {e}"),
        })?;

    if entries.is_empty() {
        return Err(GatewayError::NotFound {
            resource: format!("transcript for run {run_id_str}"),
        });
    }

    let transcript: Vec<crate::models::TranscriptEntry> = entries
        .into_iter()
        .map(|e| crate::models::TranscriptEntry {
            role: e.role,
            content: e.content,
            created_at: e.created_at,
        })
        .collect();

    let total = transcript.len();
    Ok(Json(TranscriptResponse {
        object: "transcript",
        run_id: run_id_str,
        entries: transcript,
        total,
    }))
}

// ---------------------------------------------------------------------------
// WebSocket run subscription: /ws/runs/:run_id
/// GET /v1/agents — list all registered agents (static from TOML + dynamic from store).
pub(crate) async fn agents_list(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<AgentListResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let mut data: Vec<AgentDetailResponse> = Vec::new();

    // Dynamic agents from agent store (richer metadata available).
    if let Some(store) = &state.agent_store {
        for record in store.list() {
            data.push(agent_record_to_detail(&record, "dynamic"));
        }
    }

    // Static agents from presence store (TOML-configured, limited metadata).
    if let Some(presence) = &state.presence_store {
        let records = presence.list_all().await;
        for r in &records {
            // Skip if already added from the dynamic store.
            if data.iter().any(|d| d.agent_id == r.agent_id) {
                continue;
            }
            let status_str = match r.status {
                agentzero_orchestrator::PresenceStatus::Alive => "active",
                agentzero_orchestrator::PresenceStatus::Stale => "stopped",
                agentzero_orchestrator::PresenceStatus::Dead => "stopped",
            };
            data.push(AgentDetailResponse {
                agent_id: r.agent_id.clone(),
                name: r.agent_id.clone(),
                description: String::new(),
                system_prompt: None,
                provider: String::new(),
                model: String::new(),
                keywords: vec![],
                allowed_tools: vec![],
                channels: vec![],
                status: status_str.to_string(),
                source: "static",
                created_at: 0,
                updated_at: 0,
            });
        }
    }

    // Return empty list if neither store is configured (instead of error).
    let total = data.len();
    Ok(Json(AgentListResponse {
        object: "list",
        data,
        total,
    }))
}

// ---------------------------------------------------------------------------
// Agent management CRUD
// ---------------------------------------------------------------------------

/// POST /v1/agents — create a dynamic agent at runtime.
pub(crate) async fn create_agent(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<CreateAgentRequest>,
) -> Result<(axum::http::StatusCode, Json<CreateAgentResponse>), GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let agent_store = state
        .agent_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;

    if req.name.trim().is_empty() {
        return Err(GatewayError::BadRequest {
            message: "agent name is required".to_string(),
        });
    }

    let record = agentzero_orchestrator::AgentRecord {
        agent_id: String::new(), // auto-generated
        name: req.name,
        description: req.description,
        system_prompt: req.system_prompt,
        provider: req.provider,
        model: req.model,
        keywords: req.keywords,
        allowed_tools: req.allowed_tools,
        channels: req.channels,
        created_at: 0,
        updated_at: 0,
        status: agentzero_orchestrator::AgentStatus::Active,
    };

    let created = agent_store
        .create(record)
        .map_err(|e| GatewayError::BadRequest {
            message: e.to_string(),
        })?;

    // Auto-register webhooks for channels that have bot tokens.
    let public_url = resolve_public_url(&state);
    if let Some(ref base_url) = public_url {
        for (channel_name, channel_cfg) in &created.channels {
            let instance_cfg = agent_channel_to_instance_config(channel_cfg);
            match agentzero_channels::build_channel_instance(channel_name, &instance_cfg) {
                Ok(Some(ch)) => {
                    let webhook_url = format!(
                        "{}/v1/hooks/{}/{}",
                        base_url.trim_end_matches('/'),
                        channel_name,
                        created.agent_id
                    );
                    if let Err(e) = ch.register_webhook(&webhook_url).await {
                        tracing::warn!(
                            agent_id = %created.agent_id,
                            channel = %channel_name,
                            error = %e,
                            "failed to auto-register webhook"
                        );
                    }
                }
                Ok(None) => {
                    tracing::debug!(channel = %channel_name, "channel not compiled in, skipping webhook registration");
                }
                Err(e) => {
                    tracing::warn!(channel = %channel_name, error = %e, "failed to build channel for webhook registration");
                }
            }
        }
    }

    let channel_names: Vec<String> = created.channels.keys().cloned().collect();

    Ok((
        axum::http::StatusCode::CREATED,
        Json(CreateAgentResponse {
            agent_id: created.agent_id,
            name: created.name,
            status: "active".to_string(),
            channels: channel_names,
            created_at: created.created_at,
        }),
    ))
}

/// GET /v1/agents/:id — get agent details.
pub(crate) async fn get_agent(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<AgentDetailResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    // Check dynamic store first.
    if let Some(store) = &state.agent_store {
        if let Some(record) = store.get(&agent_id) {
            return Ok(Json(agent_record_to_detail(&record, "dynamic")));
        }
    }

    // Check static (TOML) agents via presence store.
    if let Some(presence) = &state.presence_store {
        let records = presence.list_all().await;
        if records.iter().any(|r| r.agent_id == agent_id) {
            return Ok(Json(AgentDetailResponse {
                agent_id: agent_id.clone(),
                name: agent_id,
                description: String::new(),
                system_prompt: None,
                provider: String::new(),
                model: String::new(),
                keywords: vec![],
                allowed_tools: vec![],
                channels: vec![],
                status: "active".to_string(),
                source: "config",
                created_at: 0,
                updated_at: 0,
            }));
        }
    }

    Err(GatewayError::NotFound {
        resource: format!("agent/{agent_id}"),
    })
}

/// PATCH /v1/agents/:id — update agent config or toggle status.
pub(crate) async fn update_agent(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
    Json(req): Json<UpdateAgentRequest>,
) -> Result<Json<AgentDetailResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let agent_store = state
        .agent_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;

    // Handle status-only toggle (e.g. from the UI switch).
    if let Some(ref status_str) = req.status {
        let new_status = match status_str.as_str() {
            "active" => agentzero_orchestrator::AgentStatus::Active,
            "stopped" => agentzero_orchestrator::AgentStatus::Stopped,
            other => {
                return Err(GatewayError::BadRequest {
                    message: format!("invalid status '{other}': must be 'active' or 'stopped'"),
                })
            }
        };
        agent_store
            .set_status(&agent_id, new_status)
            .map_err(|e| GatewayError::BadRequest {
                message: e.to_string(),
            })?;
    }

    // Apply any field updates.
    let update = agentzero_orchestrator::AgentUpdate {
        name: req.name,
        description: req.description,
        system_prompt: req.system_prompt,
        provider: req.provider,
        model: req.model,
        keywords: req.keywords,
        allowed_tools: req.allowed_tools,
        channels: req.channels,
    };

    let updated = agent_store
        .update(&agent_id, update)
        .map_err(|e| GatewayError::BadRequest {
            message: e.to_string(),
        })?
        .ok_or(GatewayError::NotFound {
            resource: format!("agent/{agent_id}"),
        })?;

    Ok(Json(agent_record_to_detail(&updated, "dynamic")))
}

/// DELETE /v1/agents/:id — delete a dynamic agent.
pub(crate) async fn delete_agent(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let agent_store = state
        .agent_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;

    // Fetch record before deletion so we can deregister webhooks.
    let record = agent_store.get(&agent_id);

    let removed = agent_store
        .delete(&agent_id)
        .map_err(|e| GatewayError::BadRequest {
            message: e.to_string(),
        })?;

    if !removed {
        return Err(GatewayError::NotFound {
            resource: format!("agent/{agent_id}"),
        });
    }

    // Deregister webhooks for channels that had bot tokens.
    if let Some(record) = record {
        for (channel_name, channel_cfg) in &record.channels {
            let instance_cfg = agent_channel_to_instance_config(channel_cfg);
            if let Ok(Some(ch)) =
                agentzero_channels::build_channel_instance(channel_name, &instance_cfg)
            {
                if let Err(e) = ch.deregister_webhook().await {
                    tracing::warn!(
                        agent_id = %agent_id,
                        channel = %channel_name,
                        error = %e,
                        "failed to deregister webhook"
                    );
                }
            }
        }
    }

    Ok(Json(json!({
        "agent_id": agent_id,
        "deleted": true,
    })))
}

/// Convert an `AgentRecord` to an `AgentDetailResponse`.
fn agent_record_to_detail(
    record: &agentzero_orchestrator::AgentRecord,
    source: &'static str,
) -> AgentDetailResponse {
    let status = match record.status {
        agentzero_orchestrator::AgentStatus::Active => "active",
        agentzero_orchestrator::AgentStatus::Stopped => "stopped",
    };
    AgentDetailResponse {
        agent_id: record.agent_id.clone(),
        name: record.name.clone(),
        description: record.description.clone(),
        system_prompt: record.system_prompt.clone(),
        provider: record.provider.clone(),
        model: record.model.clone(),
        keywords: record.keywords.clone(),
        allowed_tools: record.allowed_tools.clone(),
        channels: record.channels.keys().cloned().collect(),
        status: status.to_string(),
        source,
        created_at: record.created_at,
        updated_at: record.updated_at,
    }
}

// ---------------------------------------------------------------------------

/// POST /v1/estop — emergency stop: cascade-cancel all active root-level runs.
///
/// Returns the list of cancelled run IDs and the total count.
pub(crate) async fn emergency_stop(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<EstopResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let job_store = state
        .job_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;

    let cancelled_ids = job_store.emergency_stop_all().await;
    let count = cancelled_ids.len();

    crate::audit::audit(
        crate::audit::AuditEvent::Estop,
        &format!("cancelled {} active runs", count),
        "",
        "/v1/estop",
    );

    Ok(Json(EstopResponse {
        emergency_stop: true,
        cancelled_count: count,
        cancelled_ids: cancelled_ids
            .iter()
            .map(|id| id.as_str().to_string())
            .collect(),
    }))
}

// ---------------------------------------------------------------------------

/// GET /ws/runs/:run_id — subscribe to real-time status updates for a job.
///
/// The client connects via WebSocket and receives JSON frames whenever the
/// job status changes. The connection closes automatically when the job
/// reaches a terminal state (completed, failed, cancelled).
///
/// Frames sent to client:
/// - `{"type": "status", "run_id": "...", "status": "pending"|"running"|...}`
/// - `{"type": "completed", "run_id": "...", "result": "..."}`
/// - `{"type": "failed", "run_id": "...", "error": "..."}`
/// - `{"type": "cancelled", "run_id": "..."}`
pub(crate) async fn ws_run_subscribe(
    State(state): State<GatewayState>,
    mut headers: HeaderMap,
    Path(run_id_str): Path<String>,
    query: axum::extract::Query<WsRunQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, GatewayError> {
    if !headers.contains_key(axum::http::header::AUTHORIZATION) {
        if let Some(ref token) = query.token {
            if let Ok(val) = format!("Bearer {token}").parse() {
                headers.insert(axum::http::header::AUTHORIZATION, val);
            }
        }
    }
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let job_store = state
        .job_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?
        .clone();

    let run_id = agentzero_core::RunId(run_id_str.clone());

    // Verify the run exists before upgrading.
    if job_store.get(&run_id).await.is_none() {
        return Err(GatewayError::NotFound {
            resource: format!("run {run_id_str}"),
        });
    }

    let use_blocks = query.format.as_deref() == Some("blocks");

    Ok(ws
        .max_message_size(WS_MAX_MESSAGE_SIZE)
        .on_upgrade(move |socket| handle_run_socket(socket, job_store, run_id, use_blocks))
        .into_response())
}

async fn handle_run_socket(
    mut socket: WebSocket,
    job_store: Arc<agentzero_orchestrator::JobStore>,
    run_id: agentzero_core::RunId,
    use_blocks: bool,
) {
    // Send current status immediately.
    if let Some(record) = job_store.get(&run_id).await {
        let frame = if use_blocks {
            block_status_frame(&run_id, &record.status)
        } else {
            status_frame(&run_id, &record.status)
        };
        if socket.send(Message::Text(frame)).await.is_err() {
            return;
        }
        if record.status.is_terminal() {
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    }

    // Subscribe to status updates via a relay channel so we can select
    // on both the broadcast receiver and socket messages without double-
    // borrowing `socket`.
    let mut rx = job_store.subscribe();
    let (relay_tx, mut relay_rx) = tokio::sync::mpsc::channel::<String>(16);
    let sub_run_id = run_id.clone();

    // Spawn a task that filters broadcast events and relays frames.
    let relay_handle = tokio::spawn(async move {
        while let Ok((notified_run_id, new_status)) = rx.recv().await {
            if notified_run_id != sub_run_id {
                continue;
            }
            let frame = if use_blocks {
                block_status_frame(&sub_run_id, &new_status)
            } else {
                status_frame(&sub_run_id, &new_status)
            };
            let terminal = new_status.is_terminal();
            if relay_tx.send(frame).await.is_err() {
                break;
            }
            if terminal {
                break;
            }
        }
    });

    let deadline = Instant::now() + Duration::from_secs(600);

    loop {
        tokio::select! {
            frame = relay_rx.recv() => {
                match frame {
                    Some(f) => {
                        if socket.send(Message::Text(f)).await.is_err() {
                            break;
                        }
                        // Check if the last sent frame was terminal by peeking at
                        // the relay channel; if the relay task exited, we're done.
                        if relay_rx.is_empty() && relay_handle.is_finished() {
                            let _ = socket.send(Message::Close(None)).await;
                            break;
                        }
                    }
                    None => {
                        // Relay closed — job reached terminal state or broadcast ended.
                        let _ = socket.send(Message::Close(None)).await;
                        break;
                    }
                }
            }
            msg = socket.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        let _ = socket.send(Message::Pong(data)).await;
                    }
                    _ => {} // Ignore other messages.
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                let _ = socket
                    .send(Message::Text(
                        json!({"type": "error", "message": "subscription timeout"}).to_string(),
                    ))
                    .await;
                let _ = socket.send(Message::Close(None)).await;
                break;
            }
        }
    }

    relay_handle.abort();
}

/// Build a JSON status frame for a given job status.
pub(crate) fn status_frame(
    run_id: &agentzero_core::RunId,
    status: &agentzero_core::JobStatus,
) -> String {
    match status {
        agentzero_core::JobStatus::Completed { result } => json!({
            "type": "completed",
            "run_id": run_id.0,
            "result": result,
        }),
        agentzero_core::JobStatus::Failed { error } => json!({
            "type": "failed",
            "run_id": run_id.0,
            "error": error,
        }),
        agentzero_core::JobStatus::Cancelled => json!({
            "type": "cancelled",
            "run_id": run_id.0,
        }),
        other => {
            let status_str = match other {
                agentzero_core::JobStatus::Pending => "pending",
                agentzero_core::JobStatus::Running => "running",
                _ => "unknown",
            };
            json!({
                "type": "status",
                "run_id": run_id.0,
                "status": status_str,
            })
        }
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// SSE run stream: GET /v1/runs/:run_id/stream
// ---------------------------------------------------------------------------

/// GET /v1/runs/:run_id/stream — Server-Sent Events stream for job status.
///
/// Alternative to WebSocket for environments that don't support WS.
/// Sends `text/event-stream` with block-level events when `?format=blocks`
/// is specified, otherwise raw status events.
pub(crate) async fn sse_run_stream(
    State(state): State<GatewayState>,
    mut headers: HeaderMap,
    Path(run_id_str): Path<String>,
    query: axum::extract::Query<WsRunQuery>,
) -> Result<Response, GatewayError> {
    if !headers.contains_key(axum::http::header::AUTHORIZATION) {
        if let Some(ref token) = query.token {
            if let Ok(val) = format!("Bearer {token}").parse() {
                headers.insert(axum::http::header::AUTHORIZATION, val);
            }
        }
    }
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let job_store = state
        .job_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?
        .clone();

    let run_id = agentzero_core::RunId(run_id_str.clone());

    // Verify the run exists.
    if job_store.get(&run_id).await.is_none() {
        return Err(GatewayError::NotFound {
            resource: format!("run {run_id_str}"),
        });
    }

    let use_blocks = query.format.as_deref() == Some("blocks");

    // Build an async stream that yields SSE events.
    let stream = async_stream::stream! {
        // Send current status immediately.
        if let Some(record) = job_store.get(&run_id).await {
            let frame = if use_blocks {
                block_status_frame(&run_id, &record.status)
            } else {
                status_frame(&run_id, &record.status)
            };
            yield Ok::<_, std::convert::Infallible>(
                format!("data: {frame}\n\n")
            );
            if record.status.is_terminal() {
                return;
            }
        }

        // Subscribe to status updates.
        let mut rx = job_store.subscribe();
        let deadline = Instant::now() + Duration::from_secs(600);

        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok((notified_run_id, new_status)) => {
                            if notified_run_id != run_id {
                                continue;
                            }
                            let frame = if use_blocks {
                                block_status_frame(&run_id, &new_status)
                            } else {
                                status_frame(&run_id, &new_status)
                            };
                            let terminal = new_status.is_terminal();
                            yield Ok(format!("data: {frame}\n\n"));
                            if terminal {
                                return;
                            }
                        }
                        Err(_) => return,
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    yield Ok(format!(
                        "data: {}\n\n",
                        json!({"type": "error", "message": "stream timeout"})
                    ));
                    return;
                }
            }
        }
    };

    let body = axum::body::Body::from_stream(stream);
    Ok(Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .body(body)
        .expect("valid SSE response builder")
        .into_response())
}

// ---------------------------------------------------------------------------

/// Validate that a channel name contains only safe characters.
/// Accepts 1–64 characters from `[a-zA-Z0-9_-]`.
pub(crate) fn is_valid_channel_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Build a block-level JSON frame for completed results.
///
/// For completed jobs, the result text is parsed through `BlockAccumulator`
/// to produce semantic blocks (paragraphs, code blocks, headers, list items).
/// Other statuses are forwarded as-is.
fn block_status_frame(
    run_id: &agentzero_core::RunId,
    status: &agentzero_core::JobStatus,
) -> String {
    match status {
        agentzero_core::JobStatus::Completed { result } => {
            let mut acc = agentzero_orchestrator::BlockAccumulator::new();
            acc.push(result);
            let blocks = acc.flush();
            let block_data: Vec<Value> = blocks
                .iter()
                .map(|b| match b {
                    agentzero_orchestrator::Block::Paragraph(text) => json!({
                        "type": "paragraph",
                        "content": text,
                    }),
                    agentzero_orchestrator::Block::CodeBlock { language, content } => json!({
                        "type": "code_block",
                        "language": language,
                        "content": content,
                    }),
                    agentzero_orchestrator::Block::Header { level, text } => json!({
                        "type": "header",
                        "level": level,
                        "content": text,
                    }),
                    agentzero_orchestrator::Block::ListItem(text) => json!({
                        "type": "list_item",
                        "content": text,
                    }),
                })
                .collect();
            json!({
                "type": "completed",
                "run_id": run_id.0,
                "format": "blocks",
                "blocks": block_data,
            })
            .to_string()
        }
        // For non-completed statuses, delegate to the raw frame builder.
        other => status_frame(run_id, other),
    }
}

// ---------------------------------------------------------------------------
// Event bus SSE endpoint
// ---------------------------------------------------------------------------

/// `GET /v1/events` — SSE stream of real-time events from the distributed event bus.
///
/// Subscribes to the event bus and streams events as SSE frames. Supports optional
/// `topic` query parameter to filter events by topic prefix.
pub(crate) async fn sse_events(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    query: axum::extract::Query<EventStreamQuery>,
) -> Result<Response, GatewayError> {
    // EventSource cannot set headers, so accept token as query param fallback.
    let effective_headers = if headers.get("authorization").is_none() {
        if let Some(ref token) = query.token {
            let mut h = headers.clone();
            if let Ok(val) = axum::http::HeaderValue::from_str(&format!("Bearer {token}")) {
                h.insert("authorization", val);
            }
            h
        } else {
            headers
        }
    } else {
        headers
    };
    authorize_with_scope(&state, &effective_headers, false, &Scope::RunsRead)?;

    let event_bus = state
        .event_bus
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?
        .clone();

    let topic_filter = query.topic.clone();

    let mut subscriber = event_bus.subscribe();

    let stream = async_stream::stream! {
        let deadline = Instant::now() + Duration::from_secs(600);

        loop {
            tokio::select! {
                result = subscriber.recv() => {
                    match result {
                        Ok(event) => {
                            // Filter by topic prefix if specified.
                            if let Some(ref prefix) = topic_filter {
                                if !event.topic.starts_with(prefix.as_str()) {
                                    continue;
                                }
                            }
                            let frame = json!({
                                "id": event.id,
                                "topic": event.topic,
                                "source": event.source,
                                "payload": event.payload,
                                "timestamp_ms": event.timestamp_ms,
                            });
                            yield Ok::<_, std::convert::Infallible>(
                                format!("data: {frame}\n\n")
                            );
                        }
                        Err(_) => return,
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    yield Ok(format!(
                        "data: {}\n\n",
                        json!({"type": "error", "message": "stream timeout"})
                    ));
                    return;
                }
            }
        }
    };

    let body = axum::body::Body::from_stream(stream);
    Ok(Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .body(body)
        .expect("SSE response builder"))
}

// ---------------------------------------------------------------------------
// OpenAPI spec endpoint
// ---------------------------------------------------------------------------

/// `GET /v1/openapi.json` — serves the auto-generated OpenAPI 3.1 specification.
pub(crate) async fn openapi_spec() -> Json<serde_json::Value> {
    Json(crate::openapi::build_openapi_spec())
}

// ---------------------------------------------------------------------------
// Webhook auto-registration helpers
// ---------------------------------------------------------------------------

/// Resolve the gateway's public URL from live config or environment variable.
pub(crate) fn resolve_public_url(state: &GatewayState) -> Option<String> {
    // Try live config first.
    if let Some(ref rx) = state.live_config {
        let url = rx.borrow().gateway.public_url.clone();
        if url.is_some() {
            return url;
        }
    }
    // Fall back to environment variable.
    std::env::var("AGENTZERO_PUBLIC_URL")
        .ok()
        .filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// Tools endpoint: GET /v1/tools
// ---------------------------------------------------------------------------

/// GET /v1/tools — list all available tools with metadata and JSON schema.
pub(crate) async fn get_tools(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<ToolsResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let policy =
        agentzero_infra::tools::ToolSecurityPolicy::default_for_workspace(std::env::temp_dir());
    let tools = agentzero_infra::tools::default_tools(&policy, None, None).unwrap_or_default();

    let summaries: Vec<ToolSummary> = tools
        .iter()
        .map(|t| ToolSummary {
            name: t.name().to_string(),
            description: t.description().to_string(),
            category: infer_tool_category(t.name()),
            input_schema: t.input_schema(),
        })
        .collect();

    let total = summaries.len();
    Ok(Json(ToolsResponse {
        object: "list",
        tools: summaries,
        total,
    }))
}

/// Infer the tool category from the tool name for UI grouping.
fn infer_tool_category(name: &str) -> String {
    let cat = if name.starts_with("read_file")
        || name.starts_with("write_file")
        || name.starts_with("glob_search")
        || name.starts_with("content_search")
        || name.starts_with("apply_patch")
        || name.starts_with("pdf_read")
        || name.starts_with("docx_read")
        || name == "file_edit"
    {
        "file"
    } else if name.starts_with("web_fetch")
        || name.starts_with("web_search")
        || name.starts_with("http_request")
        || name.starts_with("url_validation")
    {
        "web"
    } else if name.starts_with("shell")
        || name.starts_with("process")
        || name.starts_with("git_")
        || name == "code_interpreter"
    {
        "execution"
    } else if name.starts_with("memory_") {
        "memory"
    } else if name.starts_with("schedule") || name.starts_with("cron_") {
        "scheduling"
    } else if name.starts_with("delegate")
        || name.starts_with("sub_agent")
        || name.starts_with("task_plan")
        || name.starts_with("agent_")
    {
        "delegation"
    } else if name.starts_with("image_")
        || name.starts_with("screenshot")
        || name.starts_with("tts")
        || name.starts_with("video_")
    {
        "media"
    } else if name.starts_with("hardware_") {
        "hardware"
    } else {
        "other"
    };
    cat.to_string()
}

// ---------------------------------------------------------------------------
// Config endpoint: GET /v1/config
// ---------------------------------------------------------------------------

/// GET /v1/config — return current runtime configuration as structured sections.
pub(crate) async fn get_config(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<ConfigResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let cfg = match state.live_config {
        Some(ref rx) => rx.borrow().clone(),
        None => {
            return Err(GatewayError::NotFound {
                resource: "config".to_string(),
            })
        }
    };

    // Serialize the config to a JSON Value, then split into sections by top-level key.
    let json_val =
        serde_json::to_value(&cfg).unwrap_or(serde_json::Value::Object(Default::default()));
    let sections = if let serde_json::Value::Object(map) = json_val {
        map.into_iter()
            .map(|(key, value)| ConfigSection { key, value })
            .collect()
    } else {
        vec![]
    };

    Ok(Json(ConfigResponse { sections }))
}

// ---------------------------------------------------------------------------
// Memory endpoints: GET /v1/memory, POST /v1/memory/recall, POST /v1/memory/forget
// ---------------------------------------------------------------------------

/// GET /v1/memory — browse recent memory entries with optional text search.
pub(crate) async fn list_memory(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    query: axum::extract::Query<MemoryListQuery>,
) -> Result<Json<MemoryListResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let store = state
        .memory_store
        .as_ref()
        .ok_or(GatewayError::NotFound {
            resource: "memory_store".to_string(),
        })?
        .clone();

    let limit = query.limit.unwrap_or(100).min(1000);
    let entries = store.recent(limit).await.unwrap_or_default();

    // Client-side text search filter.
    let q = query.q.as_deref().unwrap_or("").to_lowercase();
    let filtered: Vec<_> = entries
        .into_iter()
        .filter(|e| {
            q.is_empty()
                || e.content.to_lowercase().contains(&q)
                || e.role.to_lowercase().contains(&q)
                || e.conversation_id.to_lowercase().contains(&q)
        })
        .collect();

    let total = filtered.len();
    let data = filtered
        .into_iter()
        .map(|e| MemoryListItem {
            role: e.role,
            content: e.content,
            conversation_id: e.conversation_id,
            agent_id: e.agent_id,
            created_at: e.created_at,
        })
        .collect();

    Ok(Json(MemoryListResponse {
        object: "list",
        data,
        total,
    }))
}

/// POST /v1/memory/recall — query memory by text similarity (currently a prefix search).
pub(crate) async fn recall_memory(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<MemoryRecallRequest>,
) -> Result<Json<MemoryListResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let store = state
        .memory_store
        .as_ref()
        .ok_or(GatewayError::NotFound {
            resource: "memory_store".to_string(),
        })?
        .clone();

    let limit = req.limit.unwrap_or(20).min(200);
    let q = req.query.to_lowercase();
    let entries = store.recent(limit * 10).await.unwrap_or_default();

    let matched: Vec<_> = entries
        .into_iter()
        .filter(|e| e.content.to_lowercase().contains(&q))
        .take(limit)
        .collect();

    let total = matched.len();
    let data = matched
        .into_iter()
        .map(|e| MemoryListItem {
            role: e.role,
            content: e.content,
            conversation_id: e.conversation_id,
            agent_id: e.agent_id,
            created_at: e.created_at,
        })
        .collect();

    Ok(Json(MemoryListResponse {
        object: "list",
        data,
        total,
    }))
}

/// POST /v1/memory/forget — forget (drop) recent memory entries matching filters.
///
/// This is a best-effort operation: the MemoryStore trait has no delete-by-filter
/// method, so this endpoint signals acceptance without removing stored entries.
/// Implementations that support deletion should override via a custom store.
pub(crate) async fn forget_memory(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(_req): Json<MemoryForgetRequest>,
) -> Result<Json<MemoryForgetResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    // Verify memory store is present.
    if state.memory_store.is_none() {
        return Err(GatewayError::NotFound {
            resource: "memory_store".to_string(),
        });
    }

    Ok(Json(MemoryForgetResponse {
        forgotten: true,
        message: "forget request accepted".to_string(),
    }))
}

// ---------------------------------------------------------------------------
// Approvals endpoint: GET /v1/approvals (stub)
// ---------------------------------------------------------------------------

/// GET /v1/approvals — list pending approval requests.
///
/// Currently a stub: returns an empty list. A full implementation would
/// require an approval queue to be wired into `GatewayState`.
pub(crate) async fn list_approvals(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<ApprovalsListResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    Ok(Json(ApprovalsListResponse {
        object: "list",
        data: vec![],
        total: 0,
    }))
}

/// Convert an `AgentChannelConfig` (from the agent store) to a
/// `ChannelInstanceConfig` (used by channel_setup) for building a temporary
/// channel instance.
pub(crate) fn agent_channel_to_instance_config(
    cfg: &agentzero_orchestrator::AgentChannelConfig,
) -> agentzero_channels::ChannelInstanceConfig {
    let mut instance = agentzero_channels::ChannelInstanceConfig {
        bot_token: cfg.bot_token.clone(),
        ..Default::default()
    };
    // Map well-known extra fields to their ChannelInstanceConfig counterparts.
    if let Some(v) = cfg.extra.get("access_token") {
        instance.access_token = Some(v.clone());
    }
    if let Some(v) = cfg.extra.get("channel_id") {
        instance.channel_id = Some(v.clone());
    }
    if let Some(v) = cfg.extra.get("app_token") {
        instance.app_token = Some(v.clone());
    }
    if let Some(v) = cfg.extra.get("webhook_url") {
        instance.base_url = Some(v.clone());
    }
    instance
}
