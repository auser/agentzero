//! A2A (Agent-to-Agent) protocol server endpoints.
//!
//! Implements:
//! - `GET /.well-known/agent.json` — Agent Card discovery
//! - `POST /a2a` — JSON-RPC task lifecycle (tasks/send, tasks/get, tasks/cancel)

use crate::state::GatewayState;
use agentzero_core::a2a_types::*;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// In-memory task store for A2A tasks.
/// Production deployments should use the JobStore, but this provides
/// a self-contained A2A implementation.
#[derive(Clone, Default)]
pub(crate) struct A2aTaskStore {
    tasks: Arc<Mutex<HashMap<String, Task>>>,
}

impl A2aTaskStore {
    pub(crate) fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn get(&self, id: &str) -> Option<Task> {
        self.tasks.lock().await.get(id).cloned()
    }

    async fn upsert(&self, task: Task) {
        self.tasks.lock().await.insert(task.id.clone(), task);
    }
}

/// `GET /.well-known/agent.json` — Return the Agent Card.
pub(crate) async fn agent_card(State(state): State<GatewayState>) -> Json<AgentCard> {
    let tool_count = state
        .mcp_server
        .as_ref()
        .map(|s| s.tool_count())
        .unwrap_or(0);

    let skills = if tool_count > 0 {
        vec![AgentSkill {
            id: "general".to_string(),
            name: "General Agent".to_string(),
            description: Some(format!("AgentZero agent with {tool_count} tools available")),
            tags: vec!["agent".to_string(), "tools".to_string()],
        }]
    } else {
        vec![]
    };

    Json(AgentCard {
        name: state.service_name.as_ref().clone(),
        description: Some("AgentZero AI agent".to_string()),
        url: "http://localhost".to_string(),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        capabilities: AgentCapabilities {
            streaming: false,
            push_notifications: false,
            state_transition_history: true,
        },
        skills,
        default_input_modes: Some(vec!["text".to_string()]),
        default_output_modes: Some(vec!["text".to_string()]),
    })
}

/// `POST /a2a` — Handle A2A JSON-RPC requests.
///
/// Supports: `tasks/send`, `tasks/get`, `tasks/cancel`.
pub(crate) async fn a2a_rpc(
    State(state): State<GatewayState>,
    _headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let method = body.get("method").and_then(Value::as_str).unwrap_or("");
    let params = body.get("params").cloned().unwrap_or(json!({}));

    let result = match method {
        "tasks/send" | "message/send" => handle_tasks_send(&state, &params).await,
        "tasks/get" => handle_tasks_get(&state, &params).await,
        "tasks/cancel" => handle_tasks_cancel(&state, &params).await,
        _ => Err(json!({
            "code": -32601,
            "message": format!("method not found: {method}"),
        })),
    };

    match result {
        Ok(result) => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        })),
        Err(error) => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": error,
        })),
    }
}

async fn handle_tasks_send(state: &GatewayState, params: &Value) -> Result<Value, Value> {
    let send_params: TaskSendParams = serde_json::from_value(params.clone()).map_err(|e| {
        json!({
            "code": -32602,
            "message": format!("invalid params: {e}"),
        })
    })?;

    // Extract text from the message parts.
    let text: String = send_params
        .message
        .parts
        .iter()
        .filter_map(|p| match p {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    if text.is_empty() {
        return Err(json!({
            "code": -32602,
            "message": "message must contain at least one text part",
        }));
    }

    // Execute via gateway channel if available (full agent loop),
    // otherwise acknowledge receipt.
    let response_text = if let Some(gw_channel) = &state.gateway_channel {
        let timeout = std::time::Duration::from_secs(120);
        match gw_channel.submit(text.clone(), timeout).await {
            Ok(resp) => resp,
            Err(e) => format!("agent error: {e}"),
        }
    } else if state.mcp_server.is_some() {
        let tool_count = state
            .mcp_server
            .as_ref()
            .map(|s| s.tool_count())
            .unwrap_or(0);
        format!("task received ({tool_count} tools available, no agent loop configured)")
    } else {
        "task received (no tools loaded)".to_string()
    };

    // Build the completed task.
    let task = Task {
        id: send_params.id,
        status: TaskStatus {
            state: TaskState::Completed,
            message: Some(Message {
                role: MessageRole::Agent,
                parts: vec![Part::text(&response_text)],
            }),
        },
        history: vec![
            send_params.message,
            Message {
                role: MessageRole::Agent,
                parts: vec![Part::text(&response_text)],
            },
        ],
        artifacts: vec![],
    };

    // Store the task.
    state.a2a_tasks.upsert(task.clone()).await;

    Ok(serde_json::to_value(task).unwrap_or(json!({})))
}

async fn handle_tasks_get(state: &GatewayState, params: &Value) -> Result<Value, Value> {
    let get_params: TaskGetParams = serde_json::from_value(params.clone()).map_err(|e| {
        json!({
            "code": -32602,
            "message": format!("invalid params: {e}"),
        })
    })?;

    match state.a2a_tasks.get(&get_params.id).await {
        Some(mut task) => {
            if let Some(max_len) = get_params.history_length {
                let len = task.history.len();
                if len > max_len {
                    task.history = task.history[len - max_len..].to_vec();
                }
            }
            Ok(serde_json::to_value(task).unwrap_or(json!({})))
        }
        None => Err(json!({
            "code": -32602,
            "message": format!("task not found: {}", get_params.id),
        })),
    }
}

async fn handle_tasks_cancel(state: &GatewayState, params: &Value) -> Result<Value, Value> {
    let cancel_params: TaskCancelParams = serde_json::from_value(params.clone()).map_err(|e| {
        json!({
            "code": -32602,
            "message": format!("invalid params: {e}"),
        })
    })?;

    let mut tasks = state.a2a_tasks.tasks.lock().await;
    match tasks.get_mut(&cancel_params.id) {
        Some(task) => {
            task.status = TaskStatus {
                state: TaskState::Canceled,
                message: None,
            };
            Ok(serde_json::to_value(task.clone()).unwrap_or(json!({})))
        }
        None => Err(json!({
            "code": -32602,
            "message": format!("task not found: {}", cancel_params.id),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a2a_task_store_upsert_and_get() {
        let store = A2aTaskStore::new();
        let task = Task {
            id: "t1".to_string(),
            status: TaskStatus {
                state: TaskState::Completed,
                message: None,
            },
            history: vec![],
            artifacts: vec![],
        };
        store.upsert(task).await;
        let retrieved = store.get("t1").await.expect("should find task");
        assert_eq!(retrieved.id, "t1");
        assert_eq!(retrieved.status.state, TaskState::Completed);
    }

    #[tokio::test]
    async fn a2a_task_store_get_missing_returns_none() {
        let store = A2aTaskStore::new();
        assert!(store.get("nonexistent").await.is_none());
    }
}
