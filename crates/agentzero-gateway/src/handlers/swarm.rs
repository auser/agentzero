use super::*;

/// Returns the number of workflows triggered.
pub(super) async fn trigger_workflows_for_channel(
    state: &GatewayState,
    wf_store: &agentzero_orchestrator::WorkflowStore,
    channel: &str,
    message_text: &str,
) -> usize {
    let workflows = wf_store.list();
    let mut triggered = 0;

    for workflow in &workflows {
        // Scan nodes for Channel trigger nodes matching this channel.
        let has_trigger = workflow.nodes.iter().any(|node| {
            let node_type = node
                .get("data")
                .and_then(|d| d.get("nodeType"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            let channel_type = node
                .get("data")
                .and_then(|d| d.get("metadata"))
                .and_then(|m| m.get("channel_type"))
                .and_then(|c| c.as_str())
                .unwrap_or("");

            node_type == "channel" && channel_type == channel
        });

        if !has_trigger {
            continue;
        }

        // Compile and execute the workflow with the message as input.
        let plan = match agentzero_orchestrator::compile_workflow(
            &workflow.workflow_id,
            &workflow.nodes,
            &workflow.edges,
        ) {
            Ok(plan) => plan,
            Err(e) => {
                tracing::warn!(
                    workflow_id = %workflow.workflow_id,
                    error = %e,
                    "failed to compile triggered workflow"
                );
                continue;
            }
        };

        let run_id = format!(
            "trigger-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let dispatcher: Arc<dyn agentzero_orchestrator::StepDispatcher> =
            match crate::workflow_dispatch::GatewayStepDispatcher::from_state(
                state,
                &plan,
                run_id.clone(),
            ) {
                Some(d) => Arc::new(d),
                None => continue,
            };

        // Store initial run state.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

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
            workflow_id: workflow.workflow_id.clone(),
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

        let input = message_text.to_string();
        let runs_ref = Arc::clone(&state.workflow_runs);
        let run_id_for_exec = run_id.clone();
        let wf_id = workflow.workflow_id.clone();

        // Execute in background.
        tokio::spawn(async move {
            let result =
                agentzero_orchestrator::execute_workflow_streaming(&plan, &input, dispatcher, None)
                    .await;

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let mut runs = runs_ref.lock().expect("workflow_runs lock");
            if let Some(run) = runs.get_mut(&run_id_for_exec) {
                run.finished_at = Some(now);
                match result {
                    Ok(wf_run) => {
                        run.status = "completed".to_string();
                        for (k, v) in &wf_run.node_statuses {
                            run.node_statuses.insert(k.clone(), *v);
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
                workflow_id = %wf_id,
                "channel-triggered workflow execution finished"
            );
        });

        triggered += 1;
    }

    triggered
}

// ---------------------------------------------------------------------------
// Template CRUD: /v1/templates
