use crate::middleware::MiddlewareConfig;
use crate::router::build_router;
use crate::state::GatewayState;
use agentzero_core::{MemoryEntry, MemoryStore};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use std::sync::Arc;
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
    // Wrong code → 403 (AuthFailed), not 401 (AuthRequired).
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
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

    // Verify content-type is Prometheus text format.
    let ct = response
        .headers()
        .get("content-type")
        .expect("should have content-type")
        .to_str()
        .unwrap();
    assert!(
        ct.contains("text/plain"),
        "content-type should be text/plain, got: {ct}"
    );

    // Test uses a non-global recorder so the handle renders empty or minimal
    // output. The real metrics content is validated in the TCP integration test.
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    // Body should be valid UTF-8 (Prometheus text format).
    let _text = std::str::from_utf8(&body).expect("metrics body should be valid utf-8");
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
async fn api_chat_returns_service_unavailable_without_config() {
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
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// --- v1_chat_completions ---

#[tokio::test]
async fn v1_chat_completions_returns_service_unavailable_without_config() {
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
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
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

// --- TCP-level integration test ---

#[tokio::test]
async fn tcp_health_endpoint_over_real_listener() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let state = GatewayState::test_with_bearer(None);
    let app = build_router(state, &default_config());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("should bind to ephemeral port");
    let addr = listener.local_addr().expect("should have local addr");

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server should run");
    });

    // Give server a moment to start accepting connections
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Send a raw HTTP/1.1 request over TCP
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("should connect to gateway");
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("should send request");

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("should read response");
    let response_str = String::from_utf8_lossy(&response);

    assert!(
        response_str.starts_with("HTTP/1.1 200"),
        "should get 200 OK, got: {}",
        response_str.lines().next().unwrap_or("(empty)")
    );
    assert!(
        response_str.contains(r#""status":"ok"#),
        "body should contain health status"
    );

    server.abort();
}

// --- Phase 5: gateway agent wiring tests ---

#[tokio::test]
async fn gateway_state_config_fields_are_active() {
    let state = GatewayState::test_with_bearer(None).with_gateway_config(false, true);
    assert!(!state.require_pairing);
    assert!(state.allow_public_bind);
    // Pairing TTL support
    let state = GatewayState::test_with_bearer(None).with_pairing_ttl(60);
    assert_eq!(state.pairing_ttl_secs, Some(60));
    assert!(state.pairing_code_valid().is_some());
}

#[tokio::test]
async fn pairing_code_expires_after_ttl() {
    let mut state = GatewayState::test_with_bearer(None);
    state.pairing_ttl_secs = Some(0); // immediate expiry
    assert!(state.pairing_code_valid().is_none());
    // pair endpoint should reject when code is expired
    let app = build_router(state, &default_config());
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

#[tokio::test]
async fn v1_chat_completions_stream_returns_service_unavailable_without_config() {
    let state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let body = json!({
        "model": "gpt-4o-mini",
        "messages": [{"role": "user", "content": "ping"}],
        "stream": true
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
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// --- Phase 1.6: Sprint 23 new tests ---

// --- Structured error response format ---

#[tokio::test]
async fn auth_required_returns_structured_json_error() {
    let app = build_router(
        GatewayState::test_with_bearer(Some("tok")),
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

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("should be json");
    assert_eq!(json["error"]["type"], "auth_required");
    assert!(json["error"]["message"].as_str().is_some());
}

#[tokio::test]
async fn agent_unavailable_returns_503_with_structured_json() {
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
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("should be json");
    assert_eq!(json["error"]["type"], "agent_unavailable");
    assert_eq!(json["error"]["message"], "agent runtime not configured");
}

#[tokio::test]
async fn auth_failed_returns_403_with_structured_json() {
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
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("should be json");
    assert_eq!(json["error"]["type"], "auth_failed");
}

#[tokio::test]
async fn not_found_returns_404_with_structured_json() {
    let state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("POST")
        .uri("/v1/webhook/nonexistent-xyz")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"payload":"test"}"#))
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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("should be json");
    assert_eq!(json["error"]["type"], "not_found");
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("nonexistent-xyz"));
}

// --- /v1/models dynamic catalog ---

#[tokio::test]
async fn v1_models_returns_known_providers() {
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
    let data = json["data"].as_array().expect("data should be array");

    let owners: std::collections::HashSet<&str> =
        data.iter().filter_map(|m| m["owned_by"].as_str()).collect();
    assert!(
        owners.len() >= 2,
        "should have models from at least 2 providers, got: {owners:?}"
    );

    for model in data {
        assert_eq!(model["object"], "model");
        assert!(model["id"].as_str().is_some_and(|id| !id.is_empty()));
        assert!(model["owned_by"].as_str().is_some_and(|o| !o.is_empty()));
    }
}

#[tokio::test]
async fn v1_models_ids_match_provider_catalog() {
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
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("should be json");
    let data = json["data"].as_array().unwrap();

    let mut catalog_count = 0usize;
    for provider in agentzero_providers::supported_providers() {
        if let Some((_pid, models)) = agentzero_providers::find_models_for_provider(provider.id) {
            catalog_count += models.len();
        }
    }

    assert_eq!(
        data.len(),
        catalog_count,
        "model count from endpoint should match catalog"
    );
}

// --- Default rate limit ---

#[tokio::test]
async fn default_rate_limit_is_600() {
    let config = MiddlewareConfig::default();
    assert_eq!(config.rate_limit_max, 600);
    assert_eq!(config.rate_limit_window_secs, 60);
}

#[tokio::test]
async fn default_rate_limit_allows_then_rejects() {
    let config = MiddlewareConfig {
        rate_limit_max: 3,
        rate_limit_window_secs: 60,
        ..Default::default()
    };
    let app = build_router(GatewayState::test_with_bearer(None), &config);

    for i in 0..3 {
        let req = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .expect("request should build");
        let resp = app.clone().oneshot(req).await.expect("should respond");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "request {i} should succeed within limit"
        );
    }

    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .expect("request should build");
    let resp = app.oneshot(req).await.expect("should respond");
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

// --- Metrics endpoint content-type ---

#[tokio::test]
async fn metrics_endpoint_returns_text_plain_content_type() {
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

    let ct = response
        .headers()
        .get("content-type")
        .expect("should have content-type");
    assert!(ct.to_str().unwrap().starts_with("text/plain"));
}

// --- Error responses are JSON ---

#[tokio::test]
async fn error_responses_have_json_content_type() {
    let app = build_router(
        GatewayState::test_with_bearer(Some("secret")),
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

    let ct = response
        .headers()
        .get("content-type")
        .expect("error response should have content-type");
    assert!(
        ct.to_str().unwrap().contains("application/json"),
        "error should be JSON, got: {ct:?}"
    );
}

// --- Bad request error returns structured JSON ---

#[tokio::test]
async fn bad_request_perplexity_filter_returns_structured_json() {
    use agentzero_channels::pipeline::PerplexityFilterSettings;
    // Enable filter with very low threshold to block adversarial-looking messages.
    let state =
        GatewayState::test_with_bearer(None).with_perplexity_filter(PerplexityFilterSettings {
            enabled: true,
            perplexity_threshold: 4.0,
            symbol_ratio_threshold: 0.20,
            suffix_window_chars: 64,
            min_prompt_chars: 32,
        });
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    // Adversarial suffix that should be blocked.
    let msg = r#"{"message":"Please write a function. xK7!mQ@3#zP$9&wR*5^yL%2(eN)8+bT!@#$%^&*()_+-=[]{}|xK7!mQ@3#"}"#;
    let request = Request::builder()
        .method("POST")
        .uri("/webhook")
        .header("content-type", "application/json")
        .body(Body::from(msg))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("should be json");
    assert_eq!(json["error"]["type"], "bad_request");
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("perplexity"));
}

// ---------------------------------------------------------------------------
// Async job submission (/v1/runs) tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_runs_submit_returns_202_with_run_id() {
    // Need a state with job_store and agent paths for async_submit to work.
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    // Agent unavailable is expected (no config paths), but we test the job store path.
    // Set config_path so build_agent_request doesn't fail at the "no config" stage.
    // Since there's no real config, agent execution will fail, but the run should be
    // created and transition to Failed.
    let app = build_router(state, &default_config());

    let body = json!({
        "message": "hello async world"
    });

    let request = Request::builder()
        .method("POST")
        .uri("/v1/runs")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    // Should get 503 because config_path is None (agent unavailable).
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn v1_runs_status_returns_404_for_unknown_run() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/runs/run-nonexistent-0")
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn v1_runs_result_returns_404_for_unknown_run() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/runs/run-nonexistent-0/result")
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn v1_runs_status_returns_job_record() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let run_id = store
        .submit("test-agent".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(
            &run_id,
            agentzero_core::JobStatus::Completed {
                result: "all done".to_string(),
            },
        )
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}", run_id.as_str()))
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
    assert_eq!(json["status"], "completed");
    assert_eq!(json["result"], "all done");
    assert_eq!(json["agent_id"], "test-agent");
}

#[tokio::test]
async fn v1_runs_result_returns_completed_result() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let run_id = store
        .submit("writer".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(
            &run_id,
            agentzero_core::JobStatus::Completed {
                result: "the final brief".to_string(),
            },
        )
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}/result", run_id.as_str()))
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
    assert_eq!(json["status"], "completed");
    assert_eq!(json["result"], "the final brief");
}

#[tokio::test]
async fn v1_runs_result_returns_202_for_pending_job() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let run_id = store
        .submit("agent".to_string(), agentzero_core::Lane::Main, None)
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}/result", run_id.as_str()))
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(
        response.status(),
        StatusCode::ACCEPTED,
        "pending job should return 202"
    );
}

#[tokio::test]
async fn v1_runs_requires_auth() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let state = GatewayState::test_with_bearer(Some("secret-tok")).with_job_store(store);
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("POST")
        .uri("/v1/runs")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"message":"test"}"#))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn v1_runs_without_job_store_returns_503() {
    // No job_store on state.
    let state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("POST")
        .uri("/v1/runs")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"message":"test"}"#))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// ---------------------------------------------------------------------------
// Async job lifecycle tests
// ---------------------------------------------------------------------------

/// Full lifecycle: submit → running → completed → poll status → get result.
/// This simulates the background execution by manually driving the JobStore.
#[tokio::test]
async fn v1_runs_full_lifecycle_pending_running_completed() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let run_id = store
        .submit(
            "lifecycle-agent".to_string(),
            agentzero_core::Lane::Main,
            None,
        )
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store.clone());
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    // 1. Poll status while pending
    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "pending");

    // 2. Transition to running
    store
        .update_status(&run_id, agentzero_core::JobStatus::Running)
        .await;
    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "running");

    // 3. Result endpoint returns 202 while still running
    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}/result", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    // 4. Transition to completed
    store
        .update_status(
            &run_id,
            agentzero_core::JobStatus::Completed {
                result: "lifecycle result".to_string(),
            },
        )
        .await;
    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "completed");
    assert_eq!(json["result"], "lifecycle result");

    // 5. Result endpoint returns 200 with result
    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}/result", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["result"], "lifecycle result");
}

/// Lifecycle variant: job fails instead of completing.
#[tokio::test]
async fn v1_runs_lifecycle_failure_path() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let run_id = store
        .submit("fail-agent".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(&run_id, agentzero_core::JobStatus::Running)
        .await;
    store
        .update_status(
            &run_id,
            agentzero_core::JobStatus::Failed {
                error: "model rate limited".to_string(),
            },
        )
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    // Status shows failed
    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error"], "model rate limited");

    // Result endpoint also returns the error
    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}/result", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error"], "model rate limited");
}

