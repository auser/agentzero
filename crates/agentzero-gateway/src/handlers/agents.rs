use super::*;
use crate::models::{
    AgentDetailResponse, AgentListResponse, AgentStatsResponse, CreateAgentRequest,
    CreateAgentResponse, TopologyEdge, TopologyNode, TopologyResponse, UpdateAgentRequest,
};

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
// Agent stats & topology
// ---------------------------------------------------------------------------

/// GET /v1/agents/:agent_id/stats — per-agent aggregated metrics.
pub(crate) async fn agent_stats(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    axum::extract::Path(agent_id): axum::extract::Path<String>,
) -> Result<Json<AgentStatsResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let job_store = state.require_job_store()?;

    let jobs = job_store.list_by_agent(&agent_id).await;
    let tool_usage = job_store.agent_tool_frequency(&agent_id).await;

    let mut running_count = 0usize;
    let mut completed_count = 0usize;
    let mut failed_count = 0usize;
    let mut total_cost: u64 = 0;
    let mut total_tokens: u64 = 0;

    for job in &jobs {
        match &job.status {
            agentzero_core::JobStatus::Running => running_count += 1,
            agentzero_core::JobStatus::Completed { .. } => completed_count += 1,
            agentzero_core::JobStatus::Failed { .. } => failed_count += 1,
            _ => {}
        }
        total_cost = total_cost.saturating_add(job.cost_microdollars);
        total_tokens = total_tokens.saturating_add(job.tokens_used);
    }

    Ok(Json(AgentStatsResponse {
        agent_id,
        total_runs: jobs.len(),
        running_count,
        completed_count,
        failed_count,
        total_cost_microdollars: total_cost,
        total_tokens_used: total_tokens,
        tool_usage,
    }))
}

/// GET /v1/topology — live agent topology snapshot.
pub(crate) async fn topology(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<TopologyResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let job_store = state.require_job_store()?;

    // Collect all agents from both stores.
    let mut agent_map: std::collections::HashMap<String, TopologyNode> =
        std::collections::HashMap::new();

    if let Some(store) = &state.agent_store {
        for record in store.list() {
            agent_map.insert(
                record.agent_id.clone(),
                TopologyNode {
                    agent_id: record.agent_id.clone(),
                    name: record.name.clone(),
                    status: match record.status {
                        agentzero_orchestrator::AgentStatus::Active => "active",
                        agentzero_orchestrator::AgentStatus::Stopped => "stopped",
                    }
                    .to_string(),
                    active_run_count: 0,
                    total_cost_microdollars: 0,
                },
            );
        }
    }

    if let Some(presence) = &state.presence_store {
        for r in presence.list_all().await {
            agent_map.entry(r.agent_id.clone()).or_insert_with(|| {
                let status = match r.status {
                    agentzero_orchestrator::PresenceStatus::Alive => "active",
                    agentzero_orchestrator::PresenceStatus::Stale => "stale",
                    agentzero_orchestrator::PresenceStatus::Dead => "stopped",
                };
                TopologyNode {
                    agent_id: r.agent_id.clone(),
                    name: r.agent_id.clone(),
                    status: status.to_string(),
                    active_run_count: 0,
                    total_cost_microdollars: 0,
                }
            });
        }
    }

    // Build edges from running jobs with parent_run_id.
    let all_jobs = job_store.list_all(None).await;
    let mut edges = Vec::new();

    // Index jobs by run_id for parent lookups.
    let job_index: std::collections::HashMap<&str, &agentzero_orchestrator::JobRecord> =
        all_jobs.iter().map(|j| (j.run_id.0.as_str(), j)).collect();

    for job in &all_jobs {
        // Update node stats.
        if let Some(node) = agent_map.get_mut(&job.agent_id) {
            if matches!(job.status, agentzero_core::JobStatus::Running) {
                node.active_run_count += 1;
                node.status = "running".to_string();
            }
            node.total_cost_microdollars = node
                .total_cost_microdollars
                .saturating_add(job.cost_microdollars);
        }

        // Build delegation edges from parent→child.
        if let Some(parent_id) = &job.parent_run_id {
            if let Some(parent_job) = job_index.get(parent_id.0.as_str()) {
                if parent_job.agent_id != job.agent_id {
                    edges.push(TopologyEdge {
                        from_agent_id: parent_job.agent_id.clone(),
                        to_agent_id: job.agent_id.clone(),
                        run_id: job.run_id.0.clone(),
                        edge_type: "delegation",
                    });
                }
            }
        }
    }

    let nodes: Vec<TopologyNode> = agent_map.into_values().collect();

    Ok(Json(TopologyResponse { nodes, edges }))
}

// ---------------------------------------------------------------------------
// Agent management CRUD
// ---------------------------------------------------------------------------

/// POST /v1/agents — create a dynamic agent at runtime.
pub(crate) async fn create_agent(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(req): AppJson<CreateAgentRequest>,
) -> Result<(axum::http::StatusCode, Json<CreateAgentResponse>), GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let agent_store = state.require_agent_store()?;

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
    AppJson(req): AppJson<UpdateAgentRequest>,
) -> Result<Json<AgentDetailResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let agent_store = state.require_agent_store()?;

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

    let agent_store = state.require_agent_store()?;

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
