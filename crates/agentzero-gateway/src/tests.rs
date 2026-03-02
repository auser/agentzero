use crate::middleware::MiddlewareConfig;
use crate::router::build_router;
use crate::state::GatewayState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

fn default_config() -> MiddlewareConfig {
    MiddlewareConfig::default()
}

#[tokio::test]
async fn pair_rejects_wrong_pairing_code_negative_path() {
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());
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
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());

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
    let app = build_router(
        GatewayState::test_with_bearer(Some("tok-1")),
        &default_config(),
    );
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
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());
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
async fn webhook_cli_channel_returns_delivery_success_path() {
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/webhook/cli")
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
        serde_json::Value::String("cli".to_string())
    );
}

// --- Middleware integration tests ---

#[tokio::test]
async fn health_endpoint_returns_ok_success_path() {
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());
    let request = Request::builder()
        .method("GET")
        .uri("/health")
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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("should be json");
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn request_size_limit_rejects_oversized_body_negative_path() {
    let config = MiddlewareConfig {
        max_body_bytes: 100,
        ..Default::default()
    };
    let app = build_router(GatewayState::test_with_bearer(None), &config);

    let big_body = "x".repeat(200);
    let request = Request::builder()
        .method("POST")
        .uri("/v1/webhook/cli")
        .header("content-type", "application/json")
        .header("content-length", big_body.len().to_string())
        .body(Body::from(big_body))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn rate_limit_rejects_excess_requests_negative_path() {
    let config = MiddlewareConfig {
        rate_limit_max: 2,
        rate_limit_window_secs: 60,
        ..Default::default()
    };
    let app = build_router(GatewayState::test_with_bearer(None), &config);

    // First two requests should succeed
    for _ in 0..2 {
        let req = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .expect("request should build");
        let resp = app.clone().oneshot(req).await.expect("should respond");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // Third request should be rate limited
    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .expect("request should build");
    let resp = app.oneshot(req).await.expect("should respond");
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn cors_preflight_returns_allowed_headers_success_path() {
    let config = MiddlewareConfig {
        cors_allowed_origins: vec!["https://example.com".to_string()],
        ..Default::default()
    };
    let app = build_router(GatewayState::test_with_bearer(None), &config);

    let request = Request::builder()
        .method("OPTIONS")
        .uri("/health")
        .header("origin", "https://example.com")
        .header("access-control-request-method", "GET")
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(response
        .headers()
        .get("access-control-allow-origin")
        .is_some());
}

#[tokio::test]
async fn pair_rejected_when_pairing_code_not_active_negative_path() {
    let app = build_router(
        GatewayState::test_with_existing_pair("tok-existing"),
        &default_config(),
    );
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

// --- Dashboard ---

#[tokio::test]
async fn dashboard_returns_html() {
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());
    let request = Request::builder()
        .method("GET")
        .uri("/")
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
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("<html>"));
    assert!(html.contains("agentzero-gateway"));
}

// --- Metrics ---

#[tokio::test]
async fn metrics_returns_prometheus_format() {
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());
    let request = Request::builder()
        .method("GET")
        .uri("/metrics")
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
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("# HELP"));
    assert!(text.contains("# TYPE"));
    assert!(text.contains("agentzero_gateway_requests_total"));
}

// --- v1_models ---

#[tokio::test]
async fn v1_models_returns_model_list() {
    // Open mode (no bearer, no paired tokens) so auth passes.
    let state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/models")
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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("should be json");
    assert_eq!(json["object"], "list");
    assert!(json["data"].as_array().unwrap().len() >= 2);
}

// --- Ping success ---

#[tokio::test]
async fn ping_with_valid_token_returns_echo() {
    let app = build_router(
        GatewayState::test_with_bearer(Some("tok-ping")),
        &default_config(),
    );
    let request = Request::builder()
        .method("POST")
        .uri("/v1/ping")
        .header("content-type", "application/json")
        .header("authorization", "Bearer tok-ping")
        .body(Body::from(r#"{"message":"hello"}"#))
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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("should be json");
    assert_eq!(json["ok"], true);
    assert_eq!(json["echo"], "hello");
}

// --- api_chat ---

#[tokio::test]
async fn api_chat_returns_echo_reply() {
    let state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("POST")
        .uri("/api/chat")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"message":"world"}"#))
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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("should be json");
    assert!(json["message"].as_str().unwrap().contains("world"));
}

// --- v1_chat_completions ---

#[tokio::test]
async fn v1_chat_completions_returns_openai_format() {
    let state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let body = serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [
            {"role": "user", "content": "ping"}
        ]
    });

    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::OK);

    let resp_body = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&resp_body).expect("should be json");
    assert_eq!(json["object"], "chat.completion");
    assert!(json["id"].as_str().unwrap().starts_with("chatcmpl-"));
    let choices = json["choices"].as_array().unwrap();
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0]["finish_reason"], "stop");
    assert!(choices[0]["message"]["content"]
        .as_str()
        .unwrap()
        .contains("ping"));
}

// --- legacy_webhook ---

#[tokio::test]
async fn legacy_webhook_returns_echo() {
    let state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("POST")
        .uri("/webhook")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"message":"test-msg"}"#))
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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("should be json");
    assert!(json["message"].as_str().unwrap().contains("echo: test-msg"));
}

// --- Webhook unknown channel ---

#[tokio::test]
async fn webhook_unknown_channel_returns_404() {
    let state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("POST")
        .uri("/v1/webhook/nonexistent-channel")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"payload":"test"}"#))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// --- Pair missing header ---

#[tokio::test]
async fn pair_missing_pairing_header_returns_401() {
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());
    let request = Request::builder()
        .method("POST")
        .uri("/pair")
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// --- v1_models with auth required ---

#[tokio::test]
async fn v1_models_requires_auth_when_bearer_set() {
    let app = build_router(
        GatewayState::test_with_bearer(Some("secret-tok")),
        &default_config(),
    );
    let request = Request::builder()
        .method("GET")
        .uri("/v1/models")
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
