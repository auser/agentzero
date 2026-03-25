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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Persistent task store for A2A tasks with optional file-backed persistence.
/// Falls back to in-memory only when no workspace root is configured.
#[derive(Clone)]
pub(crate) struct A2aTaskStore {
    tasks: Arc<Mutex<HashMap<String, Task>>>,
    /// Path to persist tasks. None = in-memory only.
    persist_path: Option<PathBuf>,
    /// Maximum number of tasks to retain.
    max_tasks: usize,
}

impl Default for A2aTaskStore {
    fn default() -> Self {
        Self::new()
    }
}

impl A2aTaskStore {
    pub(crate) fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            persist_path: None,
            max_tasks: 1000,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn with_persistence(mut self, workspace_root: &Path) -> Self {
        self.persist_path = Some(workspace_root.join(".agentzero").join("a2a_tasks.json"));
        self
    }

    #[allow(dead_code)]
    pub(crate) async fn load(&self) -> anyhow::Result<()> {
        if let Some(ref path) = self.persist_path {
            if path.exists() {
                let data = tokio::fs::read_to_string(path).await?;
                let tasks: HashMap<String, Task> = serde_json::from_str(&data)?;
                *self.tasks.lock().await = tasks;
            }
        }
        Ok(())
    }

    async fn persist(&self) {
        if let Some(ref path) = self.persist_path {
            let tasks = self.tasks.lock().await;
            if let Ok(data) = serde_json::to_string_pretty(&*tasks) {
                if let Some(parent) = path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                let _ = tokio::fs::write(path, data).await;
            }
        }
    }

    pub(crate) async fn get(&self, id: &str) -> Option<Task> {
        self.tasks.lock().await.get(id).cloned()
    }

    pub(crate) async fn upsert(&self, task: Task) {
        {
            let mut tasks = self.tasks.lock().await;
            tasks.insert(task.id.clone(), task);
            // Evict excess tasks inline to keep the store bounded.
            if tasks.len() > self.max_tasks {
                let excess = tasks.len() - self.max_tasks;
                let keys_to_remove: Vec<String> = tasks.keys().take(excess).cloned().collect();
                for key in keys_to_remove {
                    tasks.remove(&key);
                }
            }
        }
        self.persist().await;
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

    let public_url = crate::handlers::resolve_public_url(&state)
        .unwrap_or_else(|| "http://localhost".to_string());

    Json(AgentCard {
        name: state.service_name.as_ref().clone(),
        description: Some("AgentZero AI agent".to_string()),
        url: public_url,
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
/// When `[a2a] bearer_token` is configured, requires `Authorization: Bearer <token>`.
pub(crate) async fn a2a_rpc(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    // Enforce bearer token if configured.
    if let Some(ref rx) = state.live_config {
        if let Some(ref expected_token) = rx.borrow().a2a.bearer_token {
            let provided = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "));
            match provided {
                Some(token) if token == expected_token.as_str() => {}
                _ => {
                    return Json(json!({
                        "jsonrpc": "2.0",
                        "id": body.get("id").cloned().unwrap_or(Value::Null),
                        "error": {
                            "code": -32600,
                            "message": "unauthorized: invalid or missing bearer token",
                        },
                    }));
                }
            }
        }
    }

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

    let result = {
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
    };

    if result.is_ok() {
        state.a2a_tasks.persist().await;
    }

    result
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

    #[tokio::test]
    async fn a2a_task_store_persist_and_load() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = CTR.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-a2a-persist-{}-{nanos}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");

        // Create store with persistence and insert a task.
        let store = A2aTaskStore::new().with_persistence(&dir);
        let task = Task {
            id: "persist-t1".to_string(),
            status: TaskStatus {
                state: TaskState::Completed,
                message: None,
            },
            history: vec![],
            artifacts: vec![],
        };
        store.upsert(task).await;

        // Create a new store from the same path and load.
        let store2 = A2aTaskStore::new().with_persistence(&dir);
        store2.load().await.expect("load should succeed");
        let retrieved = store2
            .get("persist-t1")
            .await
            .expect("should find persisted task");
        assert_eq!(retrieved.id, "persist-t1");
        assert_eq!(retrieved.status.state, TaskState::Completed);

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn a2a_task_store_evict_on_upsert() {
        let store = A2aTaskStore {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            persist_path: None,
            max_tasks: 3,
        };

        // Insert 8 tasks — eviction happens automatically on each upsert.
        for i in 0..8 {
            let task = Task {
                id: format!("evict-{i}"),
                status: TaskStatus {
                    state: TaskState::Completed,
                    message: None,
                },
                history: vec![],
                artifacts: vec![],
            };
            store.upsert(task).await;
        }

        let remaining = store.tasks.lock().await.len();
        assert!(
            remaining <= 3,
            "expected at most 3 tasks after eviction, got {remaining}"
        );
    }
}
