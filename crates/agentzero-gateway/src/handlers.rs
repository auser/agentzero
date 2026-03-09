use crate::auth::authorize_request;
use crate::models::{
    AsyncSubmitRequest, AsyncSubmitResponse, CancelQuery, ChatCompletionsRequest,
    ChatCompletionsResponse, ChatRequest, ChatResponse, CompletionChoice, CompletionChoiceMessage,
    GatewayError, HealthResponse, JobListQuery, JobStatusResponse, ModelItem, ModelsResponse,
    PairRequest, PairResponse, PingRequest, PingResponse, WebhookResponse, WsRunQuery,
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
    })
}

pub(crate) async fn api_chat(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, GatewayError> {
    authorize_request(&state, &headers, false)?;

    if let Some(reason) = check_perplexity(&req.message, &state.effective_perplexity_filter()) {
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
        conversation_id: None,
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
// Async job submission: /v1/runs (OpenClaw-style)
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
    authorize_request(&state, &headers, false)?;

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
    authorize_request(&state, &headers, false)?;

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
    authorize_request(&state, &headers, false)?;

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
) -> Result<Json<Value>, GatewayError> {
    authorize_request(&state, &headers, false)?;

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
        Ok(Json(json!({
            "run_id": run_id_str,
            "cancelled": !cancelled_ids.is_empty(),
            "cascade_count": cancelled_ids.len(),
            "cancelled_ids": cancelled_ids.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
        })))
    } else {
        let cancelled = job_store.cancel(&run_id).await;
        Ok(Json(json!({
            "run_id": run_id_str,
            "cancelled": cancelled,
        })))
    }
}

/// GET /v1/runs — list all jobs, optionally filtered by status query param.
pub(crate) async fn job_list(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    query: axum::extract::Query<JobListQuery>,
) -> Result<Json<Value>, GatewayError> {
    authorize_request(&state, &headers, false)?;

    let job_store = state
        .job_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;
    let jobs = job_store.list_all(query.status.as_deref()).await;

    let items: Vec<Value> = jobs
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
            json!({
                "run_id": r.run_id.0,
                "status": status_str,
                "agent_id": r.agent_id,
                "result": result,
                "error": error,
                "tokens_used": r.tokens_used,
                "cost_microdollars": r.cost_microdollars,
            })
        })
        .collect();

    Ok(Json(json!({
        "object": "list",
        "data": items,
        "total": items.len(),
    })))
}

/// GET /v1/runs/:run_id/events — stream job events as newline-delimited JSON.
///
/// Returns the status transitions for a job as a sequence of events.
/// If the job is still running, returns events so far.
pub(crate) async fn job_events(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(run_id_str): Path<String>,
) -> Result<Json<Value>, GatewayError> {
    authorize_request(&state, &headers, false)?;

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
    let events: Vec<Value> = log_events
        .iter()
        .map(|e| {
            use agentzero_orchestrator::EventKind;
            match &e.kind {
                EventKind::Created => json!({
                    "type": "created",
                    "run_id": run_id_str,
                }),
                EventKind::Running => json!({
                    "type": "running",
                    "run_id": run_id_str,
                }),
                EventKind::ToolCall { name } => json!({
                    "type": "tool_call",
                    "run_id": run_id_str,
                    "tool": name,
                }),
                EventKind::ToolResult { name } => json!({
                    "type": "tool_result",
                    "run_id": run_id_str,
                    "tool": name,
                }),
                EventKind::Completed { summary } => json!({
                    "type": "completed",
                    "run_id": run_id_str,
                    "result": summary,
                }),
                EventKind::Failed { error } => json!({
                    "type": "failed",
                    "run_id": run_id_str,
                    "error": error,
                }),
                EventKind::Cancelled => json!({
                    "type": "cancelled",
                    "run_id": run_id_str,
                }),
            }
        })
        .collect();

    Ok(Json(json!({
        "object": "list",
        "run_id": run_id_str,
        "events": events,
        "total": events.len(),
    })))
}

// ---------------------------------------------------------------------------
// WebSocket run subscription: /ws/runs/:run_id
/// GET /v1/agents — list all registered agents with their presence status.
pub(crate) async fn agents_list(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<Value>, GatewayError> {
    authorize_request(&state, &headers, false)?;

    let presence = state
        .presence_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;

    let records = presence.list_all().await;
    let data: Vec<Value> = records
        .iter()
        .map(|r| {
            let status_str = match r.status {
                agentzero_orchestrator::PresenceStatus::Alive => "alive",
                agentzero_orchestrator::PresenceStatus::Stale => "stale",
                agentzero_orchestrator::PresenceStatus::Dead => "dead",
            };
            json!({
                "agent_id": r.agent_id,
                "status": status_str,
                "ttl_secs": r.ttl.as_secs(),
            })
        })
        .collect();

    Ok(Json(json!({
        "object": "list",
        "data": data,
        "total": data.len(),
    })))
}

/// POST /v1/estop — emergency stop: cascade-cancel all active root-level runs.
///
/// Returns the list of cancelled run IDs and the total count.
pub(crate) async fn emergency_stop(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<Value>, GatewayError> {
    authorize_request(&state, &headers, false)?;

    let job_store = state
        .job_store
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?;

    let cancelled_ids = job_store.emergency_stop_all().await;
    let count = cancelled_ids.len();

    tracing::warn!(
        cancelled_count = count,
        "emergency stop triggered — cancelled all active runs"
    );

    Ok(Json(json!({
        "emergency_stop": true,
        "cancelled_count": count,
        "cancelled_ids": cancelled_ids.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
    })))
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
    headers: HeaderMap,
    Path(run_id_str): Path<String>,
    query: axum::extract::Query<WsRunQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, GatewayError> {
    authorize_request(&state, &headers, false)?;

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
    headers: HeaderMap,
    Path(run_id_str): Path<String>,
    query: axum::extract::Query<WsRunQuery>,
) -> Result<Response, GatewayError> {
    authorize_request(&state, &headers, false)?;

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
        .unwrap()
        .into_response())
}

// ---------------------------------------------------------------------------

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
