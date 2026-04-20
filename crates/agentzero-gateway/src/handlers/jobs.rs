use super::*;
use crate::models::{
    AsyncSubmitRequest, AsyncSubmitResponse, CancelQuery, CancelResponse, EventItem,
    EventListResponse, EventsQuery, JobListItem, JobListQuery, JobListResponse, JobStatusResponse,
    TranscriptResponse,
};
use agentzero_channels::pipeline::check_perplexity;
use agentzero_infra::runtime::{run_agent_once, RunAgentRequest};

/// Spawn a background task that runs an agent request and tracks its status in
/// the job store. Used by all `async_submit` modes except `collect`.
fn spawn_agent_run(
    store: Arc<agentzero_orchestrator::JobStore>,
    run_id: agentzero_core::RunId,
    agent_req: RunAgentRequest,
) {
    tokio::spawn(async move {
        store
            .update_status(&run_id, agentzero_core::JobStatus::Running)
            .await;
        match run_agent_once(agent_req).await {
            Ok(output) => {
                store
                    .update_status(
                        &run_id,
                        agentzero_core::JobStatus::Completed {
                            result: output.response_text,
                        },
                    )
                    .await;
            }
            Err(e) => {
                store
                    .update_status(
                        &run_id,
                        agentzero_core::JobStatus::Failed {
                            error: e.to_string(),
                        },
                    )
                    .await;
            }
        }
    });
}

/// Build an accepted response for an async job submission.
fn accepted_response(run_id: &agentzero_core::RunId) -> Response {
    let resp = AsyncSubmitResponse {
        run_id: run_id.0.clone(),
        accepted_at: chrono_now_iso(),
    };
    (axum::http::StatusCode::ACCEPTED, Json(resp)).into_response()
}

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
    AppJson(req): AppJson<AsyncSubmitRequest>,
) -> Result<Response, GatewayError> {
    let identity = authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    let job_store = state.require_job_store()?;

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
            let existing_run_id = req.run_id.as_deref().ok_or(GatewayError::BadRequest {
                message: "followup mode requires a `run_id` field".to_string(),
            })?;
            let target_run_id = agentzero_core::RunId(existing_run_id.to_string());

            if job_store.get(&target_run_id).await.is_none() {
                return Err(GatewayError::NotFound {
                    resource: format!("run {existing_run_id}"),
                });
            }

            let run_id = job_store.submit("agent".to_string(), lane, None).await;
            let mut agent_req = build_agent_request(
                &state,
                req.message,
                req.model,
                identity.capability_ceiling.clone(),
            )?;
            agent_req.conversation_id = Some(existing_run_id.to_string());
            spawn_agent_run(job_store.clone(), run_id.clone(), agent_req);
            Ok(accepted_response(&run_id))
        }

        "collect" => {
            let run_id = job_store.submit("agent".to_string(), lane, None).await;
            let message = req.message.clone();
            let model = req.model.clone();
            let state_clone = state.clone();
            let store = job_store.clone();
            let rid = run_id.clone();
            let collect_count = 3usize;
            let cap_ceiling = identity.capability_ceiling.clone();

            tokio::spawn(async move {
                store
                    .update_status(&rid, agentzero_core::JobStatus::Running)
                    .await;

                let mut handles = Vec::with_capacity(collect_count);
                for _ in 0..collect_count {
                    let msg = message.clone();
                    let mdl = model.clone();
                    let st = state_clone.clone();
                    let cap = cap_ceiling.clone();
                    handles.push(tokio::spawn(async move {
                        let req = match build_agent_request(&st, msg, mdl, cap) {
                            Ok(r) => r,
                            Err(e) => return Err(anyhow::anyhow!("{e:?}")),
                        };
                        run_agent_once(req).await
                    }));
                }

                let mut results = Vec::with_capacity(collect_count);
                for handle in handles {
                    match handle.await {
                        Ok(Ok(output)) => results.push(output.response_text),
                        Ok(Err(e)) => results.push(format!("[error] {e}")),
                        Err(e) => results.push(format!("[join error] {e}")),
                    }
                }

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

            Ok(accepted_response(&run_id))
        }

        "interrupt" => {
            let active_runs = job_store.list_all(None).await;
            for job in &active_runs {
                if !job.status.is_terminal() {
                    job_store
                        .update_status(&job.run_id, agentzero_core::JobStatus::Cancelled)
                        .await;
                }
            }

            let run_id = job_store.submit("agent".to_string(), lane, None).await;
            let agent_req = build_agent_request(
                &state,
                req.message,
                req.model,
                identity.capability_ceiling.clone(),
            )?;
            spawn_agent_run(job_store.clone(), run_id.clone(), agent_req);
            Ok(accepted_response(&run_id))
        }

        _ => {
            let run_id = job_store.submit("agent".to_string(), lane, None).await;
            let agent_req = build_agent_request(
                &state,
                req.message,
                req.model,
                identity.capability_ceiling.clone(),
            )?;
            spawn_agent_run(job_store.clone(), run_id.clone(), agent_req);
            Ok(accepted_response(&run_id))
        }
    }
}

