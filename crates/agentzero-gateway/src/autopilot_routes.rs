//! Stub REST endpoints for the autopilot subsystem.
//!
//! These handlers return placeholder responses. The actual autopilot integration
//! will be wired later when the `AutopilotLoop` is running and state is shared
//! with the gateway.

use axum::{extract::Path, http::StatusCode, response::IntoResponse, Json};
use serde_json::{json, Value};

/// GET /v1/autopilot/proposals — list proposals (empty stub).
pub(crate) async fn list_proposals() -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [],
        "total": 0
    }))
}

/// POST /v1/autopilot/proposals/:id/approve — approve a proposal (stub).
pub(crate) async fn approve_proposal(Path(id): Path<String>) -> impl IntoResponse {
    (
        StatusCode::ACCEPTED,
        Json(json!({
            "id": id,
            "action": "approve",
            "accepted": true
        })),
    )
}

/// POST /v1/autopilot/proposals/:id/reject — reject a proposal (stub).
pub(crate) async fn reject_proposal(Path(id): Path<String>) -> impl IntoResponse {
    (
        StatusCode::ACCEPTED,
        Json(json!({
            "id": id,
            "action": "reject",
            "accepted": true
        })),
    )
}

/// GET /v1/autopilot/missions — list missions (empty stub).
pub(crate) async fn list_missions() -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [],
        "total": 0
    }))
}

/// GET /v1/autopilot/missions/:id — mission detail (stub returns 404).
pub(crate) async fn get_mission(Path(id): Path<String>) -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": {
                "type": "not_found",
                "message": format!("mission not found: {id}")
            }
        })),
    )
}

/// GET /v1/autopilot/triggers — list triggers (empty stub).
pub(crate) async fn list_triggers() -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [],
        "total": 0
    }))
}

/// POST /v1/autopilot/triggers/:id/toggle — toggle a trigger (stub).
pub(crate) async fn toggle_trigger(Path(id): Path<String>) -> impl IntoResponse {
    (
        StatusCode::ACCEPTED,
        Json(json!({
            "id": id,
            "action": "toggle",
            "accepted": true
        })),
    )
}

/// GET /v1/autopilot/stats — zeroed stats object.
pub(crate) async fn autopilot_stats() -> Json<Value> {
    Json(json!({
        "proposals_pending": 0,
        "proposals_approved": 0,
        "proposals_rejected": 0,
        "missions_active": 0,
        "missions_completed": 0,
        "missions_failed": 0,
        "triggers_enabled": 0,
        "triggers_total": 0,
        "total_cost_microdollars": 0
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::middleware::MiddlewareConfig;
    use crate::router::build_router;
    use crate::state::GatewayState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn default_config() -> MiddlewareConfig {
        MiddlewareConfig::default()
    }

    #[tokio::test]
    async fn autopilot_proposals_returns_empty_list() {
        let app = build_router(GatewayState::test_with_bearer(None), &default_config());
        let request = Request::builder()
            .method("GET")
            .uri("/v1/autopilot/proposals")
            .body(Body::empty())
            .expect("request should build");

        let response = app
            .oneshot(request)
            .await
            .expect("response should be returned");
        assert_eq!(response.status(), StatusCode::OK);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("body should be valid json");
        assert_eq!(json["object"], "list");
        assert_eq!(json["data"], serde_json::json!([]));
        assert_eq!(json["total"], 0);
    }

    #[tokio::test]
    async fn autopilot_approve_proposal_returns_202() {
        let app = build_router(GatewayState::test_with_bearer(None), &default_config());
        let request = Request::builder()
            .method("POST")
            .uri("/v1/autopilot/proposals/prop-123/approve")
            .body(Body::empty())
            .expect("request should build");

        let response = app
            .oneshot(request)
            .await
            .expect("response should be returned");
        assert_eq!(response.status(), StatusCode::ACCEPTED);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("body should be valid json");
        assert_eq!(json["id"], "prop-123");
        assert_eq!(json["action"], "approve");
        assert_eq!(json["accepted"], true);
    }

    #[tokio::test]
    async fn autopilot_stats_returns_zeroed() {
        let app = build_router(GatewayState::test_with_bearer(None), &default_config());
        let request = Request::builder()
            .method("GET")
            .uri("/v1/autopilot/stats")
            .body(Body::empty())
            .expect("request should build");

        let response = app
            .oneshot(request)
            .await
            .expect("response should be returned");
        assert_eq!(response.status(), StatusCode::OK);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("body should be valid json");
        assert_eq!(json["proposals_pending"], 0);
        assert_eq!(json["missions_active"], 0);
        assert_eq!(json["triggers_total"], 0);
        assert_eq!(json["total_cost_microdollars"], 0);
    }

    #[tokio::test]
    async fn autopilot_mission_detail_returns_404() {
        let app = build_router(GatewayState::test_with_bearer(None), &default_config());
        let request = Request::builder()
            .method("GET")
            .uri("/v1/autopilot/missions/miss-456")
            .body(Body::empty())
            .expect("request should build");

        let response = app
            .oneshot(request)
            .await
            .expect("response should be returned");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("body should be valid json");
        assert_eq!(json["error"]["type"], "not_found");
    }
}
