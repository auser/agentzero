//! REST API endpoints for managing persistent agents via the Config UI.

use agentzero_orchestrator::agent_store::{AgentRecord, AgentStatus, AgentStore, AgentUpdate};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

/// Shared state for agent API routes.
pub type AgentStoreState = Arc<AgentStore>;

#[derive(Deserialize)]
pub struct CreateAgentRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateAgentRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub keywords: Option<Vec<String>>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct SetStatusRequest {
    pub active: bool,
}

/// GET /api/agents — list all persistent agents.
pub async fn list_agents(State(store): State<AgentStoreState>) -> Json<Vec<AgentRecord>> {
    Json(store.list())
}

/// POST /api/agents — create a new persistent agent.
pub async fn create_agent(
    State(store): State<AgentStoreState>,
    Json(req): Json<CreateAgentRequest>,
) -> Result<(StatusCode, Json<AgentRecord>), (StatusCode, String)> {
    if req.name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name is required".to_string()));
    }
    let record = AgentRecord {
        agent_id: String::new(),
        name: req.name,
        description: req.description,
        system_prompt: req.system_prompt,
        provider: req.provider,
        model: req.model,
        keywords: req.keywords,
        allowed_tools: req.allowed_tools,
        channels: HashMap::new(),
        created_at: 0,
        updated_at: 0,
        status: AgentStatus::Active,
    };
    match store.create(record) {
        Ok(created) => Ok((StatusCode::CREATED, Json(created))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

/// GET /api/agents/:id — get agent by ID.
pub async fn get_agent(
    State(store): State<AgentStoreState>,
    Path(id): Path<String>,
) -> Result<Json<AgentRecord>, StatusCode> {
    store.get(&id).map(Json).ok_or(StatusCode::NOT_FOUND)
}

/// PUT /api/agents/:id — update agent fields.
pub async fn update_agent(
    State(store): State<AgentStoreState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateAgentRequest>,
) -> Result<Json<AgentRecord>, (StatusCode, String)> {
    let update = AgentUpdate {
        name: req.name,
        description: req.description,
        system_prompt: req.system_prompt,
        provider: req.provider,
        model: req.model,
        keywords: req.keywords,
        allowed_tools: req.allowed_tools,
        channels: None,
    };
    match store.update(&id, update) {
        Ok(Some(updated)) => Ok(Json(updated)),
        Ok(None) => Err((StatusCode::NOT_FOUND, format!("agent '{id}' not found"))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

/// DELETE /api/agents/:id — delete agent.
pub async fn delete_agent(
    State(store): State<AgentStoreState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match store.delete(&id) {
        Ok(true) => StatusCode::NO_CONTENT,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// PUT /api/agents/:id/status — set active/stopped.
pub async fn set_agent_status(
    State(store): State<AgentStoreState>,
    Path(id): Path<String>,
    Json(req): Json<SetStatusRequest>,
) -> impl IntoResponse {
    let status = if req.active {
        AgentStatus::Active
    } else {
        AgentStatus::Stopped
    };
    match store.set_status(&id, status) {
        Ok(true) => StatusCode::OK,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::{get, put};
    use axum::Router;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_app(store: AgentStoreState) -> Router {
        Router::new()
            .route("/api/agents", get(list_agents).post(create_agent))
            .route(
                "/api/agents/:id",
                get(get_agent).put(update_agent).delete(delete_agent),
            )
            .route("/api/agents/:id/status", put(set_agent_status))
            .with_state(store)
    }

    fn test_router() -> Router {
        test_app(Arc::new(AgentStore::new()))
    }

    async fn body_json<T: serde::de::DeserializeOwned>(
        res: axum::http::Response<axum::body::Body>,
    ) -> T {
        let bytes = res
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        serde_json::from_slice(&bytes).expect("parse json")
    }

    fn json_request(
        method: &str,
        uri: &str,
        body: serde_json::Value,
    ) -> axum::http::Request<axum::body::Body> {
        axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&body).expect("serialize"),
            ))
            .expect("build request")
    }

    #[tokio::test]
    async fn list_empty() {
        let app = test_router();
        let req = axum::http::Request::builder()
            .uri("/api/agents")
            .body(axum::body::Body::empty())
            .expect("build request");
        let res = app.oneshot(req).await.expect("request");
        assert_eq!(res.status(), StatusCode::OK);
        let agents: Vec<AgentRecord> = body_json(res).await;
        assert!(agents.is_empty());
    }

    #[tokio::test]
    async fn create_returns_201() {
        let app = test_router();
        let req = json_request(
            "POST",
            "/api/agents",
            serde_json::json!({"name": "Aria", "model": "gpt-4o"}),
        );
        let res = app.oneshot(req).await.expect("request");
        assert_eq!(res.status(), StatusCode::CREATED);
        let created: AgentRecord = body_json(res).await;
        assert_eq!(created.name, "Aria");
        assert_eq!(created.model, "gpt-4o");
        assert!(!created.agent_id.is_empty());
    }

    #[tokio::test]
    async fn create_and_get() {
        let store: AgentStoreState = Arc::new(AgentStore::new());
        let app = test_app(store);

        // Create
        let req = json_request(
            "POST",
            "/api/agents",
            serde_json::json!({"name": "Bot", "provider": "anthropic"}),
        );
        let res = app.clone().oneshot(req).await.expect("create");
        assert_eq!(res.status(), StatusCode::CREATED);
        let created: AgentRecord = body_json(res).await;

        // Get
        let req = axum::http::Request::builder()
            .uri(format!("/api/agents/{}", created.agent_id))
            .body(axum::body::Body::empty())
            .expect("build");
        let res = app.oneshot(req).await.expect("get");
        assert_eq!(res.status(), StatusCode::OK);
        let fetched: AgentRecord = body_json(res).await;
        assert_eq!(fetched.name, "Bot");
    }

    #[tokio::test]
    async fn get_unknown_returns_404() {
        let app = test_router();
        let req = axum::http::Request::builder()
            .uri("/api/agents/nonexistent")
            .body(axum::body::Body::empty())
            .expect("build");
        let res = app.oneshot(req).await.expect("request");
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_unknown_returns_404() {
        let app = test_router();
        let req = axum::http::Request::builder()
            .method("DELETE")
            .uri("/api/agents/nonexistent")
            .body(axum::body::Body::empty())
            .expect("build");
        let res = app.oneshot(req).await.expect("request");
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn create_update_delete_lifecycle() {
        let store: AgentStoreState = Arc::new(AgentStore::new());
        let app = test_app(store);

        // Create
        let req = json_request("POST", "/api/agents", serde_json::json!({"name": "Old"}));
        let res = app.clone().oneshot(req).await.expect("create");
        let created: AgentRecord = body_json(res).await;

        // Update
        let req = json_request(
            "PUT",
            &format!("/api/agents/{}", created.agent_id),
            serde_json::json!({"name": "New", "model": "new-model"}),
        );
        let res = app.clone().oneshot(req).await.expect("update");
        assert_eq!(res.status(), StatusCode::OK);
        let updated: AgentRecord = body_json(res).await;
        assert_eq!(updated.name, "New");
        assert_eq!(updated.model, "new-model");

        // Delete
        let req = axum::http::Request::builder()
            .method("DELETE")
            .uri(format!("/api/agents/{}", created.agent_id))
            .body(axum::body::Body::empty())
            .expect("build");
        let res = app.clone().oneshot(req).await.expect("delete");
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        // Verify gone
        let req = axum::http::Request::builder()
            .uri(format!("/api/agents/{}", created.agent_id))
            .body(axum::body::Body::empty())
            .expect("build");
        let res = app.oneshot(req).await.expect("get after delete");
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
