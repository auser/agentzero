use super::*;
use axum::body::Body;

/// POST /v1/workflows — create a new workflow definition.
pub(crate) async fn create_workflow(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(req): AppJson<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let store = state.require_workflow_store()?;

    let name = req["name"]
        .as_str()
        .unwrap_or("Untitled Workflow")
        .to_string();
    let description = req["description"].as_str().unwrap_or("").to_string();
    // Support both `{ layout: { nodes, edges } }` (frontend) and top-level `{ nodes, edges }`.
    let layout = &req["layout"];
    let nodes: Vec<Value> = layout["nodes"]
        .as_array()
        .or_else(|| req["nodes"].as_array())
        .cloned()
        .unwrap_or_default();
    let edges: Vec<Value> = layout["edges"]
        .as_array()
        .or_else(|| req["edges"].as_array())
        .cloned()
        .unwrap_or_default();

    let record = agentzero_orchestrator::WorkflowRecord {
        workflow_id: String::new(),
        name,
        description,
        nodes,
        edges,
        created_at: 0,
        updated_at: 0,
    };

    let created = store.create(record).map_err(|e| GatewayError::BadRequest {
        message: e.to_string(),
    })?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "workflow_id": created.workflow_id,
            "name": created.name,
            "description": created.description,
            "nodes": created.nodes,
            "edges": created.edges,
            "created_at": created.created_at,
            "updated_at": created.updated_at,
        })),
    ))
}

/// GET /v1/workflows — list all workflow definitions.
pub(crate) async fn list_workflows(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let store = state.require_workflow_store()?;

    let include_layout = params.get("include").is_some_and(|v| v == "layout");

    let workflows: Vec<Value> = store
        .list()
        .into_iter()
        .map(|w| {
            let mut entry = json!({
                "workflow_id": w.workflow_id,
                "name": w.name,
                "description": w.description,
                "node_count": w.nodes.len(),
                "edge_count": w.edges.len(),
                "created_at": w.created_at,
                "updated_at": w.updated_at,
            });
            if include_layout {
                entry["layout"] = json!({
                    "nodes": w.nodes,
                    "edges": w.edges,
                });
            }
            entry
        })
        .collect();

    let total = workflows.len();
    Ok(Json(json!({
        "object": "list",
        "data": workflows,
        "total": total,
    })))
}

/// GET /v1/workflows/:id — get a workflow definition.
pub(crate) async fn get_workflow(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let store = state.require_workflow_store()?;

    let workflow = store.get(&id).ok_or(GatewayError::NotFound {
        resource: format!("workflow/{id}"),
    })?;

    Ok(Json(json!({
        "workflow_id": workflow.workflow_id,
        "name": workflow.name,
        "description": workflow.description,
        "nodes": workflow.nodes,
        "edges": workflow.edges,
        "layout": {
            "nodes": workflow.nodes,
            "edges": workflow.edges,
        },
        "created_at": workflow.created_at,
        "updated_at": workflow.updated_at,
    })))
}

/// PATCH /v1/workflows/:id — update a workflow definition.
pub(crate) async fn update_workflow(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    AppJson(req): AppJson<Value>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let store = state.require_workflow_store()?;

    let layout = &req["layout"];
    let update = agentzero_orchestrator::WorkflowUpdate {
        name: req["name"].as_str().map(String::from),
        description: req["description"].as_str().map(String::from),
        nodes: layout["nodes"]
            .as_array()
            .or_else(|| req["nodes"].as_array())
            .cloned(),
        edges: layout["edges"]
            .as_array()
            .or_else(|| req["edges"].as_array())
            .cloned(),
    };

    let updated = store
        .update(&id, update)
        .map_err(|e| GatewayError::BadRequest {
            message: e.to_string(),
        })?
        .ok_or(GatewayError::NotFound {
            resource: format!("workflow/{id}"),
        })?;

    Ok(Json(json!({
        "workflow_id": updated.workflow_id,
        "name": updated.name,
        "description": updated.description,
        "nodes": updated.nodes,
        "edges": updated.edges,
        "created_at": updated.created_at,
        "updated_at": updated.updated_at,
    })))
}

/// DELETE /v1/workflows/:id — delete a workflow definition.
pub(crate) async fn delete_workflow(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let store = state.require_workflow_store()?;

    let removed = store.delete(&id).map_err(|e| GatewayError::BadRequest {
        message: e.to_string(),
    })?;

    if !removed {
        return Err(GatewayError::NotFound {
            resource: format!("workflow/{id}"),
        });
    }

    Ok(Json(json!({ "deleted": true, "workflow_id": id })))
}

