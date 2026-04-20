use super::*;
use agentzero_infra::runtime::RunAgentRequest;
use agentzero_infra::runtime::{build_runtime_execution, run_agent_streaming};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use futures_util::StreamExt;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::{interval, Instant};

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
    let agent_store = state
        .agent_store
        .as_ref()
        .map(|s| Arc::clone(s) as Arc<dyn agentzero_core::agent_store::AgentStoreApi>);
    let ws_cfg = state.ws_config.clone();
    crate::gateway_metrics::record_ws_connection();
    Ok(ws
        .max_message_size(ws_cfg.max_message_bytes)
        .on_upgrade(move |socket| {
            handle_socket(socket, config_path, workspace_root, agent_store, ws_cfg)
        })
        .into_response())
}

async fn handle_socket(
    mut socket: WebSocket,
    config_path: Arc<PathBuf>,
    workspace_root: Arc<PathBuf>,
    agent_store: Option<Arc<dyn agentzero_core::agent_store::AgentStoreApi>>,
    ws_cfg: agentzero_config::WebSocketConfig,
) {
    let mut heartbeat = interval(Duration::from_secs(ws_cfg.heartbeat_interval_secs));
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
                            &agent_store,
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
                if last_pong.elapsed() > Duration::from_secs(ws_cfg.pong_timeout_secs) {
                    tracing::warn!("WebSocket pong timeout, closing connection");
                    let _ = socket.send(Message::Close(None)).await;
                    break;
                }
                // Check idle timeout.
                if last_activity.elapsed() > Duration::from_secs(ws_cfg.idle_timeout_secs) {
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
///
/// Accepts either plain text (backward compat) or JSON:
/// ```json
/// { "message": "hello", "provider": "builtin", "model": "qwen2.5-coder-3b", "agent_id": "..." }
/// ```
/// When `provider` is set, it overrides the config file's provider (e.g., "builtin" for local model).
async fn handle_text_message(
    socket: &mut WebSocket,
    config_path: &Arc<PathBuf>,
    workspace_root: &Arc<PathBuf>,
    agent_store: &Option<Arc<dyn agentzero_core::agent_store::AgentStoreApi>>,
    text: String,
) {
    // Try to parse as JSON for provider/model override support.
    // Falls back to treating the entire string as the message.
    let (message, provider_override, model_override) =
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
            let msg = parsed["message"].as_str().unwrap_or(&text).to_string();
            let provider = parsed["provider"].as_str().map(String::from);
            let model = parsed["model"].as_str().map(String::from);
            (msg, provider, model)
        } else {
            (text.clone(), None, None)
        };

    let req = RunAgentRequest {
        workspace_root: workspace_root.as_ref().clone(),
        config_path: config_path.as_ref().clone(),
        message: message.clone(),
        provider_override,
        model_override,
        profile_override: None,
        extra_tools: vec![],
        conversation_id: None,
        agent_store: agent_store.clone(),
        memory_override: None,
        memory_window_override: None,
        capability_set_override: agentzero_core::security::CapabilitySet::default(),
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

    // Inject subsystem awareness: append a hint to the system prompt so the
    // chat agent knows it can manage agents, schedules, config, memory, etc.
    let mut execution = execution;
    let subsystem_hint = concat!(
        "\n\nYou have access to AgentZero platform management tools. ",
        "You can create/list/update/delete persistent agents (agent_manage), ",
        "manage cron schedules (cron_add/list/remove), ",
        "store and recall memories (memory_store/recall/forget), ",
        "read and update system configuration (config_manage), ",
        "and create custom tools at runtime (tool_create). ",
        "When the user asks you to set up agents, schedules, or configure the system, ",
        "use these tools directly.",
    );
    match execution.config.system_prompt {
        Some(ref mut prompt) if !prompt.contains("agent_manage") => {
            prompt.push_str(subsystem_hint);
        }
        None => {
            execution.config.system_prompt =
                Some(format!("You are a helpful AI assistant.{subsystem_hint}"));
        }
        _ => {}
    }

    let (mut rx, handle) = run_agent_streaming(execution, workspace_root.as_ref().clone(), message);
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
