use super::*;
use crate::models::EventStreamQuery;
use crate::models::WsRunQuery;
use std::time::Duration;
use tokio::time::Instant;

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
pub(super) fn block_status_frame(
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
