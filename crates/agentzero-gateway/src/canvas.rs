//! Canvas REST and WebSocket handlers for the A2UI Live Canvas feature.
//!
//! Provides endpoints for listing, reading, writing, and streaming canvas content.

use crate::state::GatewayState;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// `GET /api/canvas` — list all canvases.
pub(crate) async fn list_canvases(State(state): State<GatewayState>) -> impl IntoResponse {
    let store = match &state.canvas_store {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "canvas store not configured"})),
            )
                .into_response();
        }
    };
    let summaries = store.list().await;
    (StatusCode::OK, Json(json!(summaries))).into_response()
}

/// `GET /api/canvas/:id` — get current snapshot of a canvas.
pub(crate) async fn get_canvas(
    State(state): State<GatewayState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let store = match &state.canvas_store {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "canvas store not configured"})),
            )
                .into_response();
        }
    };
    match store.snapshot(&id).await {
        Some(canvas) => (StatusCode::OK, Json(json!(canvas))).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("canvas '{id}' not found")})),
        )
            .into_response(),
    }
}

/// `POST /api/canvas/:id` — render content to a canvas.
///
/// Body: `{ "content_type": "text/html", "content": "<h1>Hello</h1>" }`
pub(crate) async fn post_canvas(
    State(state): State<GatewayState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let store = match &state.canvas_store {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "canvas store not configured"})),
            )
                .into_response();
        }
    };

    let content_type = match body.get("content_type").and_then(|v| v.as_str()) {
        Some(ct) => ct,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "missing 'content_type' field"})),
            )
                .into_response();
        }
    };

    let content = match body.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "missing 'content' field"})),
            )
                .into_response();
        }
    };

    match store.render(&id, content_type, content).await {
        Ok(()) => (StatusCode::OK, Json(json!({"ok": true, "canvas_id": id}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// `DELETE /api/canvas/:id` — clear a canvas.
pub(crate) async fn delete_canvas(
    State(state): State<GatewayState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let store = match &state.canvas_store {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "canvas store not configured"})),
            )
                .into_response();
        }
    };

    if store.clear(&id).await {
        (
            StatusCode::OK,
            Json(json!({"ok": true, "canvas_id": id, "cleared": true})),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("canvas '{id}' not found")})),
        )
            .into_response()
    }
}

/// `GET /api/canvas/:id/history` — get frame history for a canvas.
pub(crate) async fn canvas_history(
    State(state): State<GatewayState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let store = match &state.canvas_store {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "canvas store not configured"})),
            )
                .into_response();
        }
    };

    match store.history(&id).await {
        Some(history) => {
            let frames: Vec<_> = history.into_iter().collect();
            (StatusCode::OK, Json(json!(frames))).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("canvas '{id}' not found")})),
        )
            .into_response(),
    }
}

/// `GET /ws/canvas/:id` — WebSocket for real-time canvas updates.
pub(crate) async fn ws_canvas(
    State(state): State<GatewayState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_canvas_socket(socket, state, id))
}

async fn handle_canvas_socket(mut socket: WebSocket, state: GatewayState, canvas_id: String) {
    let store = match &state.canvas_store {
        Some(s) => s,
        None => {
            let _ = socket
                .send(Message::Text(
                    json!({"type": "error", "message": "canvas store not configured"}).to_string(),
                ))
                .await;
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    };

    // Send current snapshot on connect.
    if let Some(canvas) = store.snapshot(&canvas_id).await {
        let frame = json!({
            "type": "snapshot",
            "canvas_id": canvas_id,
            "canvas": canvas,
        });
        if socket.send(Message::Text(frame.to_string())).await.is_err() {
            return;
        }
    }

    // Subscribe to updates.
    let mut rx = store.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok((id, frame)) => {
                        if id != canvas_id {
                            continue;
                        }
                        let msg = json!({
                            "type": "frame",
                            "canvas_id": id,
                            "frame": frame,
                        });
                        if socket.send(Message::Text(msg.to_string())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                        tracing::warn!(canvas_id = %canvas_id, lagged = count, "canvas ws subscriber lagged");
                        // Continue receiving — we just missed some frames.
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
            msg = futures_util::StreamExt::next(&mut socket) => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    // Ignore other client messages (ping/pong handled by axum).
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}