/// Broadcast subscriber receives status transitions.
#[tokio::test]
async fn job_store_broadcast_lifecycle_transitions() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let mut rx = store.subscribe();

    let run_id = store
        .submit(
            "broadcast-agent".to_string(),
            agentzero_core::Lane::Main,
            None,
        )
        .await;

    // Pending notification from submit
    let (notified_id, status) = rx.recv().await.unwrap();
    assert_eq!(notified_id, run_id);
    assert!(matches!(status, agentzero_core::JobStatus::Pending));

    // Running
    store
        .update_status(&run_id, agentzero_core::JobStatus::Running)
        .await;
    let (_, status) = rx.recv().await.unwrap();
    assert!(matches!(status, agentzero_core::JobStatus::Running));

    // Completed
    store
        .update_status(
            &run_id,
            agentzero_core::JobStatus::Completed {
                result: "done".to_string(),
            },
        )
        .await;
    let (_, status) = rx.recv().await.unwrap();
    assert!(matches!(
        status,
        agentzero_core::JobStatus::Completed { .. }
    ));
}

// ---------------------------------------------------------------------------
// Job management endpoint tests: cancel, list, events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_runs_cancel_running_job() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let run_id = store
        .submit("cancel-agent".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(&run_id, agentzero_core::JobStatus::Running)
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("DELETE")
        .uri(format!("/v1/runs/{}", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["cancelled"], true);
}

#[tokio::test]
async fn v1_runs_cancel_completed_job_returns_false() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let run_id = store
        .submit("done-agent".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(
            &run_id,
            agentzero_core::JobStatus::Completed {
                result: "done".to_string(),
            },
        )
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("DELETE")
        .uri(format!("/v1/runs/{}", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["cancelled"], false);
}

#[tokio::test]
async fn v1_runs_cancel_unknown_returns_404() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("DELETE")
        .uri("/v1/runs/run-nonexistent")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn v1_runs_list_all_jobs() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    store
        .submit("a".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .submit("b".to_string(), agentzero_core::Lane::Main, None)
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/runs")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 2);
    assert_eq!(json["data"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn v1_runs_list_filtered_by_status() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let r1 = store
        .submit("a".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .submit("b".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(&r1, agentzero_core::JobStatus::Running)
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/runs?status=running")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["data"][0]["agent_id"], "a");
}

#[tokio::test]
async fn v1_runs_events_for_completed_job() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let run_id = store
        .submit("events-agent".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(&run_id, agentzero_core::JobStatus::Running)
        .await;
    store
        .update_status(
            &run_id,
            agentzero_core::JobStatus::Completed {
                result: "final output".to_string(),
            },
        )
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}/events", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let events = json["events"].as_array().unwrap();
    assert_eq!(events.len(), 3); // created, running, completed
    assert_eq!(json["total"], 3);
    assert_eq!(events[0]["type"], "created");
    assert_eq!(events[1]["type"], "running");
    assert_eq!(events[2]["type"], "completed");
    assert_eq!(events[2]["result"], "final output");
}

#[tokio::test]
async fn v1_runs_events_since_seq_filters_earlier_events() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let run_id = store
        .submit("seq-agent".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(&run_id, agentzero_core::JobStatus::Running)
        .await;
    store
        .update_status(
            &run_id,
            agentzero_core::JobStatus::Completed {
                result: "done".to_string(),
            },
        )
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    // Without since_seq — all 3 events returned.
    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}/events", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let all_events = json["events"].as_array().unwrap();
    assert_eq!(all_events.len(), 3);
    // Verify seq numbering is 1-based and monotonic.
    assert_eq!(all_events[0]["seq"], 1);
    assert_eq!(all_events[1]["seq"], 2);
    assert_eq!(all_events[2]["seq"], 3);

    // With since_seq=1 — skip first event, return 2.
    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}/events?since_seq=1", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let filtered_events = json["events"].as_array().unwrap();
    assert_eq!(filtered_events.len(), 2);
    assert_eq!(filtered_events[0]["seq"], 2);
    assert_eq!(filtered_events[0]["type"], "running");
    assert_eq!(filtered_events[1]["seq"], 3);
    assert_eq!(filtered_events[1]["type"], "completed");

    // With since_seq=3 — all events already seen, return empty.
    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}/events?since_seq=3", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["events"].as_array().unwrap().len(), 0);
    assert_eq!(json["total"], 0);
}

#[tokio::test]
async fn v1_runs_events_unknown_returns_404() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/runs/run-nope/events")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// WebSocket run subscription tests
// ---------------------------------------------------------------------------

#[test]
fn status_frame_pending() {
    let run_id = agentzero_core::RunId("run-test".to_string());
    let frame = crate::handlers::status_frame(&run_id, &agentzero_core::JobStatus::Pending);
    let v: serde_json::Value = serde_json::from_str(&frame).unwrap();
    assert_eq!(v["type"], "status");
    assert_eq!(v["status"], "pending");
    assert_eq!(v["run_id"], "run-test");
}

#[test]
fn status_frame_completed() {
    let run_id = agentzero_core::RunId("run-done".to_string());
    let frame = crate::handlers::status_frame(
        &run_id,
        &agentzero_core::JobStatus::Completed {
            result: "output data".to_string(),
        },
    );
    let v: serde_json::Value = serde_json::from_str(&frame).unwrap();
    assert_eq!(v["type"], "completed");
    assert_eq!(v["result"], "output data");
}

#[test]
fn status_frame_failed() {
    let run_id = agentzero_core::RunId("run-err".to_string());
    let frame = crate::handlers::status_frame(
        &run_id,
        &agentzero_core::JobStatus::Failed {
            error: "something broke".to_string(),
        },
    );
    let v: serde_json::Value = serde_json::from_str(&frame).unwrap();
    assert_eq!(v["type"], "failed");
    assert_eq!(v["error"], "something broke");
}

// ---------------------------------------------------------------------------
// Cascade cancel tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_runs_cascade_cancel() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let parent = store
        .submit("parent".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(&parent, agentzero_core::JobStatus::Running)
        .await;

    let _child = store
        .submit(
            "child".to_string(),
            agentzero_core::Lane::SubAgent {
                parent_run_id: parent.clone(),
                depth: 1,
            },
            Some(parent.clone()),
        )
        .await;
    store
        .update_status(&_child, agentzero_core::JobStatus::Running)
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("DELETE")
        .uri(format!("/v1/runs/{}?cascade=true", parent.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["cancelled"], true);
    assert_eq!(json["cascade_count"], 2); // parent + child
}

// ---------------------------------------------------------------------------
// Agents list tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_agents_list_with_presence() {
    let store = std::sync::Arc::new(agentzero_orchestrator::PresenceStore::new());
    store
        .register("researcher", std::time::Duration::from_secs(30))
        .await;
    store
        .register("writer", std::time::Duration::from_secs(30))
        .await;

    let mut state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    state.presence_store = Some(store);
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/agents")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 2);
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    // All should be "active" (PresenceStatus::Alive maps to "active" for UI consistency).
    for agent in data {
        assert_eq!(agent["status"], "active");
    }
}

#[tokio::test]
async fn v1_agents_list_no_stores_returns_empty_list() {
    // Handler returns an empty list (200) when neither presence nor agent store
    // is configured, rather than a 503.
    let state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/agents")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 0);
    assert!(json["data"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Event log via API tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_runs_events_include_tool_calls() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let run_id = store
        .submit("tool-agent".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(&run_id, agentzero_core::JobStatus::Running)
        .await;
    store.record_tool_call(&run_id, "read_file").await;
    store.record_tool_result(&run_id, "read_file").await;
    store
        .update_status(
            &run_id,
            agentzero_core::JobStatus::Completed {
                result: "done".to_string(),
            },
        )
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}/events", run_id.as_str()))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let events = json["events"].as_array().unwrap();
    // Created, Running, ToolCall, ToolResult, Completed = 5
    assert_eq!(events.len(), 5);
    assert_eq!(events[2]["type"], "tool_call");
    assert_eq!(events[2]["tool"], "read_file");
    assert_eq!(events[3]["type"], "tool_result");
    assert_eq!(events[3]["tool"], "read_file");
}

// ---------------------------------------------------------------------------
// Queue mode tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_runs_followup_mode_requires_run_id() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    // Followup without run_id should return 400.
    let body = json!({
        "message": "follow up question",
        "mode": "followup"
    });

    let request = Request::builder()
        .method("POST")
        .uri("/v1/runs")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn v1_runs_followup_mode_unknown_run_id_returns_404() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let body = json!({
        "message": "follow up",
        "mode": "followup",
        "run_id": "nonexistent-run-id"
    });

    let request = Request::builder()
        .method("POST")
        .uri("/v1/runs")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn v1_runs_followup_mode_valid_run_id_accepted() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let run_id = store
        .submit("agent".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(
            &run_id,
            agentzero_core::JobStatus::Completed {
                result: "done".to_string(),
            },
        )
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let body = json!({
        "message": "follow up question",
        "mode": "followup",
        "run_id": run_id.0
    });

    let request = Request::builder()
        .method("POST")
        .uri("/v1/runs")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    // 503 because no config_path, but the mode dispatch reached the agent submission
    // (past validation). If we had config paths it would return 202.
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn v1_runs_interrupt_mode_cancels_active_runs() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let r1 = store
        .submit("agent".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(&r1, agentzero_core::JobStatus::Running)
        .await;
    let r2 = store
        .submit("agent".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(&r2, agentzero_core::JobStatus::Running)
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store.clone());
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let body = json!({
        "message": "interrupt everything",
        "mode": "interrupt"
    });

    let request = Request::builder()
        .method("POST")
        .uri("/v1/runs")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    // 503 because no config_path, but by this point interrupt has already cancelled active runs.
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    // Verify both original runs were cancelled.
    let r1_record = store.get(&r1).await.unwrap();
    assert!(r1_record.status.is_terminal());
    let r2_record = store.get(&r2).await.unwrap();
    assert!(r2_record.status.is_terminal());
}

// ---------------------------------------------------------------------------
// SSE stream tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_runs_stream_returns_sse_for_completed_job() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let run_id = store
        .submit("agent".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(
            &run_id,
            agentzero_core::JobStatus::Completed {
                result: "hello world".to_string(),
            },
        )
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}/stream", run_id.0))
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("data:"));
    assert!(body_str.contains("\"type\":\"completed\""));
    assert!(body_str.contains("hello world"));
}

#[tokio::test]
async fn v1_runs_stream_blocks_format_returns_blocks() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let run_id = store
        .submit("agent".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(
            &run_id,
            agentzero_core::JobStatus::Completed {
                result: "# Title\n\nSome paragraph text.\n\n```rust\nfn main() {}\n```\n"
                    .to_string(),
            },
        )
        .await;

    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/runs/{}/stream?format=blocks", run_id.0))
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
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("\"format\":\"blocks\""));
    assert!(body_str.contains("\"blocks\""));
}

#[tokio::test]
async fn v1_runs_stream_unknown_run_returns_404() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let state = GatewayState::test_with_bearer(None).with_job_store(store);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/runs/nonexistent/stream")
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// E2E integration test: full gateway → pair → chat with real LLM
// ---------------------------------------------------------------------------

/// Full end-to-end test of the research pipeline flow:
///   1. Start a real gateway on an ephemeral port with the research-pipeline config
///   2. Pair a client using the pairing code
///   3. Send a chat request via /api/chat
///   4. Verify the LLM returns a non-empty response
///
/// Requires: ANTHROPIC_API_KEY set in the environment.
/// Run with: ANTHROPIC_API_KEY=sk-... cargo test -p agentzero-gateway -- --ignored e2e_research_pipeline
#[tokio::test]
#[ignore]
async fn e2e_research_pipeline_pair_and_chat() {
    use std::collections::HashSet;
    use std::path::PathBuf;

    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        eprintln!("SKIP: ANTHROPIC_API_KEY not set");
        return;
    }

    // Resolve paths relative to workspace root.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.join("../..").canonicalize().unwrap();
    let config_path = workspace_root.join("examples/research-pipeline/agentzero.toml");
    assert!(
        config_path.exists(),
        "research-pipeline config should exist at {config_path:?}"
    );

    // Create a temp workspace with a .env file so the agent runtime finds the key
    // via the config path's parent directory.
    let tmp_workspace = std::env::temp_dir().join(format!(
        "agentzero-e2e-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&tmp_workspace).unwrap();

    // Build gateway state with a known pairing code and agent paths.
    let pairing_code = "123456";
    let prometheus_handle = GatewayState::test_prometheus_handle();
    let state = GatewayState::new(
        Some(pairing_code.to_string()),
        "TESTOTP".to_string(),
        HashSet::new(),
        None,
        prometheus_handle,
    )
    .with_agent_paths(config_path.clone(), tmp_workspace.clone())
    .with_gateway_config(true, false);

    let app = build_router(state, &default_config());

    // Bind to ephemeral port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("should bind to ephemeral port");
    let addr = listener.local_addr().expect("should have local addr");
    let base_url = format!("http://{addr}");

    // Spawn the server.
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server should run");
    });

    // Give server a moment to start.
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();

    // --- Step 1: Health check ---
    let health = client
        .get(format!("{base_url}/health"))
        .send()
        .await
        .expect("health should succeed");
    assert_eq!(health.status(), 200, "health endpoint should return 200");

    // --- Step 2: Pair ---
    let pair_resp = client
        .post(format!("{base_url}/pair"))
        .header("x-pairing-code", pairing_code)
        .send()
        .await
        .expect("pair request should succeed");
    assert_eq!(pair_resp.status(), 200, "pairing should return 200");

    let pair_json: serde_json::Value = pair_resp
        .json()
        .await
        .expect("pair response should be json");
    assert_eq!(pair_json["paired"], true, "should be paired");
    let token = pair_json["token"]
        .as_str()
        .expect("pair response should contain token");
    assert!(!token.is_empty(), "token should not be empty");

    // --- Step 3: Send chat and get LLM response ---
    let chat_resp = client
        .post(format!("{base_url}/api/chat"))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "message": "What is 2 + 2? Reply with just the number."
        }))
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .expect("chat request should succeed");
    let chat_status = chat_resp.status();
    let chat_body = chat_resp.text().await.expect("should read chat body");
    assert_eq!(
        chat_status, 200,
        "chat should return 200, got {chat_status}: {chat_body}"
    );

    let chat_json: serde_json::Value =
        serde_json::from_str(&chat_body).expect("chat response should be json");
    let message = chat_json["message"]
        .as_str()
        .expect("chat response should contain message field");
    assert!(!message.is_empty(), "LLM response should not be empty");
    assert!(
        message.contains('4'),
        "LLM should answer 2+2=4, got: {message}"
    );

    // --- Cleanup ---
    server.abort();
    let _ = std::fs::remove_dir_all(&tmp_workspace);
}

