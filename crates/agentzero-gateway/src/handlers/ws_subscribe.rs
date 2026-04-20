use super::sse::block_status_frame;
use super::*;
use crate::models::WsRunQuery;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use futures_util::StreamExt;
use std::time::Duration;
use tokio::time::Instant;

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
