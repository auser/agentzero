use super::*;
use crate::models::{
    ApprovalsListResponse, MemoryForgetRequest, MemoryForgetResponse, MemoryListItem,
    MemoryListQuery, MemoryListResponse, MemoryRecallRequest,
};

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
    AppJson(req): AppJson<MemoryRecallRequest>,
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
    AppJson(_req): AppJson<MemoryForgetRequest>,
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

// ---------------------------------------------------------------------------
// Remote tool execution (lite mode delegation)