/// GET /v1/workflows/:id/export — export a workflow as a portable JSON file.
pub(crate) async fn export_workflow(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, true, &Scope::Admin)?;

    let store = state.require_workflow_store()?;

    let workflow = store.get(&id).ok_or(GatewayError::NotFound {
        resource: format!("workflow/{id}"),
    })?;

    Ok(Json(json!({
        "workflow_id": workflow.workflow_id,
        "name": workflow.name,
        "description": workflow.description,
        "nodes": workflow.nodes,
        "edges": workflow.edges,
        "created_at": workflow.created_at,
        "updated_at": workflow.updated_at,
    })))
}

/// POST /v1/workflows/import — import a workflow from JSON.
///
/// Accepts a workflow definition, validates it compiles, and creates a new
/// workflow in the store with a fresh ID.
pub(crate) async fn import_workflow(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(req): AppJson<Value>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let store = state.require_workflow_store()?;

    let name = req["name"]
        .as_str()
        .unwrap_or("imported-workflow")
        .to_string();
    let description = req["description"].as_str().unwrap_or("").to_string();

    // Support both { nodes, edges } and { layout: { nodes, edges } } formats.
    let (nodes_val, edges_val) = if req.get("layout").is_some() {
        (
            req["layout"]["nodes"].clone(),
            req["layout"]["edges"].clone(),
        )
    } else {
        (req["nodes"].clone(), req["edges"].clone())
    };

    let nodes: Vec<serde_json::Value> =
        serde_json::from_value(nodes_val).map_err(|e| GatewayError::BadRequest {
            message: format!("invalid nodes array: {e}"),
        })?;
    let edges: Vec<serde_json::Value> =
        serde_json::from_value(edges_val).map_err(|e| GatewayError::BadRequest {
            message: format!("invalid edges array: {e}"),
        })?;

    // Validate the workflow compiles.
    agentzero_orchestrator::compile_workflow("validate", &nodes, &edges).map_err(|e| {
        GatewayError::BadRequest {
            message: format!("workflow validation failed: {e}"),
        }
    })?;

    let record = store
        .create(agentzero_orchestrator::WorkflowRecord {
            workflow_id: String::new(), // generated by store
            name: name.clone(),
            description,
            nodes: nodes.clone(),
            edges: edges.clone(),
            created_at: 0, // set by store
            updated_at: 0,
        })
        .map_err(|e| GatewayError::BadRequest {
            message: e.to_string(),
        })?;

    Ok(Json(json!({
        "workflow_id": record.workflow_id,
        "name": record.name,
        "nodes_count": record.nodes.len(),
        "edges_count": record.edges.len(),
        "imported": true,
    })))
}

