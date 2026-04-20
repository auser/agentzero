use super::*;
use crate::models::{CreateCronRequest, CronJobResponse, CronListResponse, UpdateCronRequest};
use crate::util::now_epoch_secs;

fn cron_store_from_state(
    state: &GatewayState,
) -> Result<agentzero_tools::cron_store::CronStore, GatewayError> {
    let data_dir = state
        .workspace_root
        .as_ref()
        .map(|p| p.as_ref().join(".agentzero"))
        .ok_or(GatewayError::NotFound {
            resource: "cron data directory (no workspace_root configured)".to_string(),
        })?;
    agentzero_tools::cron_store::CronStore::new(&data_dir).map_err(|e| {
        GatewayError::AgentExecutionFailed {
            message: format!("failed to open cron store: {e}"),
        }
    })
}

fn task_to_response(task: &agentzero_tools::cron_store::CronTask) -> CronJobResponse {
    CronJobResponse {
        id: task.id.clone(),
        schedule: task.schedule.clone(),
        message: task.command.clone(),
        agent_id: None,
        enabled: task.enabled,
    }
}

/// GET /v1/cron — list all scheduled cron jobs.
pub(crate) async fn list_cron(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<CronListResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;
    let store = cron_store_from_state(&state)?;
    let tasks = store
        .list()
        .map_err(|e| GatewayError::AgentExecutionFailed {
            message: format!("failed to list cron tasks: {e}"),
        })?;
    let jobs = tasks.iter().map(task_to_response).collect();
    Ok(Json(CronListResponse { jobs }))
}

/// POST /v1/cron — create a new cron job.
pub(crate) async fn create_cron(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(req): AppJson<CreateCronRequest>,
) -> Result<(axum::http::StatusCode, Json<CronJobResponse>), GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;
    let store = cron_store_from_state(&state)?;
    let id = format!("cron_{:x}", now_epoch_secs());
    let task =
        store
            .add(&id, &req.schedule, &req.message)
            .map_err(|e| GatewayError::BadRequest {
                message: e.to_string(),
            })?;
    let mut resp = task_to_response(&task);
    resp.agent_id = req.agent_id;
    Ok((axum::http::StatusCode::CREATED, Json(resp)))
}

/// PATCH /v1/cron/:id — update a cron job (toggle enabled, change schedule/message).
pub(crate) async fn update_cron(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    AppJson(req): AppJson<UpdateCronRequest>,
) -> Result<Json<CronJobResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;
    let store = cron_store_from_state(&state)?;

    // Handle enable/disable toggle
    if let Some(enabled) = req.enabled {
        let task = if enabled {
            store.resume(&id)
        } else {
            store.pause(&id)
        }
        .map_err(|e| GatewayError::NotFound {
            resource: format!("cron job {id}: {e}"),
        })?;
        return Ok(Json(task_to_response(&task)));
    }

    // Handle schedule/message update
    let task = store
        .update(&id, req.schedule.as_deref(), req.message.as_deref())
        .map_err(|e| GatewayError::NotFound {
            resource: format!("cron job {id}: {e}"),
        })?;
    Ok(Json(task_to_response(&task)))
}

/// DELETE /v1/cron/:id — remove a cron job.
pub(crate) async fn delete_cron(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;
    let store = cron_store_from_state(&state)?;
    store.remove(&id).map_err(|e| GatewayError::NotFound {
        resource: format!("cron job {id}: {e}"),
    })?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