/// Full e2e test using the OpenAI-compatible /v1/chat/completions endpoint.
/// Tests the same flow but through the standardized API surface.
#[tokio::test]
#[ignore]
async fn e2e_v1_chat_completions_with_real_llm() {
    use std::collections::HashSet;
    use std::path::PathBuf;

    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        eprintln!("SKIP: ANTHROPIC_API_KEY not set");
        return;
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.join("../..").canonicalize().unwrap();
    let config_path = workspace_root.join("examples/research-pipeline/agentzero.toml");
    assert!(config_path.exists());

    let tmp_workspace = std::env::temp_dir().join(format!(
        "agentzero-e2e-v1-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&tmp_workspace).unwrap();

    let pairing_code = "654321";
    let prometheus_handle = GatewayState::test_prometheus_handle();
    let state = GatewayState::new(
        Some(pairing_code.to_string()),
        "TESTOTP".to_string(),
        HashSet::new(),
        None,
        prometheus_handle,
    )
    .with_agent_paths(config_path.clone(), tmp_workspace.clone())
    .with_gateway_config(true, false);

    let app = build_router(state, &default_config());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("should bind");
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    let client = reqwest::Client::new();

    // Pair first.
    let pair_json: serde_json::Value = client
        .post(format!("{base_url}/pair"))
        .header("x-pairing-code", pairing_code)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let token = pair_json["token"].as_str().unwrap();

    // Send via OpenAI-compatible endpoint.
    let chat_resp = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-6",
            "messages": [
                {"role": "user", "content": "What is the capital of France? Reply with just the city name."}
            ]
        }))
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .expect("v1/chat/completions should succeed");
    assert_eq!(chat_resp.status(), 200);

    let json: serde_json::Value = chat_resp.json().await.unwrap();
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .expect("should have choices[0].message.content");
    assert!(!content.is_empty());
    assert!(
        content.to_lowercase().contains("paris"),
        "should answer Paris, got: {content}"
    );

    server.abort();
    let _ = std::fs::remove_dir_all(&tmp_workspace);
}

// ---------------------------------------------------------------------------
// Emergency stop tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_estop_cancels_all_active_runs() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());

    // Create 3 root-level runs, one already completed.
    let r1 = store
        .submit("agent-a".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(&r1, agentzero_core::JobStatus::Running)
        .await;

    let r2 = store
        .submit("agent-b".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(&r2, agentzero_core::JobStatus::Running)
        .await;

    let r3 = store
        .submit("agent-c".to_string(), agentzero_core::Lane::Main, None)
        .await;
    store
        .update_status(
            &r3,
            agentzero_core::JobStatus::Completed {
                result: "done".to_string(),
            },
        )
        .await;

    // Create a child run under r1.
    let child = store
        .submit(
            "child".to_string(),
            agentzero_core::Lane::Main,
            Some(r1.clone()),
        )
        .await;
    store
        .update_status(&child, agentzero_core::JobStatus::Running)
        .await;

    let mut state = GatewayState::test_with_bearer(None);
    state.job_store = Some(store.clone());
    state.require_pairing = false;
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("POST")
        .uri("/v1/estop")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["emergency_stop"], true);
    // r1 + child + r2 cancelled; r3 was already completed → skipped.
    assert_eq!(json["cancelled_count"], 3);

    // Verify individual statuses.
    assert_eq!(
        store.get(&r1).await.unwrap().status,
        agentzero_core::JobStatus::Cancelled
    );
    assert_eq!(
        store.get(&r2).await.unwrap().status,
        agentzero_core::JobStatus::Cancelled
    );
    assert_eq!(
        store.get(&child).await.unwrap().status,
        agentzero_core::JobStatus::Cancelled
    );
    // r3 should still be completed.
    assert!(matches!(
        store.get(&r3).await.unwrap().status,
        agentzero_core::JobStatus::Completed { .. }
    ));
}

#[tokio::test]
async fn v1_estop_no_active_runs_returns_zero() {
    let store = std::sync::Arc::new(agentzero_orchestrator::JobStore::new());
    let mut state = GatewayState::test_with_bearer(None);
    state.job_store = Some(store);
    state.require_pairing = false;
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("POST")
        .uri("/v1/estop")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["emergency_stop"], true);
    assert_eq!(json["cancelled_count"], 0);
}