/// POST /v1/workflows/:id/execute — compile and execute a workflow graph.
///
/// Spawns execution in a background task and returns immediately with a
/// `run_id`. Poll `GET /v1/workflows/runs/:run_id` for real-time status.
pub(crate) async fn execute_workflow(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    AppJson(req): AppJson<Value>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    let store = state.require_workflow_store()?;

    let workflow = store.get(&id).ok_or(GatewayError::NotFound {
        resource: format!("workflow/{id}"),
    })?;

    // Compile the workflow graph into an execution plan.
    let plan = agentzero_orchestrator::compile_workflow(&id, &workflow.nodes, &workflow.edges)
        .map_err(|e| GatewayError::BadRequest {
            message: e.to_string(),
        })?;

    let input = req["input"]["message"]
        .as_str()
        .or_else(|| req["input"].as_str())
        .or_else(|| req["message"].as_str())
        .unwrap_or("")
        .to_string();

    // Generate a run ID before creating dispatcher so gates can be keyed.
    let run_id = format!(
        "wfrun-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );

    let dispatcher: Arc<dyn agentzero_orchestrator::StepDispatcher> = Arc::new(
        crate::workflow_dispatch::GatewayStepDispatcher::from_state(&state, &plan, run_id.clone())
            .ok_or(GatewayError::AgentUnavailable)?,
    );

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Collect initial node statuses (all pending).
    let mut initial_statuses = std::collections::HashMap::new();
    for level in &plan.levels {
        for step in level {
            initial_statuses.insert(
                step.node_id.clone(),
                agentzero_orchestrator::NodeStatus::Pending,
            );
        }
    }

    let run_state = crate::state::WorkflowRunState {
        run_id: run_id.clone(),
        workflow_id: id.clone(),
        status: "running".to_string(),
        node_statuses: initial_statuses,
        node_outputs: std::collections::HashMap::new(),
        outputs: std::collections::HashMap::new(),
        started_at: now,
        finished_at: None,
        error: None,
    };

    // Store the initial state.
    {
        let mut runs = state.workflow_runs.lock().expect("workflow_runs lock");
        runs.insert(run_id.clone(), run_state);
    }

    // Set up status update channel — the executor sends updates, we write them
    // to the shared run store so the polling endpoint can see them.
    let (status_tx, mut status_rx) =
        tokio::sync::mpsc::channel::<agentzero_orchestrator::StatusUpdate>(64);
    let runs_ref = Arc::clone(&state.workflow_runs);
    let run_id_for_rx = run_id.clone();

    // Spawn a task that drains status updates into the shared run store.
    tokio::spawn(async move {
        while let Some(update) = status_rx.recv().await {
            tracing::info!(
                run_id = %run_id_for_rx,
                node_id = %update.node_id,
                node_name = %update.node_name,
                status = ?update.status,
                has_output = update.output.is_some(),
                "workflow status update received"
            );
            let mut runs = runs_ref.lock().expect("workflow_runs lock");
            if let Some(run) = runs.get_mut(&run_id_for_rx) {
                run.node_statuses
                    .insert(update.node_id.clone(), update.status);
                if let Some(ref output) = update.output {
                    run.node_outputs
                        .insert(update.node_id.clone(), output.clone());
                }
            }
        }
    });

    // Spawn the actual executor.
    let runs_ref2 = Arc::clone(&state.workflow_runs);
    let run_id_for_exec = run_id.clone();
    let workflow_id = id.clone();

    tokio::spawn(async move {
        let result = agentzero_orchestrator::execute_workflow_streaming(
            &plan,
            &input,
            dispatcher,
            Some(status_tx),
        )
        .await;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut runs = runs_ref2.lock().expect("workflow_runs lock");
        if let Some(run) = runs.get_mut(&run_id_for_exec) {
            run.finished_at = Some(now);
            match result {
                Ok(wf_run) => {
                    run.status = "completed".to_string();
                    for (k, v) in &wf_run.node_statuses {
                        run.node_statuses.insert(k.clone(), *v);
                    }
                    for ((node_id, port), val) in &wf_run.outputs {
                        run.outputs.insert(format!("{node_id}:{port}"), val.clone());
                    }
                }
                Err(e) => {
                    run.status = "failed".to_string();
                    run.error = Some(e.to_string());
                }
            }
        }

        tracing::info!(
            run_id = %run_id_for_exec,
            workflow_id = %workflow_id,
            "workflow execution finished"
        );
    });

    Ok(Json(json!({
        "run_id": run_id,
        "workflow_id": id,
        "status": "running",
    })))
}

/// POST /v1/swarm — decompose a goal and execute as a swarm.
///
/// Accepts `{ "goal": "...", "sandbox_level": "worktree" }` or
/// `{ "plan": { "title": "...", "nodes": [...] } }` for pre-planned workflows.
/// Returns a workflow run ID for status polling.
pub(crate) async fn swarm_execute(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(req): AppJson<Value>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    // Parse the request — either a goal string or a pre-planned workflow.
    let plan = if let Some(plan_val) = req.get("plan") {
        serde_json::from_value::<agentzero_orchestrator::PlannedWorkflow>(plan_val.clone())
            .map_err(|e| GatewayError::BadRequest {
                message: format!("invalid plan: {e}"),
            })?
    } else if let Some(goal) = req.get("goal").and_then(|v| v.as_str()) {
        // For now, wrap the goal in a single agent node.
        // In production, this would invoke GoalPlanner with an LLM.
        agentzero_orchestrator::PlannedWorkflow {
            title: goal.to_string(),
            nodes: vec![agentzero_orchestrator::PlannedNode {
                id: "agent-1".to_string(),
                name: "executor".to_string(),
                task: goal.to_string(),
                depends_on: vec![],
                file_scopes: vec![],
                sandbox_level: req
                    .get("sandbox_level")
                    .and_then(|v| v.as_str())
                    .unwrap_or("worktree")
                    .to_string(),
                tool_hints: vec![],
            }],
        }
    } else {
        return Err(GatewayError::BadRequest {
            message: "request must contain 'goal' (string) or 'plan' (object)".to_string(),
        });
    };

    // Compile the plan.
    let (nodes, edges) = plan.to_workflow_json();
    let exec_plan =
        agentzero_orchestrator::compile_workflow("swarm", &nodes, &edges).map_err(|e| {
            GatewayError::BadRequest {
                message: format!("plan compilation failed: {e}"),
            }
        })?;

    // Generate run ID before dispatcher so gates can be keyed.
    let run_id = format!(
        "swarm-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );

    // Build dispatcher.
    let dispatcher: Arc<dyn agentzero_orchestrator::StepDispatcher> = Arc::new(
        crate::workflow_dispatch::GatewayStepDispatcher::from_state(
            &state,
            &exec_plan,
            run_id.clone(),
        )
        .ok_or(GatewayError::AgentUnavailable)?,
    );

    let input = req
        .get("input")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Execute via SwarmSupervisor in a background task.
    let supervisor = agentzero_orchestrator::SwarmSupervisor::new();
    let plan_clone = plan.clone();

    let (status_tx, mut status_rx) =
        tokio::sync::mpsc::channel::<agentzero_orchestrator::StatusUpdate>(64);
    let run_id_clone = run_id.clone();

    // Store initial run state.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut initial_statuses = std::collections::HashMap::new();
    for node in &plan.nodes {
        initial_statuses.insert(node.id.clone(), agentzero_orchestrator::NodeStatus::Pending);
    }

    let run_state = crate::state::WorkflowRunState {
        run_id: run_id.clone(),
        workflow_id: "swarm".to_string(),
        status: "running".to_string(),
        node_statuses: initial_statuses,
        node_outputs: std::collections::HashMap::new(),
        outputs: std::collections::HashMap::new(),
        started_at: now,
        finished_at: None,
        error: None,
    };

    {
        let mut runs = state.workflow_runs.lock().expect("workflow_runs lock");
        runs.insert(run_id.clone(), run_state);
    }

    // Drain status updates to the run store.
    let runs_ref = Arc::clone(&state.workflow_runs);
    let run_id_for_rx = run_id.clone();
    tokio::spawn(async move {
        while let Some(update) = status_rx.recv().await {
            let mut runs = runs_ref.lock().expect("workflow_runs lock");
            if let Some(run) = runs.get_mut(&run_id_for_rx) {
                run.node_statuses
                    .insert(update.node_id.clone(), update.status);
                if let Some(ref output) = update.output {
                    run.node_outputs
                        .insert(update.node_id.clone(), output.clone());
                }
            }
        }
    });

    // Execute in background.
    let runs_ref2 = Arc::clone(&state.workflow_runs);
    tokio::spawn(async move {
        let result = supervisor
            .execute(&plan_clone, &input, dispatcher, Some(status_tx))
            .await;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut runs = runs_ref2.lock().expect("workflow_runs lock");
        if let Some(run) = runs.get_mut(&run_id_clone) {
            run.finished_at = Some(now);
            match result {
                Ok(swarm_result) => {
                    run.status = if swarm_result.success {
                        "completed".to_string()
                    } else {
                        "failed".to_string()
                    };
                    for (k, v) in &swarm_result.node_statuses {
                        run.node_statuses.insert(k.clone(), *v);
                    }
                    for (k, v) in &swarm_result.outputs {
                        run.outputs.insert(k.clone(), serde_json::json!(v));
                    }
                }
                Err(e) => {
                    run.status = "failed".to_string();
                    run.error = Some(e.to_string());
                }
            }
        }
    });

    Ok(Json(json!({
        "run_id": run_id,
        "title": plan.title,
        "node_count": plan.nodes.len(),
        "status": "running",
    })))
}

/// GET /v1/workflows/runs/:run_id — poll for workflow run status.
pub(crate) async fn get_workflow_run(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let runs = state.workflow_runs.lock().expect("workflow_runs lock");
    let run = runs.get(&run_id).ok_or(GatewayError::NotFound {
        resource: format!("workflow-run/{run_id}"),
    })?;

    tracing::debug!(
        run_id = %run.run_id,
        run_status = %run.status,
        node_count = run.node_statuses.len(),
        node_statuses = ?run.node_statuses,
        "polling workflow run status"
    );

    let statuses: serde_json::Map<String, Value> = run
        .node_statuses
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or(Value::Null)))
        .collect();

    let node_outputs: serde_json::Map<String, Value> = run
        .node_outputs
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();

    let outputs: serde_json::Map<String, Value> = run
        .outputs
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    Ok(Json(json!({
        "run_id": run.run_id,
        "workflow_id": run.workflow_id,
        "status": run.status,
        "node_statuses": statuses,
        "node_outputs": node_outputs,
        "outputs": outputs,
        "started_at": run.started_at,
        "finished_at": run.finished_at,
        "error": run.error,
    })))
}