/// Decompose a `JobStatus` into `(status_str, result, error)` for JSON responses.
fn job_status_fields(
    status: &agentzero_core::JobStatus,
) -> (&'static str, Option<String>, Option<String>) {
    match status {
        agentzero_core::JobStatus::Pending => ("pending", None, None),
        agentzero_core::JobStatus::Running => ("running", None, None),
        agentzero_core::JobStatus::Completed { result } => {
            ("completed", Some(result.clone()), None)
        }
        agentzero_core::JobStatus::Failed { error } => ("failed", None, Some(error.clone())),
        agentzero_core::JobStatus::Cancelled => ("cancelled", None, None),
    }
}

/// GET /v1/runs/:run_id — poll job status.
pub(crate) async fn job_status(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(run_id_str): Path<String>,
) -> Result<Json<JobStatusResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let job_store = state.require_job_store()?;

    let run_id = agentzero_core::RunId(run_id_str.clone());
    let record = job_store.get(&run_id).await.ok_or(GatewayError::NotFound {
        resource: format!("run {run_id_str}"),
    })?;

    let (status_str, result, error) = job_status_fields(&record.status);

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

    let job_store = state.require_job_store()?;

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

    let job_store = state.require_job_store()?;
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

    let job_store = state.require_job_store()?;
    let jobs = job_store.list_all(query.status.as_deref()).await;

    let items: Vec<JobListItem> = jobs
        .iter()
        .map(|r| {
            let (status_str, result, error) = job_status_fields(&r.status);
            let depth = match &r.lane {
                agentzero_core::Lane::SubAgent { depth, .. } => *depth,
                _ => 0,
            };
            JobListItem {
                run_id: r.run_id.0.clone(),
                status: status_str,
                agent_id: r.agent_id.clone(),
                parent_run_id: r.parent_run_id.as_ref().map(|id| id.0.clone()),
                depth,
                result,
                error,
                tokens_used: r.tokens_used,
                cost_microdollars: r.cost_microdollars,
                created_at_epoch_ms: r.created_at_epoch_ms,
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
    Query(query): Query<EventsQuery>,
) -> Result<Json<EventListResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let job_store = state.require_job_store()?;
    let run_id = agentzero_core::RunId(run_id_str.clone());

    if job_store.get(&run_id).await.is_none() {
        return Err(GatewayError::NotFound {
            resource: format!("run {run_id_str}"),
        });
    }

    // Use the persistent event log instead of reconstructing from state.
    let log_events = job_store.get_events(&run_id).await;
    let since_seq = query.since_seq.unwrap_or(0);
    let events: Vec<EventItem> = log_events
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let seq = i + 1; // 1-based sequence numbers
            use agentzero_orchestrator::EventKind;
            match &e.kind {
                EventKind::Created => EventItem {
                    seq,
                    event_type: "created",
                    run_id: run_id_str.clone(),
                    tool: None,
                    result: None,
                    error: None,
                },
                EventKind::Running => EventItem {
                    seq,
                    event_type: "running",
                    run_id: run_id_str.clone(),
                    tool: None,
                    result: None,
                    error: None,
                },
                EventKind::ToolCall { name } => EventItem {
                    seq,
                    event_type: "tool_call",
                    run_id: run_id_str.clone(),
                    tool: Some(name.clone()),
                    result: None,
                    error: None,
                },
                EventKind::ToolResult { name } => EventItem {
                    seq,
                    event_type: "tool_result",
                    run_id: run_id_str.clone(),
                    tool: Some(name.clone()),
                    result: None,
                    error: None,
                },
                EventKind::Completed { summary } => EventItem {
                    seq,
                    event_type: "completed",
                    run_id: run_id_str.clone(),
                    tool: None,
                    result: Some(summary.clone()),
                    error: None,
                },
                EventKind::Failed { error } => EventItem {
                    seq,
                    event_type: "failed",
                    run_id: run_id_str.clone(),
                    tool: None,
                    result: None,
                    error: Some(error.clone()),
                },
                EventKind::Cancelled => EventItem {
                    seq,
                    event_type: "cancelled",
                    run_id: run_id_str.clone(),
                    tool: None,
                    result: None,
                    error: None,
                },
            }
        })
        .filter(|e| e.seq > since_seq)
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
