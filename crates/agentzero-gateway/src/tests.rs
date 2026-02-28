use crate::router::build_router;
use crate::state::GatewayState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn pair_rejects_wrong_pairing_code_negative_path() {
    let app = build_router(GatewayState::test_with_bearer(None));
    let request = Request::builder()
        .method("POST")
        .uri("/pair")
        .header("x-pairing-code", "000000")
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn pair_success_and_api_fallback_requires_token_success_path() {
    let app = build_router(GatewayState::test_with_bearer(None));

    let pair_request = Request::builder()
        .method("POST")
        .uri("/pair")
        .header("x-pairing-code", "406823")
        .body(Body::empty())
        .expect("request should build");
    let pair_response = app
        .clone()
        .oneshot(pair_request)
        .await
        .expect("response should be returned");
    assert_eq!(pair_response.status(), StatusCode::OK);
    let pair_body = pair_response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let pair_json: serde_json::Value =
        serde_json::from_slice(&pair_body).expect("pair body should be json");
    let token = pair_json["token"]
        .as_str()
        .expect("token should be string")
        .to_string();

    let blocked_request = Request::builder()
        .method("GET")
        .uri("/api/internal")
        .body(Body::empty())
        .expect("request should build");
    let blocked_response = app
        .clone()
        .oneshot(blocked_request)
        .await
        .expect("response should be returned");
    assert_eq!(blocked_response.status(), StatusCode::UNAUTHORIZED);

    let allowed_request = Request::builder()
        .method("GET")
        .uri("/api/internal")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request should build");
    let allowed_response = app
        .oneshot(allowed_request)
        .await
        .expect("response should be returned");
    assert_eq!(allowed_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn ping_requires_bearer_token_when_env_token_configured_negative_path() {
    let app = build_router(GatewayState::test_with_bearer(Some("tok-1")));
    let request = Request::builder()
        .method("POST")
        .uri("/v1/ping")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"message":"hi"}"#))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn websocket_route_requires_upgrade_headers_negative_path() {
    let app = build_router(GatewayState::test_with_bearer(None));
    let request = Request::builder()
        .method("GET")
        .uri("/ws/chat")
        .header("connection", "upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);
}

#[tokio::test]
async fn webhook_echo_channel_returns_delivery_success_path() {
    let app = build_router(GatewayState::test_with_bearer(None));
    let request = Request::builder()
        .method("POST")
        .uri("/v1/webhook/echo")
        .header("content-type", "application/json")
        .body(Body::from(json!({"message": "hello"}).to_string()))
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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("body should be valid json");
    assert_eq!(json["accepted"], serde_json::Value::Bool(true));
    assert_eq!(
        json["channel"],
        serde_json::Value::String("echo".to_string())
    );
}

#[tokio::test]
async fn pair_rejected_when_pairing_code_not_active_negative_path() {
    let app = build_router(GatewayState::test_with_existing_pair("tok-existing"));
    let request = Request::builder()
        .method("POST")
        .uri("/pair")
        .header("x-pairing-code", "406823")
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