/// DELETE /v1/workflows/runs/:run_id — cancel a workflow run.
///
/// Marks the run as failed, drops any suspended gate senders (which auto-denies),
/// and removes the run from the active runs map.
pub(crate) async fn cancel_workflow_run(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsManage)?;

    let mut runs = state.workflow_runs.lock().expect("workflow_runs lock");
    let run = runs.get_mut(&run_id).ok_or(GatewayError::NotFound {
        resource: format!("workflow-run/{run_id}"),
    })?;

    // Mark as cancelled/failed.
    run.status = "cancelled".to_string();
    run.finished_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );
    run.error = Some("cancelled by user".to_string());

    // Drop gate senders for this run (auto-denies any suspended gates).
    {
        let mut senders = state.gate_senders.lock().expect("gate_senders lock");
        senders.retain(|(rid, _), _| rid != &run_id);
    }

    tracing::info!(run_id = %run_id, "workflow run cancelled");

    Ok(Json(json!({
        "cancelled": true,
        "run_id": run_id,
    })))
}

/// GET /v1/workflows/runs/:run_id/stream — SSE stream for workflow run status.
///
/// Polls the workflow run state every 500ms and emits node status events.
/// Ends when the run reaches a terminal state or after 10 minutes.
pub(crate) async fn stream_workflow_run(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> Result<Response, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    // Verify the run exists.
    {
        let runs = state.workflow_runs.lock().expect("workflow_runs lock");
        if !runs.contains_key(&run_id) {
            return Err(GatewayError::NotFound {
                resource: format!("workflow-run/{run_id}"),
            });
        }
    }

    let runs_ref = Arc::clone(&state.workflow_runs);
    let rid = run_id.clone();

    let stream = async_stream::stream! {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(600);

        loop {
            // Check if we've exceeded the deadline.
            if tokio::time::Instant::now() >= deadline {
                yield Ok::<_, std::convert::Infallible>(
                    "data: {\"event\":\"timeout\"}\n\n".to_string()
                );
                break;
            }

            let (status, statuses, error) = {
                let runs = runs_ref.lock().expect("workflow_runs lock");
                match runs.get(&rid) {
                    Some(run) => {
                        let statuses: serde_json::Map<String, Value> = run
                            .node_statuses
                            .iter()
                            .map(|(k, v)| {
                                (k.clone(), serde_json::to_value(v).unwrap_or(Value::Null))
                            })
                            .collect();
                        (run.status.clone(), statuses, run.error.clone())
                    }
                    None => break,
                }
            };

            let event = json!({
                "run_id": rid,
                "status": status,
                "node_statuses": statuses,
                "error": error,
            });
            yield Ok(format!("data: {event}\n\n"));

            // Terminal states end the stream.
            if status == "completed" || status == "failed" || status == "cancelled" {
                break;
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    };

    Ok(Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .body(Body::from_stream(stream))
        .expect("valid sse response"))
}

/// POST /v1/workflows/runs/:run_id/resume — resume a suspended gate node.
///
/// Accepts `{ "node_id": "...", "decision": "approved"|"denied" }`.
/// Unblocks the gate node's execution task with the given decision.
pub(crate) async fn resume_workflow_run(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    AppJson(req): AppJson<Value>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    let node_id = req["node_id"]
        .as_str()
        .ok_or(GatewayError::BadRequest {
            message: "missing 'node_id' field".to_string(),
        })?
        .to_string();

    let decision = req["decision"]
        .as_str()
        .ok_or(GatewayError::BadRequest {
            message: "missing 'decision' field (must be 'approved' or 'denied')".to_string(),
        })?
        .to_string();

    if decision != "approved" && decision != "denied" {
        return Err(GatewayError::BadRequest {
            message: format!("decision must be 'approved' or 'denied', got '{decision}'"),
        });
    }

    // Look up the gate sender.
    let sender = {
        let mut senders = state.gate_senders.lock().expect("gate_senders lock");
        senders.remove(&(run_id.clone(), node_id.clone()))
    };

    match sender {
        Some(tx) => {
            let _ = tx.send(decision.clone());

            // Update the node status from Suspended to Running.
            {
                let mut runs = state.workflow_runs.lock().expect("workflow_runs lock");
                if let Some(run) = runs.get_mut(&run_id) {
                    run.node_statuses.insert(
                        node_id.clone(),
                        agentzero_orchestrator::NodeStatus::Running,
                    );
                }
            }

            tracing::info!(
                run_id = %run_id,
                node_id = %node_id,
                decision = %decision,
                "gate node resumed"
            );

            Ok(Json(json!({
                "resumed": true,
                "run_id": run_id,
                "node_id": node_id,
                "decision": decision,
            })))
        }
        None => Err(GatewayError::NotFound {
            resource: format!(
                "suspended gate node '{node_id}' in run '{run_id}' (may have already been resumed or timed out)"
            ),
        }),
    }
}

// ---------------------------------------------------------------------------
// Workflow channel trigger helper
