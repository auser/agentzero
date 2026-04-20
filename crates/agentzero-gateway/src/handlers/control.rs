use super::*;
use crate::models::{CodegenControlResponse, EstopResponse};

// ---------------------------------------------------------------------------

/// POST /v1/estop — emergency stop: cascade-cancel all active root-level runs.
///
/// Returns the list of cancelled run IDs and the total count.
pub(crate) async fn emergency_stop(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<EstopResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let job_store = state.require_job_store()?;

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

/// POST /v1/runtime/codegen-disable — disable the codegen dynamic tool
/// strategy for this runtime process. Admin-scoped.
///
/// This flips a process-global `AtomicBool` that `create_codegen_tool()`
/// checks before calling the LLM or the compiler. No restart required.
/// To re-enable, call `POST /v1/runtime/codegen-enable` or set
/// `codegen_enabled = true` in `agentzero.toml` and reload the config.
pub(crate) async fn runtime_codegen_disable(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<CodegenControlResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    agentzero_infra::tools::tool_create::set_codegen_enabled(false);

    crate::audit::audit(
        crate::audit::AuditEvent::AdminAction,
        "codegen disabled via gateway",
        "",
        "/v1/runtime/codegen-disable",
    );

    Ok(Json(CodegenControlResponse {
        codegen_enabled: false,
    }))
}

/// POST /v1/runtime/codegen-enable — re-enable the codegen dynamic tool
/// strategy for this runtime process. Admin-scoped.
pub(crate) async fn runtime_codegen_enable(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<CodegenControlResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    agentzero_infra::tools::tool_create::set_codegen_enabled(true);

    crate::audit::audit(
        crate::audit::AuditEvent::AdminAction,
        "codegen enabled via gateway",
        "",
        "/v1/runtime/codegen-enable",
    );

    Ok(Json(CodegenControlResponse {
        codegen_enabled: true,
    }))
}
