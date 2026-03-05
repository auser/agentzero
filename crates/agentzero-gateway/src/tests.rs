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
        assert!(info["public_key"].as_str().unwrap().len() > 0);
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