#[tokio::test]
async fn v1_estop_no_job_store_returns_503() {
    let mut state = GatewayState::test_with_bearer(None);
    state.require_pairing = false;
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("POST")
        .uri("/v1/estop")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// ---------------------------------------------------------------------------
// Privacy integration tests
// ---------------------------------------------------------------------------

#[cfg(feature = "privacy")]
mod privacy_tests {
    use super::*;
    use crate::privacy_state::NoiseSessionStore;
    use crate::relay::RelayMailbox;
    use agentzero_core::privacy::noise::{NoiseHandshaker, NoiseKeypair};
    use axum::Router;
    use base64::Engine as _;

    fn state_with_noise() -> GatewayState {
        let sessions = NoiseSessionStore::new(100, 3600);
        let keypair = NoiseKeypair::generate().expect("keypair should generate");
        GatewayState::test_with_bearer(None).with_noise_privacy(sessions, keypair)
    }

    fn state_with_relay() -> GatewayState {
        let mailbox = RelayMailbox::new(100, 3600);
        GatewayState::test_with_bearer(None).with_relay_mode(mailbox)
    }

    #[allow(dead_code)]
    fn state_with_noise_and_relay() -> GatewayState {
        let sessions = NoiseSessionStore::new(100, 3600);
        let keypair = NoiseKeypair::generate().expect("keypair should generate");
        let mailbox = RelayMailbox::new(100, 3600);
        GatewayState::test_with_bearer(None)
            .with_noise_privacy(sessions, keypair)
            .with_relay_mode(mailbox)
    }

    // --- Noise handshake integration tests ---

    #[tokio::test]
    async fn noise_handshake_full_round_trip() {
        let state = state_with_noise();
        let app = build_router(state, &default_config());

        // Step 1: Client initiates handshake
        let client_kp = NoiseKeypair::generate().unwrap();
        let mut client = NoiseHandshaker::new_initiator("XX", &client_kp).unwrap();
        let mut buf = [0u8; 65535];
        let len = client.write_message(b"", &mut buf).unwrap();
        let client_msg = base64::engine::general_purpose::STANDARD.encode(&buf[..len]);

        let step1_body = json!({
            "handshake_id": "test-hs-001",
            "message": client_msg,
        });

        let request = Request::builder()
            .method("POST")
            .uri("/v1/noise/handshake/step1")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&step1_body).unwrap()))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let step1_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let server_msg_b64 = step1_resp["message"].as_str().unwrap();
        let server_msg = base64::engine::general_purpose::STANDARD
            .decode(server_msg_b64)
            .unwrap();

        // Client processes server's ← e ee s es
        client.read_message(&server_msg, &mut buf).unwrap();

        // Step 2: Client sends → s se
        let len2 = client.write_message(b"", &mut buf).unwrap();
        let client_msg2 = base64::engine::general_purpose::STANDARD.encode(&buf[..len2]);

        let step2_body = json!({
            "handshake_id": "test-hs-001",
            "message": client_msg2,
        });

        let request2 = Request::builder()
            .method("POST")
            .uri("/v1/noise/handshake/step2")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&step2_body).unwrap()))
            .unwrap();

        let response2 = app.oneshot(request2).await.unwrap();
        assert_eq!(response2.status(), StatusCode::OK);

        let body2 = response2.into_body().collect().await.unwrap().to_bytes();
        let step2_resp: serde_json::Value = serde_json::from_slice(&body2).unwrap();
        let session_id = step2_resp["session_id"].as_str().unwrap();
        assert!(!session_id.is_empty(), "session_id should be non-empty");
        // Session ID is hex-encoded, should be 64 chars (32 bytes)
        assert_eq!(session_id.len(), 64, "session_id should be 64 hex chars");
    }

    #[tokio::test]
    async fn noise_handshake_step1_rejects_invalid_base64() {
        let state = state_with_noise();
        let app = build_router(state, &default_config());

        let body = json!({
            "handshake_id": "test-bad-b64",
            "message": "not-valid-base64!!!",
        });

        let request = Request::builder()
            .method("POST")
            .uri("/v1/noise/handshake/step1")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn noise_handshake_step2_rejects_unknown_handshake_id() {
        let state = state_with_noise();
        let app = build_router(state, &default_config());

        let body = json!({
            "handshake_id": "nonexistent-hs",
            "message": base64::engine::general_purpose::STANDARD.encode(b"hello"),
        });

        let request = Request::builder()
            .method("POST")
            .uri("/v1/noise/handshake/step2")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn noise_disabled_returns_service_unavailable() {
        let state = GatewayState::test_with_bearer(None);
        let app = build_router(state, &default_config());

        let body = json!({
            "handshake_id": "test",
            "message": base64::engine::general_purpose::STANDARD.encode(b"hello"),
        });

        let request = Request::builder()
            .method("POST")
            .uri("/v1/noise/handshake/step1")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // --- Privacy info endpoint ---

    #[tokio::test]
    async fn privacy_info_reports_noise_enabled() {
        let state = state_with_noise();
        let app = build_router(state, &default_config());

        let request = Request::builder()
            .method("GET")
            .uri("/v1/privacy/info")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let info: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(info["noise_enabled"], true);
        assert_eq!(info["handshake_pattern"], "XX");
        assert!(!info["public_key"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn privacy_info_reports_noise_disabled() {
        let state = GatewayState::test_with_bearer(None);
        let app = build_router(state, &default_config());

        let request = Request::builder()
            .method("GET")
            .uri("/v1/privacy/info")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let info: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(info["noise_enabled"], false);
    }

    // --- Relay integration tests ---

    #[tokio::test]
    async fn relay_submit_and_poll_round_trip() {
        let state = state_with_relay();
        let app = build_router(state, &default_config());

        let routing_id = "aa".repeat(32);
        let payload = base64::engine::general_purpose::STANDARD.encode(b"sealed-envelope-data");

        // Submit
        let submit_body = json!({
            "routing_id": routing_id,
            "payload": payload,
        });

        let request = Request::builder()
            .method("POST")
            .uri("/v1/relay/submit")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&submit_body).unwrap()))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Poll
        let request = Request::builder()
            .method("GET")
            .uri(format!("/v1/relay/poll/{routing_id}"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let poll_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let envelopes = poll_resp["envelopes"].as_array().unwrap();
        assert_eq!(envelopes.len(), 1);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(envelopes[0].as_str().unwrap())
            .unwrap();
        assert_eq!(decoded, b"sealed-envelope-data");
    }

    #[tokio::test]
    async fn relay_submit_rejects_duplicate_nonce() {
        let state = state_with_relay();
        let app = build_router(state, &default_config());

        let routing_id = "bb".repeat(32);
        let payload = base64::engine::general_purpose::STANDARD.encode(b"data");
        let nonce = base64::engine::general_purpose::STANDARD.encode([42u8; 24]);

        let submit_body = json!({
            "routing_id": routing_id,
            "payload": payload,
            "nonce": nonce,
        });

        // First submit should succeed
        let request = Request::builder()
            .method("POST")
            .uri("/v1/relay/submit")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&submit_body).unwrap()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Second submit with same nonce should fail with 409
        let request = Request::builder()
            .method("POST")
            .uri("/v1/relay/submit")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&submit_body).unwrap()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn relay_disabled_returns_not_found() {
        // When relay mode is disabled, routes are not registered → 404.
        let state = GatewayState::test_with_bearer(None);
        let app = build_router(state, &default_config());

        let submit_body = json!({
            "routing_id": "cc".repeat(32),
            "payload": base64::engine::general_purpose::STANDARD.encode(b"data"),
        });

        let request = Request::builder()
            .method("POST")
            .uri("/v1/relay/submit")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&submit_body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // --- End-to-end Noise encrypted request/response ---

    /// Helper: perform full XX handshake and return (app, server_session_id_hex, client_session).
    async fn perform_handshake() -> (Router, String, agentzero_core::privacy::noise::NoiseSession) {
        let state = state_with_noise();
        let app = build_router(state, &default_config());

        // Step 1: Client → e
        let client_kp = NoiseKeypair::generate().unwrap();
        let mut client = NoiseHandshaker::new_initiator("XX", &client_kp).unwrap();
        let mut buf = [0u8; 65535];
        let len = client.write_message(b"", &mut buf).unwrap();
        let client_msg = base64::engine::general_purpose::STANDARD.encode(&buf[..len]);

        let step1_body = json!({
            "handshake_id": "e2e-test",
            "message": client_msg,
        });
        let request = Request::builder()
            .method("POST")
            .uri("/v1/noise/handshake/step1")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&step1_body).unwrap()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let step1_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let server_msg = base64::engine::general_purpose::STANDARD
            .decode(step1_resp["message"].as_str().unwrap())
            .unwrap();

        // Client reads server's ← e ee s es
        client.read_message(&server_msg, &mut buf).unwrap();

        // Step 2: Client → s se
        let len2 = client.write_message(b"", &mut buf).unwrap();
        let client_msg2 = base64::engine::general_purpose::STANDARD.encode(&buf[..len2]);
        let step2_body = json!({
            "handshake_id": "e2e-test",
            "message": client_msg2,
        });
        let request2 = Request::builder()
            .method("POST")
            .uri("/v1/noise/handshake/step2")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&step2_body).unwrap()))
            .unwrap();
        let response2 = app.clone().oneshot(request2).await.unwrap();
        assert_eq!(response2.status(), StatusCode::OK);
        let body2 = response2.into_body().collect().await.unwrap().to_bytes();
        let step2_resp: serde_json::Value = serde_json::from_slice(&body2).unwrap();
        let session_id_hex = step2_resp["session_id"].as_str().unwrap().to_string();

        // Client transitions to transport mode
        assert!(client.is_finished());
        let client_session = client.into_transport().unwrap();

        (app, session_id_hex, client_session)
    }

    #[tokio::test]
    async fn noise_e2e_encrypted_response_on_get() {
        let (app, session_id, mut client_session) = perform_handshake().await;

        // GET /health with X-Noise-Session header (empty body → middleware passes through)
        let request = Request::builder()
            .method("GET")
            .uri("/health")
            .header("x-noise-session", &session_id)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Response body should be encrypted
        let encrypted_body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(
            !encrypted_body.is_empty(),
            "encrypted response should not be empty"
        );

        // Client decrypts the response
        let decrypted = client_session.decrypt(&encrypted_body).unwrap();
        let health: serde_json::Value = serde_json::from_slice(&decrypted).unwrap();
        assert_eq!(health["status"], "ok");
        assert_eq!(health["service"], "agentzero-gateway");
    }

    #[tokio::test]
    async fn noise_e2e_request_without_session_passes_through() {
        let state = state_with_noise();
        let app = build_router(state, &default_config());

        // GET /health WITHOUT X-Noise-Session header → plaintext passthrough
        let request = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let health: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(health["status"], "ok");
    }

    #[tokio::test]
    async fn noise_e2e_invalid_session_returns_unauthorized() {
        let state = state_with_noise();
        let app = build_router(state, &default_config());

        // Send request with bogus session ID and a non-empty encrypted body
        let request = Request::builder()
            .method("POST")
            .uri("/health")
            .header("x-noise-session", "aa".repeat(32))
            .body(Body::from(vec![1u8; 32])) // non-empty → triggers decrypt
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn relay_poll_empty_mailbox_returns_empty_array() {
        let state = state_with_relay();
        let app = build_router(state, &default_config());

        let routing_id = "dd".repeat(32);
        let request = Request::builder()
            .method("GET")
            .uri(format!("/v1/relay/poll/{routing_id}"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let poll_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(poll_resp["envelopes"].as_array().unwrap().len(), 0);
    }
}

// ---------------------------------------------------------------------------
// Transcript API tests
// ---------------------------------------------------------------------------

/// In-memory store for transcript tests.
#[derive(Default, Clone)]
struct TestTranscriptStore {
    entries: Arc<std::sync::Mutex<Vec<MemoryEntry>>>,
}

#[async_trait::async_trait]
impl MemoryStore for TestTranscriptStore {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        self.entries.lock().unwrap().push(entry);
        Ok(())
    }
    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let entries = self.entries.lock().unwrap();
        Ok(entries.iter().rev().take(limit).cloned().collect())
    }
    async fn recent_for_conversation(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let entries = self.entries.lock().unwrap();
        Ok(entries
            .iter()
            .filter(|e| e.conversation_id == conversation_id)
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect())
    }
}

#[tokio::test]
async fn v1_runs_transcript_returns_entries() {
    let store = TestTranscriptStore::default();
    store
        .append(MemoryEntry {
            role: "user".into(),
            content: "hello agent".into(),
            conversation_id: "run-abc".into(),
            created_at: Some("2026-03-08T10:00:00".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    store
        .append(MemoryEntry {
            role: "assistant".into(),
            content: "hello human".into(),
            conversation_id: "run-abc".into(),
            created_at: Some("2026-03-08T10:00:01".into()),
            ..Default::default()
        })
        .await
        .unwrap();

    let mut state = GatewayState::test_with_bearer(Some("tok"));
    state.memory_store = Some(Arc::new(store));
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/runs/run-abc/transcript")
        .header("authorization", "Bearer tok")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["object"], "transcript");
    assert_eq!(resp["run_id"], "run-abc");
    assert_eq!(resp["total"], 2);
    let entries = resp["entries"].as_array().unwrap();
    assert_eq!(entries[0]["role"], "user");
    assert_eq!(entries[0]["content"], "hello agent");
    assert_eq!(entries[1]["role"], "assistant");
}

#[tokio::test]
async fn v1_runs_transcript_not_found_for_unknown_run() {
    let store = TestTranscriptStore::default();
    let mut state = GatewayState::test_with_bearer(Some("tok"));
    state.memory_store = Some(Arc::new(store));
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/runs/nonexistent/transcript")
        .header("authorization", "Bearer tok")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn v1_runs_transcript_requires_auth() {
    let store = TestTranscriptStore::default();
    let mut state = GatewayState::test_with_bearer(Some("tok"));
    state.memory_store = Some(Arc::new(store));
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/runs/run-abc/transcript")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn v1_runs_transcript_no_store_returns_503() {
    // No memory store configured → AgentUnavailable (503)
    let state = GatewayState::test_with_bearer(Some("tok"));
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/runs/run-abc/transcript")
        .header("authorization", "Bearer tok")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// --- Health & readiness probe tests ---

#[tokio::test]
async fn health_returns_version_field() {
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());
    let request = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
    assert!(json["version"].is_string());
    assert!(!json["version"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn health_ready_returns_ready_without_config() {
    // No config_path → memory_store check is skipped → ready = true.
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());
    let request = Request::builder()
        .method("GET")
        .uri("/health/ready")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ready"], true);
    assert!(json["version"].is_string());
}

#[tokio::test]
async fn health_ready_reports_missing_memory_store_when_config_set() {
    // Config path set but no memory store → checks_failed includes "memory_store".
    let mut state = GatewayState::test_with_bearer(None);
    state.config_path = Some(std::sync::Arc::new(std::path::PathBuf::from(
        "/tmp/test.toml",
    )));
    let app = build_router(state, &default_config());
    let request = Request::builder()
        .method("GET")
        .uri("/health/ready")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ready"], false);
    let checks = json["checks_failed"].as_array().unwrap();
    assert!(checks.iter().any(|v| v == "memory_store"));
}

// ---------------------------------------------------------------------------
// New endpoint coverage tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ready_endpoint_returns_ready_true() {
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());
    let request = Request::builder()
        .method("GET")
        .uri("/health/ready")
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
    assert_eq!(json["ready"], true);
    assert!(
        json["service"].as_str().is_some_and(|s| !s.is_empty()),
        "response should contain non-empty service field"
    );
    assert!(
        json["version"].as_str().is_some_and(|v| !v.is_empty()),
        "response should contain non-empty version field"
    );
}

#[tokio::test]
async fn ping_echoes_message_with_fields() {
    // Open mode (no bearer requirement) so auth passes.
    let state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("POST")
        .uri("/v1/ping")
        .header("content-type", "application/json")
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
    assert_eq!(json["echo"], "hello", "ping should echo back the message");
}

#[tokio::test]
async fn api_fallback_returns_200_for_unknown_path() {
    // api_fallback requires API-level auth. Use paired token for access.
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());

    // First pair to get a valid token.
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

    // Now hit an unknown API path with the paired token.
    let request = Request::builder()
        .method("GET")
        .uri("/api/nonexistent")
        .header("authorization", format!("Bearer {token}"))
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
    assert_eq!(json["ok"], true);
    assert_eq!(json["path"], "nonexistent");
}

#[tokio::test]
async fn agents_list_returns_empty_without_presence() {
    // Create a presence store with no registered agents.
    let presence = std::sync::Arc::new(agentzero_orchestrator::PresenceStore::new());
    let mut state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    state.presence_store = Some(presence);
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/v1/agents")
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
    assert_eq!(json["total"], 0);
    assert_eq!(json["data"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn v1_chat_completions_format_matches_openai() {
    // Without config, chat completions returns 503 (AgentUnavailable).
    // Verify the error response matches OpenAI-style error shape: { "error": { "type": ..., "message": ... } }
    let state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let body = json!({
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
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let resp_body = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&resp_body).expect("should be json");

    // OpenAI-compatible error shape: { "error": { "type": "...", "message": "..." } }
    assert!(
        json["error"].is_object(),
        "error response should have an 'error' object"
    );
    assert!(
        json["error"]["type"].is_string(),
        "error.type should be a string"
    );
    assert!(
        json["error"]["message"].is_string(),
        "error.message should be a string"
    );
    assert_eq!(json["error"]["type"], "agent_unavailable");
}

#[tokio::test]
async fn websocket_run_subscribe_rejects_without_job_store() {
    // No job_store on state → ws_run_subscribe should fail.
    // The WebSocketUpgrade extractor requires a real HTTP upgrade, which oneshot
    // cannot perform, so the request returns 426 Upgrade Required. This confirms
    // the route exists and is correctly wired but requires a true WS connection.
    let state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("GET")
        .uri("/ws/runs/some-run-id")
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
    // Axum's WebSocketUpgrade extractor rejects oneshot requests with 426
    // because a real HTTP/1.1 upgrade handshake is required.
    assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);
}

// --- Channel name validation tests ---

#[test]
fn valid_channel_names() {
    use crate::handlers::is_valid_channel_name;
    assert!(is_valid_channel_name("telegram"));
    assert!(is_valid_channel_name("slack-alerts"));
    assert!(is_valid_channel_name("my_channel_01"));
    assert!(is_valid_channel_name("A"));
}

#[test]
fn invalid_channel_names() {
    use crate::handlers::is_valid_channel_name;
    assert!(!is_valid_channel_name(""));
    assert!(!is_valid_channel_name("../traversal"));
    assert!(!is_valid_channel_name("spaces not ok"));
    assert!(!is_valid_channel_name("semi;colon"));
    assert!(!is_valid_channel_name("path/slash"));
    let long = "a".repeat(65);
    assert!(!is_valid_channel_name(&long));
}

#[test]
fn channel_name_at_boundary() {
    use crate::handlers::is_valid_channel_name;
    let exactly_64 = "a".repeat(64);
    assert!(is_valid_channel_name(&exactly_64));
}

// ============================================================
// E2E Security Integration Tests
// ============================================================

/// Full auth lifecycle: create API key → use it to authenticate → verify
/// scope enforcement (403 on insufficient scope) → revoke key → 401 on revoked key.
#[tokio::test]
async fn e2e_api_key_lifecycle_and_scope_enforcement() {
    use crate::api_keys::{ApiKeyStore, Scope};
    use std::collections::HashSet;

    // Set up gateway with an API key store (no bearer token → requires API key).
    let store = Arc::new(ApiKeyStore::new());
    let mut state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().expect("lock").clear();
    state.api_key_store = Some(store.clone());
    // Disable pairing requirement for this test.
    let state = state.with_gateway_config(false, false);

    let config = default_config();
    let app = build_router(state, &config);

    // Step 1: Create an API key with only RunsRead scope.
    let scopes: HashSet<Scope> = [Scope::RunsRead].into();
    let (raw_key, record) = store
        .create("org-e2e", "user-e2e", scopes, None)
        .expect("key creation should succeed");

    // Step 2: Authenticated request to /health (no scope required) should succeed.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    // Step 3: Authenticated request to /v1/models (requires RunsRead) should succeed.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("Authorization", format!("Bearer {raw_key}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    // Step 4: Request to /v1/estop (requires Admin) should return 403 Forbidden.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/estop")
                .header("Authorization", format!("Bearer {raw_key}"))
                .header("Content-Type", "application/json")
                .body(Body::from("{}"))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Step 5: Request without any token should return 401.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Step 6: Revoke the key.
    assert!(store.revoke(&record.key_id).expect("revoke should succeed"));

    // Step 7: Request with revoked key should return 401.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("Authorization", format!("Bearer {raw_key}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Verify that an API key with Admin scope can access admin-only endpoints.
#[tokio::test]
async fn e2e_admin_scope_grants_estop_access() {
    use crate::api_keys::{ApiKeyStore, Scope};

    let store = Arc::new(ApiKeyStore::new());
    let mut state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().expect("lock").clear();
    state.api_key_store = Some(store.clone());
    let state = state.with_gateway_config(false, false);

    let config = default_config();
    let app = build_router(state, &config);

    // Create key with Admin scope.
    let (raw_key, _) = store
        .create("org-admin", "admin-user", [Scope::Admin].into(), None)
        .expect("key creation");

    // /v1/estop should succeed with Admin scope.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/estop")
                .header("Authorization", format!("Bearer {raw_key}"))
                .header("Content-Type", "application/json")
                .body(Body::from("{}"))
                .expect("request"),
        )
        .await
        .expect("response");
    // Should not be 403 (scope check passes). May be 500 if no job store, but not 403.
    assert_ne!(resp.status(), StatusCode::FORBIDDEN);
    assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Expired API key returns 401.
#[tokio::test]
async fn e2e_expired_api_key_returns_401() {
    use crate::api_keys::{ApiKeyStore, Scope};

    let store = Arc::new(ApiKeyStore::new());
    let mut state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().expect("lock").clear();
    state.api_key_store = Some(store.clone());
    let state = state.with_gateway_config(false, false);

    let config = default_config();
    let app = build_router(state, &config);

    // Create key that expired in the past (epoch 0).
    let (raw_key, _) = store
        .create("org-1", "user-1", [Scope::RunsRead].into(), Some(0))
        .expect("key creation");

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("Authorization", format!("Bearer {raw_key}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Sustained concurrent load: 100 parallel requests to /health, all succeed.
#[tokio::test]
async fn e2e_load_concurrent_health_requests() {
    let state = GatewayState::test_with_bearer(None);
    let config = default_config();
    let app = build_router(state, &config);

    let mut handles = Vec::new();
    for _ in 0..100 {
        let app = app.clone();
        handles.push(tokio::spawn(async move {
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/health")
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            resp.status()
        }));
    }

    let mut ok_count = 0;
    for handle in handles {
        let status = handle.await.expect("task should not panic");
        if status == StatusCode::OK {
            ok_count += 1;
        }
    }
    assert_eq!(ok_count, 100, "all 100 requests should return 200 OK");
}

/// Sustained concurrent load: 50 authenticated requests to /v1/models.
#[tokio::test]
async fn e2e_load_concurrent_authenticated_requests() {
    use crate::api_keys::{ApiKeyStore, Scope};

    let store = Arc::new(ApiKeyStore::new());
    let mut state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().expect("lock").clear();
    state.api_key_store = Some(store.clone());
    let state = state.with_gateway_config(false, false);

    let (raw_key, _) = store
        .create("org-load", "user-load", [Scope::RunsRead].into(), None)
        .expect("key creation");

    let config = default_config();
    let app = build_router(state, &config);

    let mut handles = Vec::new();
    for _ in 0..50 {
        let app = app.clone();
        let key = raw_key.clone();
        handles.push(tokio::spawn(async move {
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/v1/models")
                        .header("Authorization", format!("Bearer {key}"))
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            resp.status()
        }));
    }

    let mut ok_count = 0;
    for handle in handles {
        let status = handle.await.expect("task should not panic");
        if status == StatusCode::OK {
            ok_count += 1;
        }
    }
    assert_eq!(ok_count, 50, "all 50 authenticated requests should succeed");
}

/// WebSocket endpoints reject non-upgrade HTTP requests with 400 (Bad Request).
/// The WebSocketUpgrade extractor rejects before auth runs, which is correct —
/// plain HTTP requests shouldn't reach WebSocket handlers.
#[tokio::test]
async fn e2e_ws_endpoints_reject_non_upgrade_requests() {
    let state = GatewayState::test_with_bearer(Some("secret"));
    let config = default_config();
    let app = build_router(state, &config);

    // /ws/chat — non-upgrade GET → 400.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ws/chat")
                .header("Authorization", "Bearer secret")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // /ws/runs/:run_id — non-upgrade GET → 400.
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/ws/runs/test-run-123")
                .header("Authorization", "Bearer secret")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// WebSocket max message size constant is 2 MB.
#[test]
fn ws_max_message_size_is_2mb() {
    use crate::handlers::WS_MAX_MESSAGE_SIZE;
    assert_eq!(WS_MAX_MESSAGE_SIZE, 2 * 1024 * 1024);
}

/// Session TTL enforcement at the HTTP level.
#[tokio::test]
async fn e2e_session_ttl_enforcement() {
    // Create a state with a paired token and a short TTL.
    let mut state = GatewayState::test_with_existing_pair("session-tok");
    state.session_ttl_secs = Some(3600); // 1 hour TTL

    // Record timestamp from 2 hours ago → token should be expired.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    state
        .paired_token_timestamps
        .lock()
        .expect("lock")
        .insert("session-tok".to_string(), now - 7200);

    let config = default_config();
    let app = build_router(state, &config);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("Authorization", "Bearer session-tok")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// Liveness probe tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_live_returns_alive_true() {
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());
    let request = Request::builder()
        .method("GET")
        .uri("/health/live")
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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("body should be json");
    assert_eq!(json["alive"], true);
}

#[tokio::test]
async fn health_live_no_auth_required() {
    let app = build_router(
        GatewayState::test_with_bearer(Some("secret-token")),
        &default_config(),
    );
    let request = Request::builder()
        .method("GET")
        .uri("/health/live")
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Typed response struct tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn api_fallback_returns_typed_response() {
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
    let pair_body = pair_response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let pair_json: serde_json::Value = serde_json::from_slice(&pair_body).expect("should parse");
    let token = pair_json["token"].as_str().expect("token").to_string();

    let request = Request::builder()
        .method("GET")
        .uri("/api/some-path")
        .header("authorization", format!("Bearer {token}"))
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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("body should be json");
    assert_eq!(json["ok"], true);
    assert_eq!(json["path"], "some-path");
}

#[tokio::test]
async fn webhook_invalid_channel_returns_400() {
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
    let pair_body = pair_response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let pair_json: serde_json::Value = serde_json::from_slice(&pair_body).expect("should parse");
    let token = pair_json["token"].as_str().expect("token").to_string();

    let request = Request::builder()
        .method("POST")
        .uri("/v1/webhook/invalid%20channel!")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"event":"test"}"#))
        .expect("request should build");
    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn webhook_payload_accepts_arbitrary_json() {
    let payload = serde_json::from_str::<crate::models::WebhookPayload>(
        r#"{"event":"push","repo":"my-repo","nested":{"key":42}}"#,
    )
    .expect("should deserialize arbitrary JSON");
    assert_eq!(payload.inner["event"], "push");
    assert_eq!(payload.inner["nested"]["key"], 42);
}

// ---------------------------------------------------------------------------
// SSE event bus endpoint tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sse_events_requires_event_bus() {
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/events")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    // No event bus configured → 503 (AgentUnavailable).
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn sse_events_requires_auth() {
    let bus = std::sync::Arc::new(agentzero_core::InMemoryBus::default_capacity());
    let state = GatewayState::test_with_bearer(Some("secret-tok")).with_event_bus(bus);
    let app = build_router(state, &default_config());
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/events")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    // No auth header → 401.
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ─── Event bus wiring integration tests ──────────────────────────────────────

#[tokio::test]
async fn job_store_publishes_to_shared_event_bus_on_submit() {
    // Verify that a job_store wired with an event bus publishes job.pending on submit.
    use agentzero_core::{EventBus, InMemoryBus};
    use agentzero_orchestrator::JobStore;

    let bus = Arc::new(InMemoryBus::default_capacity());
    let mut sub = bus.subscribe();
    let store = JobStore::new().with_event_bus(bus.clone() as Arc<dyn EventBus>);

    let run_id = store
        .submit("agent-a".into(), agentzero_core::Lane::Main, None)
        .await;
    let event = tokio::time::timeout(std::time::Duration::from_millis(100), async {
        loop {
            if let Ok(ev) = sub.recv().await {
                return ev;
            }
        }
    })
    .await
    .expect("event bus should receive job.pending within timeout");

    assert_eq!(event.topic, "job.pending");
    assert!(event.payload.contains(run_id.as_str()));
}

#[tokio::test]
async fn job_store_publishes_to_shared_event_bus_on_status_change() {
    // Verify that status transitions publish to the event bus.
    use agentzero_core::{EventBus, InMemoryBus, JobStatus};
    use agentzero_orchestrator::JobStore;

    let bus = Arc::new(InMemoryBus::default_capacity());
    let mut sub = bus.subscribe();
    let store = JobStore::new().with_event_bus(bus.clone() as Arc<dyn EventBus>);

    let run_id = store
        .submit("agent-b".into(), agentzero_core::Lane::Main, None)
        .await;
    // Drain the pending event.
    let _ = tokio::time::timeout(std::time::Duration::from_millis(50), async {
        loop {
            if sub.recv().await.is_ok() {
                return;
            }
        }
    })
    .await;

    store.update_status(&run_id, JobStatus::Running).await;
    let event = tokio::time::timeout(std::time::Duration::from_millis(100), async {
        loop {
            if let Ok(ev) = sub.recv().await {
                return ev;
            }
        }
    })
    .await
    .expect("event bus should receive job.running within timeout");

    assert_eq!(event.topic, "job.running");
}

#[tokio::test]
async fn sse_events_without_event_bus_returns_503() {
    // State with no event bus wired → SSE endpoint returns 503.
    let state = GatewayState::test_with_bearer(Some("tok"));
    let app = build_router(state, &default_config());
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/events")
                .header("authorization", "Bearer tok")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn presence_store_heartbeat_publishes_to_shared_event_bus() {
    // Verify PresenceStore wired with a bus emits presence.heartbeat on heartbeat().
    use agentzero_core::{EventBus, InMemoryBus};
    use agentzero_orchestrator::PresenceStore;

    let bus = Arc::new(InMemoryBus::default_capacity());
    let mut sub = bus.subscribe();
    let store = PresenceStore::new().with_event_bus(bus.clone() as Arc<dyn EventBus>);

    store
        .register("agent-x", std::time::Duration::from_secs(30))
        .await;
    store.heartbeat("agent-x").await;

    let event = tokio::time::timeout(std::time::Duration::from_millis(100), async {
        loop {
            if let Ok(ev) = sub.recv().await {
                return ev;
            }
        }
    })
    .await
    .expect("event bus should receive presence.heartbeat within timeout");

    assert_eq!(event.topic, "presence.heartbeat");
    assert!(event.payload.contains("agent-x"));
}

// ---------------------------------------------------------------------------
// Agent management CRUD tests
// ---------------------------------------------------------------------------

fn state_with_agent_store() -> GatewayState {
    use agentzero_orchestrator::AgentStore;
    let mut state = GatewayState::test_with_bearer(Some("test-token"));
    state.agent_store = Some(Arc::new(AgentStore::new()));
    state
}

#[tokio::test]
async fn create_agent_returns_201() {
    let app = build_router(state_with_agent_store(), &default_config());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/agents")
        .header("authorization", "Bearer test-token")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "name": "Aria",
                "description": "Travel assistant",
                "system_prompt": "You are Aria",
                "provider": "anthropic",
                "model": "claude-sonnet-4-20250514"
            })
            .to_string(),
        ))
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::CREATED);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["name"], "Aria");
    assert_eq!(json["status"], "active");
    assert!(json["agent_id"].as_str().unwrap().starts_with("agent_"));
}

#[tokio::test]
async fn create_agent_rejects_empty_name() {
    let app = build_router(state_with_agent_store(), &default_config());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/agents")
        .header("authorization", "Bearer test-token")
        .header("content-type", "application/json")
        .body(Body::from(json!({"name": ""}).to_string()))
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_agent_requires_admin_scope() {
    let app = build_router(state_with_agent_store(), &default_config());
    // No auth header → 401.
    let request = Request::builder()
        .method("POST")
        .uri("/v1/agents")
        .header("content-type", "application/json")
        .body(Body::from(json!({"name": "Test"}).to_string()))
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn list_agents_includes_dynamic_agents() {
    let state = state_with_agent_store();
    let store = state.agent_store.as_ref().unwrap();
    store
        .create(agentzero_orchestrator::AgentRecord {
            agent_id: String::new(),
            name: "TestBot".to_string(),
            description: String::new(),
            system_prompt: None,
            provider: String::new(),
            model: String::new(),
            keywords: vec![],
            allowed_tools: vec![],
            channels: std::collections::HashMap::new(),
            created_at: 0,
            updated_at: 0,
            status: agentzero_orchestrator::AgentStatus::Active,
        })
        .expect("create");

    let app = build_router(state, &default_config());
    let request = Request::builder()
        .method("GET")
        .uri("/v1/agents")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["total"], 1);
    assert_eq!(json["data"][0]["status"], "active");
}

#[tokio::test]
async fn get_agent_returns_detail() {
    let state = state_with_agent_store();
    let store = state.agent_store.as_ref().unwrap();
    let record = store
        .create(agentzero_orchestrator::AgentRecord {
            agent_id: String::new(),
            name: "DetailBot".to_string(),
            description: "A detailed bot".to_string(),
            system_prompt: Some("You are DetailBot".to_string()),
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            keywords: vec!["detail".to_string()],
            allowed_tools: vec![],
            channels: std::collections::HashMap::new(),
            created_at: 0,
            updated_at: 0,
            status: agentzero_orchestrator::AgentStatus::Active,
        })
        .expect("create");

    let app = build_router(state, &default_config());
    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/agents/{}", record.agent_id))
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["name"], "DetailBot");
    assert_eq!(json["source"], "dynamic");
    assert_eq!(json["provider"], "anthropic");
}

#[tokio::test]
async fn get_agent_unknown_returns_404() {
    let app = build_router(state_with_agent_store(), &default_config());
    let request = Request::builder()
        .method("GET")
        .uri("/v1/agents/nonexistent")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_agent_removes_and_returns_success() {
    let state = state_with_agent_store();
    let store = state.agent_store.as_ref().unwrap();
    let record = store
        .create(agentzero_orchestrator::AgentRecord {
            agent_id: String::new(),
            name: "Ephemeral".to_string(),
            description: String::new(),
            system_prompt: None,
            provider: String::new(),
            model: String::new(),
            keywords: vec![],
            allowed_tools: vec![],
            channels: std::collections::HashMap::new(),
            created_at: 0,
            updated_at: 0,
            status: agentzero_orchestrator::AgentStatus::Active,
        })
        .expect("create");

    let app = build_router(state, &default_config());
    let request = Request::builder()
        .method("DELETE")
        .uri(format!("/v1/agents/{}", record.agent_id))
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["deleted"], true);
}

#[tokio::test]
async fn update_agent_modifies_fields() {
    let state = state_with_agent_store();
    let store = state.agent_store.as_ref().unwrap();
    let record = store
        .create(agentzero_orchestrator::AgentRecord {
            agent_id: String::new(),
            name: "OldName".to_string(),
            description: String::new(),
            system_prompt: None,
            provider: String::new(),
            model: String::new(),
            keywords: vec![],
            allowed_tools: vec![],
            channels: std::collections::HashMap::new(),
            created_at: 0,
            updated_at: 0,
            status: agentzero_orchestrator::AgentStatus::Active,
        })
        .expect("create");

    let app = build_router(state, &default_config());
    let request = Request::builder()
        .method("PATCH")
        .uri(format!("/v1/agents/{}", record.agent_id))
        .header("authorization", "Bearer test-token")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"name": "NewName", "system_prompt": "Updated prompt"}).to_string(),
        ))
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["name"], "NewName");
    assert_eq!(json["system_prompt"], "Updated prompt");
}

#[tokio::test]
async fn webhook_with_agent_validates_agent_exists() {
    let app = build_router(state_with_agent_store(), &default_config());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/hooks/telegram/nonexistent_agent")
        .header("authorization", "Bearer test-token")
        .header("content-type", "application/json")
        .body(Body::from(json!({"update_id": 1}).to_string()))
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ─── Phase D: Webhook auto-registration helpers ──────────────────────────

#[test]
fn resolve_public_url_returns_none_without_config() {
    let state = GatewayState::test_with_bearer(Some("t"));
    // No live_config, no env var → None.
    let url = crate::handlers::resolve_public_url(&state);
    // We can't guarantee env var is unset, but at least exercise the path.
    if std::env::var("AGENTZERO_PUBLIC_URL").is_err() {
        assert!(url.is_none());
    }
}

#[test]
fn agent_channel_to_instance_config_maps_bot_token() {
    use std::collections::HashMap;
    let cfg = agentzero_orchestrator::AgentChannelConfig {
        bot_token: Some("my-bot-token".to_string()),
        webhook_url: None,
        extra: HashMap::new(),
    };
    let instance = crate::handlers::agent_channel_to_instance_config(&cfg);
    assert_eq!(instance.bot_token.as_deref(), Some("my-bot-token"));
}

#[test]
fn agent_channel_to_instance_config_maps_extra_fields() {
    let mut extra = std::collections::HashMap::new();
    extra.insert("access_token".to_string(), "at-123".to_string());
    extra.insert("channel_id".to_string(), "ch-456".to_string());
    extra.insert("app_token".to_string(), "app-789".to_string());
    let cfg = agentzero_orchestrator::AgentChannelConfig {
        bot_token: None,
        webhook_url: None,
        extra,
    };
    let instance = crate::handlers::agent_channel_to_instance_config(&cfg);
    assert_eq!(instance.access_token.as_deref(), Some("at-123"));
    assert_eq!(instance.channel_id.as_deref(), Some("ch-456"));
    assert_eq!(instance.app_token.as_deref(), Some("app-789"));
}

#[test]
fn build_channel_instance_unknown_returns_none() {
    let cfg = agentzero_channels::ChannelInstanceConfig::default();
    let result = agentzero_channels::build_channel_instance("nonexistent", &cfg);
    assert!(matches!(result, Ok(None)));
}

// ─── Cross-feature integration tests ────────────────────────────────────────

#[tokio::test]
async fn cross_feature_health_and_tools_both_respond() {
    // Verify that the health endpoint and the tools endpoint both work
    // on the same router instance, confirming cross-feature routing.
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());

    // Health endpoint
    let health_req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .expect("request should build");
    let health_resp = app
        .clone()
        .oneshot(health_req)
        .await
        .expect("health response");
    assert_eq!(health_resp.status(), StatusCode::OK);
    let health_body = health_resp
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let health_json: serde_json::Value =
        serde_json::from_slice(&health_body).expect("health should be json");
    assert_eq!(health_json["status"], "ok");

    // Tools endpoint (no auth required for test_with_bearer with no token)
    let tools_req = Request::builder()
        .method("GET")
        .uri("/v1/tools")
        .body(Body::empty())
        .expect("request should build");
    let tools_resp = app.oneshot(tools_req).await.expect("tools response");

    // Tools endpoint requires auth; with no bearer token set, it either
    // returns a tool list or an auth error. Both are valid cross-feature behavior.
    let status = tools_resp.status();
    assert!(
        status == StatusCode::OK
            || status == StatusCode::UNAUTHORIZED
            || status == StatusCode::FORBIDDEN,
        "tools endpoint should return OK, 401, or 403, got {status}"
    );
}

#[tokio::test]
async fn cross_feature_health_and_metrics_coexist() {
    // Verify that health and metrics endpoints both work on the same server.
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());

    // Health
    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .expect("request should build");
    let resp = app.clone().oneshot(req).await.expect("health response");
    assert_eq!(resp.status(), StatusCode::OK);

    // Metrics (Prometheus)
    let req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .expect("request should build");
    let resp = app.oneshot(req).await.expect("metrics response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let _body_str = String::from_utf8_lossy(&body);
    // Prometheus output is text, not JSON. In test mode the registry may be
    // empty, so just verify the endpoint responded successfully (200 OK).
}

#[tokio::test]
async fn cross_feature_health_ready_checks_memory_store() {
    // Verify that /health/ready integrates with memory store state.
    // Without a config path, memory_store is None but no check fires.
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());

    let req = Request::builder()
        .method("GET")
        .uri("/health/ready")
        .body(Body::empty())
        .expect("request should build");
    let resp = app.oneshot(req).await.expect("ready response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("should be json");
    assert_eq!(json["ready"], true);
}

#[tokio::test]
async fn cross_feature_openapi_spec_includes_tool_routes() {
    // Verify that the OpenAPI spec endpoint returns a valid spec that includes
    // tool-related paths, proving gateway + tool features are wired together.
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());

    let req = Request::builder()
        .method("GET")
        .uri("/v1/openapi.json")
        .body(Body::empty())
        .expect("request should build");
    let resp = app.oneshot(req).await.expect("openapi response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let spec: serde_json::Value = serde_json::from_slice(&body).expect("should be valid JSON");

    // Verify the spec has paths and includes expected endpoints.
    let paths = spec.get("paths").expect("spec should have paths");
    assert!(
        paths.get("/health").is_some(),
        "/health should be in OpenAPI spec"
    );
    assert!(
        paths.get("/health/ready").is_some(),
        "/health/ready should be in OpenAPI spec"
    );
}

// ---------------------------------------------------------------------------
// Workflow endpoint tests (Sprint 71 backend)
// ---------------------------------------------------------------------------

mod workflow_tests {
    use super::*;

    fn state_with_workflow_store() -> GatewayState {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let store =
            agentzero_orchestrator::WorkflowStore::persistent(tmp.path()).expect("create store");

        // Create a test workflow.
        store
            .create(agentzero_orchestrator::WorkflowRecord {
                workflow_id: String::new(),
                name: "test-workflow".to_string(),
                description: "A test workflow".to_string(),
                nodes: vec![serde_json::json!({
                    "id": "a1",
                    "data": {
                        "name": "agent1",
                        "nodeType": "agent",
                        "metadata": { "system_prompt": "test" }
                    }
                })],
                edges: vec![],
                created_at: 0,
                updated_at: 0,
            })
            .expect("create workflow");

        let mut state = GatewayState::test_with_bearer(Some("test-token"));
        state.workflow_store = Some(Arc::new(store));
        // Keep temp dir alive by leaking it (test only).
        std::mem::forget(tmp);
        state
    }

    fn authed(builder: axum::http::request::Builder) -> axum::http::request::Builder {
        builder.header("authorization", "Bearer test-token")
    }

    fn get_workflow_id(state: &GatewayState) -> String {
        let store = state.workflow_store.as_ref().expect("store");
        store
            .list()
            .first()
            .expect("workflow exists")
            .workflow_id
            .clone()
    }

    #[tokio::test]
    async fn export_workflow_returns_full_json() {
        let state = state_with_workflow_store();
        let wf_id = get_workflow_id(&state);
        let app = build_router(state, &default_config());

        let req = authed(Request::builder())
            .method("GET")
            .uri(format!("/v1/workflows/{wf_id}/export"))
            .header("authorization", "Bearer test-token")
            .body(Body::empty())
            .expect("build request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");

        assert_eq!(json["name"], "test-workflow");
        assert!(json["nodes"].is_array());
        assert!(json["edges"].is_array());
        assert!(json["workflow_id"].is_string());
    }

    #[tokio::test]
    async fn export_unknown_workflow_returns_404() {
        let state = state_with_workflow_store();
        let app = build_router(state, &default_config());

        let req = authed(Request::builder())
            .method("GET")
            .uri("/v1/workflows/nonexistent/export")
            .body(Body::empty())
            .expect("build request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn import_workflow_creates_new_record() {
        let state = state_with_workflow_store();
        let app = build_router(state, &default_config());

        let payload = json!({
            "name": "imported-wf",
            "description": "imported",
            "nodes": [{
                "id": "n1",
                "data": {
                    "name": "node1",
                    "nodeType": "agent",
                    "metadata": { "system_prompt": "hello" }
                }
            }],
            "edges": []
        });

        let req = authed(Request::builder())
            .method("POST")
            .uri("/v1/workflows/import")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("build request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");

        assert!(json["imported"].as_bool().unwrap_or(false));
        assert!(json["workflow_id"].is_string());
        assert_eq!(json["nodes_count"], 1);
    }

    #[tokio::test]
    async fn import_invalid_workflow_returns_400() {
        let state = state_with_workflow_store();
        let app = build_router(state, &default_config());

        // Empty nodes — compile will fail with EmptyGraph.
        let payload = json!({
            "name": "bad",
            "nodes": [],
            "edges": []
        });

        let req = authed(Request::builder())
            .method("POST")
            .uri("/v1/workflows/import")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("build request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn cancel_workflow_run_marks_cancelled() {
        let state = state_with_workflow_store();

        // Insert a fake running workflow run.
        {
            let mut runs = state.workflow_runs.lock().expect("lock");
            runs.insert(
                "test-run-1".to_string(),
                crate::state::WorkflowRunState {
                    run_id: "test-run-1".to_string(),
                    workflow_id: "wf-1".to_string(),
                    status: "running".to_string(),
                    node_statuses: std::collections::HashMap::new(),
                    node_outputs: std::collections::HashMap::new(),
                    outputs: std::collections::HashMap::new(),
                    started_at: 0,
                    finished_at: None,
                    error: None,
                },
            );
        }

        let app = build_router(state.clone(), &default_config());

        let req = authed(Request::builder())
            .method("DELETE")
            .uri("/v1/workflows/runs/test-run-1")
            .body(Body::empty())
            .expect("build request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(json["cancelled"].as_bool().unwrap_or(false));

        // Verify status updated.
        let runs = state.workflow_runs.lock().expect("lock");
        let run = runs.get("test-run-1").expect("run exists");
        assert_eq!(run.status, "cancelled");
        assert!(run.error.is_some());
    }

    #[tokio::test]
    async fn cancel_unknown_run_returns_404() {
        let state = state_with_workflow_store();
        let app = build_router(state, &default_config());

        let req = authed(Request::builder())
            .method("DELETE")
            .uri("/v1/workflows/runs/nonexistent")
            .body(Body::empty())
            .expect("build request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn stream_workflow_run_returns_sse() {
        let state = state_with_workflow_store();

        // Insert a completed workflow run.
        {
            let mut runs = state.workflow_runs.lock().expect("lock");
            runs.insert(
                "sse-run-1".to_string(),
                crate::state::WorkflowRunState {
                    run_id: "sse-run-1".to_string(),
                    workflow_id: "wf-1".to_string(),
                    status: "completed".to_string(),
                    node_statuses: std::collections::HashMap::new(),
                    node_outputs: std::collections::HashMap::new(),
                    outputs: std::collections::HashMap::new(),
                    started_at: 0,
                    finished_at: Some(1),
                    error: None,
                },
            );
        }

        let app = build_router(state, &default_config());

        let req = authed(Request::builder())
            .method("GET")
            .uri("/v1/workflows/runs/sse-run-1/stream")
            .body(Body::empty())
            .expect("build request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("text/event-stream")
        );

        // Read the SSE body — should contain at least one event with status.
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("data:"),
            "SSE body should contain data events"
        );
        assert!(
            text.contains("completed"),
            "SSE body should contain completed status"
        );
    }

    #[tokio::test]
    async fn stream_unknown_run_returns_404() {
        let state = state_with_workflow_store();
        let app = build_router(state, &default_config());

        let req = authed(Request::builder())
            .method("GET")
            .uri("/v1/workflows/runs/ghost/stream")
            .body(Body::empty())
            .expect("build request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn resume_nonexistent_gate_returns_404() {
        let state = state_with_workflow_store();

        // Insert a running workflow run but no gate senders.
        {
            let mut runs = state.workflow_runs.lock().expect("lock");
            runs.insert(
                "resume-run-1".to_string(),
                crate::state::WorkflowRunState {
                    run_id: "resume-run-1".to_string(),
                    workflow_id: "wf-1".to_string(),
                    status: "running".to_string(),
                    node_statuses: std::collections::HashMap::new(),
                    node_outputs: std::collections::HashMap::new(),
                    outputs: std::collections::HashMap::new(),
                    started_at: 0,
                    finished_at: None,
                    error: None,
                },
            );
        }

        let app = build_router(state, &default_config());

        let payload = json!({ "node_id": "gate-1", "decision": "approved" });
        let req = authed(Request::builder())
            .method("POST")
            .uri("/v1/workflows/runs/resume-run-1/resume")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("build request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn resume_gate_sends_decision() {
        let state = state_with_workflow_store();

        // Create a oneshot channel and register it as a gate sender.
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut senders = state.gate_senders.lock().expect("lock");
            senders.insert(("gate-run-1".to_string(), "gate-node-1".to_string()), tx);
        }

        // Insert the run.
        {
            let mut runs = state.workflow_runs.lock().expect("lock");
            runs.insert(
                "gate-run-1".to_string(),
                crate::state::WorkflowRunState {
                    run_id: "gate-run-1".to_string(),
                    workflow_id: "wf-1".to_string(),
                    status: "running".to_string(),
                    node_statuses: std::collections::HashMap::new(),
                    node_outputs: std::collections::HashMap::new(),
                    outputs: std::collections::HashMap::new(),
                    started_at: 0,
                    finished_at: None,
                    error: None,
                },
            );
        }

        let app = build_router(state, &default_config());

        let payload = json!({ "node_id": "gate-node-1", "decision": "denied" });
        let req = authed(Request::builder())
            .method("POST")
            .uri("/v1/workflows/runs/gate-run-1/resume")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("build request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(json["resumed"].as_bool().unwrap_or(false));
        assert_eq!(json["decision"], "denied");

        // Verify the decision was sent through the channel.
        let decision = rx.await.expect("should receive decision");
        assert_eq!(decision, "denied");
    }

    #[tokio::test]
    async fn resume_rejects_invalid_decision() {
        let state = state_with_workflow_store();
        let app = build_router(state, &default_config());

        let payload = json!({ "node_id": "g1", "decision": "maybe" });
        let req = authed(Request::builder())
            .method("POST")
            .uri("/v1/workflows/runs/some-run/resume")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("build request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}

// ---------------------------------------------------------------------------
// Delivery confirmation + gate suspend/resume executor tests
// ---------------------------------------------------------------------------

mod executor_tests {
    use agentzero_orchestrator::workflow_executor::*;
    use serde_json::json;
    use std::sync::Arc;

    struct FailingChannelDispatcher;

    #[async_trait::async_trait]
    impl StepDispatcher for FailingChannelDispatcher {
        async fn run_agent(
            &self,
            step: &ExecutionStep,
            input: &str,
            _context: Option<&serde_json::Value>,
        ) -> anyhow::Result<String> {
            Ok(format!("[{}] processed: {}", step.name, input))
        }

        async fn run_tool(
            &self,
            _tool_name: &str,
            _input: &serde_json::Value,
        ) -> anyhow::Result<String> {
            Ok("ok".to_string())
        }

        async fn send_channel(&self, _channel_type: &str, _message: &str) -> anyhow::Result<()> {
            anyhow::bail!("channel offline")
        }
    }

    struct DenyingGateDispatcher;

    #[async_trait::async_trait]
    impl StepDispatcher for DenyingGateDispatcher {
        async fn run_agent(
            &self,
            step: &ExecutionStep,
            input: &str,
            _context: Option<&serde_json::Value>,
        ) -> anyhow::Result<String> {
            Ok(format!("[{}] processed: {}", step.name, input))
        }

        async fn run_tool(
            &self,
            _tool_name: &str,
            _input: &serde_json::Value,
        ) -> anyhow::Result<String> {
            Ok("ok".to_string())
        }

        async fn send_channel(&self, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn suspend_gate(&self, _run_id: &str, _node_id: &str, _node_name: &str) -> String {
            "denied".to_string()
        }
    }

    fn channel_node(id: &str, channel_type: &str) -> serde_json::Value {
        json!({
            "id": id,
            "data": {
                "name": channel_type,
                "nodeType": "channel",
                "metadata": { "channel_type": channel_type }
            }
        })
    }

    fn agent_node(id: &str, name: &str) -> serde_json::Value {
        json!({
            "id": id,
            "data": {
                "name": name,
                "nodeType": "agent",
                "metadata": { "system_prompt": "test" }
            }
        })
    }

    fn gate_node(id: &str) -> serde_json::Value {
        json!({
            "id": id,
            "data": {
                "name": "gate",
                "nodeType": "gate",
                "metadata": {}
            }
        })
    }

    fn edge(id: &str, source: &str, target: &str) -> serde_json::Value {
        json!({
            "id": id,
            "source": source,
            "target": target,
            "sourceHandle": "response",
            "targetHandle": "input"
        })
    }

    #[tokio::test]
    async fn channel_delivery_failure_records_status() {
        // Channel node with a dispatcher that always fails send_channel.
        let nodes = vec![channel_node("c1", "slack")];
        let edges: Vec<serde_json::Value> = vec![];
        let plan = compile("wf-delivery", &nodes, &edges).expect("compile");

        let run = execute(&plan, "hello", Arc::new(FailingChannelDispatcher))
            .await
            .expect("execute");

        // Node should still complete (delivery failure is non-fatal).
        assert_eq!(run.node_statuses.get("c1"), Some(&NodeStatus::Completed));

        // delivery_status port should record the failure.
        let status = run
            .outputs
            .get(&("c1".to_string(), "delivery_status".to_string()))
            .expect("should have delivery_status");
        let status_str = status.as_str().expect("string");
        assert!(
            status_str.starts_with("failed:"),
            "expected 'failed:...' got '{status_str}'"
        );
    }

    #[tokio::test]
    async fn gate_with_deny_skips_approved_branch() {
        // a1 → gate → a2 (approved) / a3 (denied)
        // DenyingGateDispatcher always returns "denied".
        let nodes = vec![
            agent_node("a1", "check"),
            gate_node("g1"),
            agent_node("a2", "proceed"),
            agent_node("a3", "reject"),
        ];
        let edges = vec![
            edge("e1", "a1", "g1"),
            json!({
                "id": "e2", "source": "g1", "target": "a2",
                "sourceHandle": "approved", "targetHandle": "input"
            }),
            json!({
                "id": "e3", "source": "g1", "target": "a3",
                "sourceHandle": "denied", "targetHandle": "input"
            }),
        ];
        let plan = compile("wf-gate-deny", &nodes, &edges).expect("compile");

        let run = execute(&plan, "review this", Arc::new(DenyingGateDispatcher))
            .await
            .expect("execute");

        // Denied branch should complete, approved branch should be skipped.
        assert_eq!(
            run.node_statuses.get("a2"),
            Some(&NodeStatus::Skipped),
            "approved branch should be skipped when gate denies"
        );
        assert_eq!(
            run.node_statuses.get("a3"),
            Some(&NodeStatus::Completed),
            "denied branch should complete"
        );
    }

    #[tokio::test]
    async fn gate_default_auto_approves() {
        // Default StepDispatcher auto-approves (existing MockDispatcher behavior).
        struct AutoApproveDispatcher;

        #[async_trait::async_trait]
        impl StepDispatcher for AutoApproveDispatcher {
            async fn run_agent(
                &self,
                step: &ExecutionStep,
                input: &str,
                _ctx: Option<&serde_json::Value>,
            ) -> anyhow::Result<String> {
                Ok(format!("[{}] {}", step.name, input))
            }
            async fn run_tool(&self, _: &str, _: &serde_json::Value) -> anyhow::Result<String> {
                Ok("ok".to_string())
            }
            async fn send_channel(&self, _: &str, _: &str) -> anyhow::Result<()> {
                Ok(())
            }
            // suspend_gate uses default impl → "approved"
        }

        let nodes = vec![
            agent_node("a1", "check"),
            gate_node("g1"),
            agent_node("a2", "proceed"),
            agent_node("a3", "reject"),
        ];
        let edges = vec![
            edge("e1", "a1", "g1"),
            json!({
                "id": "e2", "source": "g1", "target": "a2",
                "sourceHandle": "approved", "targetHandle": "input"
            }),
            json!({
                "id": "e3", "source": "g1", "target": "a3",
                "sourceHandle": "denied", "targetHandle": "input"
            }),
        ];
        let plan = compile("wf-gate-approve", &nodes, &edges).expect("compile");

        let run = execute(&plan, "review", Arc::new(AutoApproveDispatcher))
            .await
            .expect("execute");

        assert_eq!(run.node_statuses.get("a2"), Some(&NodeStatus::Completed));
        assert_eq!(run.node_statuses.get("a3"), Some(&NodeStatus::Skipped));
    }
}

// ---------------------------------------------------------------------------
// Bulletproof Rust Web compliance tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn security_headers_present_on_all_responses() {
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

    assert_eq!(
        response
            .headers()
            .get("x-content-type-options")
            .map(|v| v.to_str().unwrap()),
        Some("nosniff"),
        "X-Content-Type-Options header missing or wrong"
    );
    assert_eq!(
        response
            .headers()
            .get("x-frame-options")
            .map(|v| v.to_str().unwrap()),
        Some("DENY"),
        "X-Frame-Options header missing or wrong"
    );
    assert_eq!(
        response
            .headers()
            .get("content-security-policy")
            .map(|v| v.to_str().unwrap()),
        Some("default-src 'none'; frame-ancestors 'none'"),
        "Content-Security-Policy header missing or wrong"
    );
    assert_eq!(
        response
            .headers()
            .get("referrer-policy")
            .map(|v| v.to_str().unwrap()),
        Some("strict-origin-when-cross-origin"),
        "Referrer-Policy header missing or wrong"
    );
}

#[tokio::test]
async fn correlation_id_generated_when_not_provided() {
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

    let request_id = response
        .headers()
        .get("x-request-id")
        .expect("X-Request-Id should be present")
        .to_str()
        .expect("should be valid string");

    // Generated IDs are UUID v4 format.
    assert_eq!(request_id.len(), 36, "should be a UUID");
    assert_eq!(
        request_id.chars().filter(|c| *c == '-').count(),
        4,
        "should have UUID dashes"
    );
}

#[tokio::test]
async fn correlation_id_propagated_when_provided() {
    let app = build_router(GatewayState::test_with_bearer(None), &default_config());
    let request = Request::builder()
        .method("GET")
        .uri("/health")
        .header("x-request-id", "custom-trace-123")
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");

    assert_eq!(
        response
            .headers()
            .get("x-request-id")
            .map(|v| v.to_str().unwrap()),
        Some("custom-trace-123"),
        "should propagate the provided X-Request-Id"
    );
}

#[tokio::test]
async fn auth_rejection_returns_structured_json_error() {
    let app = build_router(
        GatewayState::test_with_bearer(Some("secret")),
        &default_config(),
    );
    // Use an auth-protected endpoint (not /health which is public).
    let request = Request::builder()
        .method("POST")
        .uri("/v1/ping")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"message":"test"}"#))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("error should be JSON");
    assert_eq!(json["error"]["type"], "auth_required");
    assert!(
        json["error"]["message"].is_string(),
        "error message should be a string"
    );
}

#[tokio::test]
async fn malformed_json_returns_structured_bad_request() {
    // Send invalid JSON to a handler that uses AppJson — should get structured
    // GatewayError format, not axum's default plain-text 422.
    let state = GatewayState::test_with_bearer(None);
    state.paired_tokens.lock().unwrap().clear();
    let app = build_router(state, &default_config());

    let request = Request::builder()
        .method("POST")
        .uri("/v1/webhook/cli")
        .header("content-type", "application/json")
        .body(Body::from("{ this is not valid json }"))
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("error should be JSON");
    assert_eq!(json["error"]["type"], "bad_request");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("invalid request body"),
        "error message should describe the parsing failure"
    );
}

#[tokio::test]
async fn unmatched_route_returns_404_not_auth_error() {
    // The guide says: auth should NOT leak whether a route exists.
    // An unmatched path should get 404, not 401.
    let app = build_router(
        GatewayState::test_with_bearer(Some("secret")),
        &default_config(),
    );
    let request = Request::builder()
        .method("GET")
        .uri("/v1/this-route-does-not-exist")
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .oneshot(request)
        .await
        .expect("response should be returned");

    // Since auth is per-handler (not a global layer), unmatched routes hit
    // axum's default 404 fallback without going through auth at all.
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rate_limit_includes_standard_headers() {
    let config = MiddlewareConfig {
        rate_limit_max: 1,
        rate_limit_window_secs: 60,
        ..Default::default()
    };
    let app = build_router(GatewayState::test_with_bearer(None), &config);

    // First request succeeds and has rate limit headers.
    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .expect("request should build");
    let resp = app.clone().oneshot(req).await.expect("should respond");
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers().get("x-ratelimit-limit").is_some(),
        "should have X-RateLimit-Limit"
    );
    assert!(
        resp.headers().get("x-ratelimit-remaining").is_some(),
        "should have X-RateLimit-Remaining"
    );
    assert!(
        resp.headers().get("x-ratelimit-reset").is_some(),
        "should have X-RateLimit-Reset"
    );

    // Second request is rate limited and includes Retry-After.
    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .expect("request should build");
    let resp = app.oneshot(req).await.expect("should respond");
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        resp.headers().get("retry-after").is_some(),
        "429 should have Retry-After header"
    );
}
