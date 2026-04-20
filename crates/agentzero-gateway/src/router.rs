use crate::a2a::{a2a_agents, a2a_rpc, a2a_stream, agent_card};
use crate::handlers::{
    agent_stats, agents_list, api_chat, api_fallback, async_submit, cancel_workflow_run,
    create_agent, create_cron, create_template, create_workflow, dashboard, delete_agent,
    delete_cron, delete_template, delete_workflow, emergency_stop, execute_workflow,
    export_dynamic_tool_bundle, export_workflow, forget_memory, get_agent, get_config,
    get_template, get_tools, get_workflow, get_workflow_run, health, health_live, health_ready,
    import_dynamic_tool_bundle, import_workflow, job_cancel, job_events, job_list, job_result,
    job_status, job_transcript, legacy_webhook, list_approvals, list_cron, list_dynamic_tools,
    list_memory, list_templates, list_workflows, mcp_message, metrics, openapi_spec, pair, ping,
    recall_memory, resume_workflow_run, runtime_codegen_disable, runtime_codegen_enable,
    sse_events, sse_run_stream, stream_workflow_run, swarm_execute, tool_execute, topology,
    update_agent, update_config, update_cron, update_template, update_workflow,
    v1_chat_completions, v1_models, webhook, webhook_with_agent, ws_chat, ws_run_subscribe,
};
use crate::middleware::{self, MiddlewareConfig, RateLimiter};
use crate::state::GatewayState;
use axum::{
    body::Body,
    extract::Request,
    middleware::from_fn,
    routing::{get, patch, post},
    Router,
};
use std::sync::Arc;

/// Default request timeout for non-streaming routes (30 seconds).
const DEFAULT_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub(crate) fn build_router(state: GatewayState, config: &MiddlewareConfig) -> Router {
    let max_bytes = config.max_body_bytes;
    let limiter = Arc::new(
        RateLimiter::new(config.rate_limit_max, config.rate_limit_window_secs)
            .with_per_identity(config.rate_limit_per_identity),
    );
    let cors_origins = config.cors_allowed_origins.clone();

    // Streaming/long-lived routes — excluded from the request timeout layer.
    let streaming_routes = Router::new()
        .route("/ws/chat", get(ws_chat))
        .route("/ws/runs/:run_id", get(ws_run_subscribe))
        .route("/v1/runs/:run_id/stream", get(sse_run_stream))
        .route("/v1/runs/:run_id/events", get(job_events))
        .route("/v1/events", get(sse_events))
        .route(
            "/v1/workflows/runs/:run_id/stream",
            get(stream_workflow_run),
        )
        .route("/v1/chat/completions", post(v1_chat_completions))
        .route("/a2a/stream", post(a2a_stream));

    // Standard routes — these get the request timeout layer.
    let timeout = DEFAULT_REQUEST_TIMEOUT;
    let standard_routes = Router::new()
        .route("/", get(dashboard))
        .route("/health", get(health))
        .route("/health/ready", get(health_ready))
        .route("/health/live", get(health_live))
        .route("/metrics", get(metrics))
        .route("/pair", post(pair))
        .route("/webhook", post(legacy_webhook))
        .route("/v1/ping", post(ping))
        .route("/v1/webhook/:channel", post(webhook))
        .route("/api/chat", post(api_chat))
        .route("/v1/models", get(v1_models))
        .route("/v1/runs", post(async_submit).get(job_list))
        .route("/v1/runs/:run_id", get(job_status).delete(job_cancel))
        .route("/v1/runs/:run_id/result", get(job_result))
        .route("/v1/runs/:run_id/transcript", get(job_transcript))
        .route("/v1/agents", get(agents_list).post(create_agent))
        .route(
            "/v1/agents/:agent_id",
            get(get_agent).patch(update_agent).delete(delete_agent),
        )
        .route("/v1/agents/:agent_id/stats", get(agent_stats))
        .route("/v1/workflows", get(list_workflows).post(create_workflow))
        .route(
            "/v1/workflows/runs/:run_id",
            get(get_workflow_run).delete(cancel_workflow_run),
        )
        .route(
            "/v1/workflows/runs/:run_id/resume",
            post(resume_workflow_run),
        )
        .route(
            "/v1/workflows/:id",
            get(get_workflow)
                .patch(update_workflow)
                .delete(delete_workflow),
        )
        .route("/v1/workflows/:id/execute", post(execute_workflow))
        .route("/v1/workflows/:id/export", get(export_workflow))
        .route("/v1/workflows/import", post(import_workflow))
        .route("/v1/swarm", post(swarm_execute))
        .route("/v1/templates", get(list_templates).post(create_template))
        .route(
            "/v1/templates/:id",
            get(get_template)
                .patch(update_template)
                .delete(delete_template),
        )
        .route("/v1/topology", get(topology))
        .route("/v1/hooks/:channel/:agent_id", post(webhook_with_agent))
        .route("/v1/estop", post(emergency_stop))
        .route("/v1/runtime/codegen-disable", post(runtime_codegen_disable))
        .route("/v1/runtime/codegen-enable", post(runtime_codegen_enable))
        .route("/v1/tools", get(get_tools))
        .route("/v1/tool-execute", post(tool_execute))
        .route(
            "/v1/dynamic-tools",
            get(list_dynamic_tools).post(import_dynamic_tool_bundle),
        )
        .route(
            "/v1/dynamic-tools/:name/bundle",
            get(export_dynamic_tool_bundle),
        )
        .route("/mcp/message", post(mcp_message))
        .route("/.well-known/agent.json", get(agent_card))
        .route("/a2a", post(a2a_rpc))
        .route("/a2a/agents", get(a2a_agents))
        .route("/v1/config", get(get_config).put(update_config))
        .route("/v1/cron", get(list_cron).post(create_cron))
        .route("/v1/cron/:id", patch(update_cron).delete(delete_cron))
        .route("/v1/memory", get(list_memory))
        .route("/v1/memory/recall", post(recall_memory))
        .route("/v1/memory/forget", post(forget_memory))
        .route("/v1/approvals", get(list_approvals))
        .route("/v1/openapi.json", get(openapi_spec))
        .route("/docs", get(api_docs_handler))
        .route("/api/*path", get(api_fallback))
        .layer(from_fn(
            move |req: Request<Body>, next: axum::middleware::Next| async move {
                middleware::request_timeout(req, next, timeout).await
            },
        ));

    // Merge both route groups.
    let mut router = Router::new().merge(streaming_routes).merge(standard_routes);

    // Noise Protocol handshake and transport routes (privacy feature).
    #[cfg(feature = "privacy")]
    {
        use crate::noise_handshake::{
            noise_handshake_ik, noise_handshake_step1, noise_handshake_step2,
        };

        router = router
            .route("/v1/noise/handshake/step1", post(noise_handshake_step1))
            .route("/v1/noise/handshake/step2", post(noise_handshake_step2))
            .route("/v1/noise/handshake/ik", post(noise_handshake_ik))
            .route("/v1/privacy/info", get(privacy_info));

        // Noise transport middleware: decrypt request / encrypt response for
        // requests that carry the X-Noise-Session header.
        if let Some(ref sessions) = state.noise_sessions {
            let sessions = sessions.clone();
            router = router.layer(from_fn(
                move |req: Request<Body>, next: axum::middleware::Next| {
                    let s = sessions.clone();
                    async move {
                        crate::noise_middleware::noise_transport_middleware(req, next, s).await
                    }
                },
            ));
        }

        // Relay routes: submit and poll sealed envelopes.
        if state.relay_mode {
            use crate::relay::{relay_poll, relay_submit, strip_metadata_headers};

            router = router
                .route("/v1/relay/submit", post(relay_submit))
                .route("/v1/relay/poll/:routing_id", get(relay_poll))
                .layer(from_fn(strip_metadata_headers));
        }
    }

    // SPA static file serving (embedded-ui feature).
    #[cfg(feature = "embedded-ui")]
    {
        router = router.fallback(static_handler);
    }

    // -----------------------------------------------------------------------
    // Middleware stack — applied inside-out, so the LAST `.layer()` call is
    // the OUTERMOST layer (first to see requests, last to see responses).
    //
    // Request order (outermost → innermost):
    //   Compression → Correlation ID → Metrics → Security Headers → HSTS →
    //   CORS → Rate Limit → Body Limit
    //
    // The body limit uses tower-http's RequestBodyLimitLayer which wraps the
    // body stream itself, preventing chunked-encoding bypass.
    // -----------------------------------------------------------------------

    // (innermost) Body size limit — wraps the body stream, not just Content-Length.
    router = router.layer(tower_http::limit::RequestBodyLimitLayer::new(max_bytes));

    // Rate limiting middleware (only if configured).
    if config.rate_limit_max > 0 {
        let rate_limiter = limiter;
        router = router.layer(from_fn(
            move |req: Request<Body>, next: axum::middleware::Next| {
                let lim = rate_limiter.clone();
                async move { middleware::rate_limit(req, next, lim).await }
            },
        ));
    }

    // CORS middleware (only if configured).
    if !cors_origins.is_empty() {
        router = router.layer(from_fn(
            move |req: Request<Body>, next: axum::middleware::Next| {
                let o = cors_origins.clone();
                async move { middleware::cors_middleware(req, next, o).await }
            },
        ));
    }

    // HSTS middleware (only when TLS is active).
    if config.tls_enabled {
        router = router.layer(from_fn(middleware::hsts_middleware));
    }

    // Security headers (unconditional — X-Content-Type-Options, X-Frame-Options,
    // Content-Security-Policy, Referrer-Policy).
    router = router.layer(from_fn(middleware::security_headers));

    // Request metrics (records method, path, status, latency).
    router = router.layer(from_fn(middleware::request_metrics));

    // Correlation ID (propagate or generate X-Request-Id). Outermost so that
    // all downstream layers can reference the request ID in tracing spans.
    router = router.layer(from_fn(middleware::correlation_id));

    // (outermost) Response compression — gzip/deflate based on Accept-Encoding.
    router = router.layer(tower_http::compression::CompressionLayer::new());

    router.with_state(state)
}

// ---------------------------------------------------------------------------
// Interactive API documentation (Scalar)
// ---------------------------------------------------------------------------

async fn api_docs_handler() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("api_docs.html"))
}

// ---------------------------------------------------------------------------
// Embedded SPA static file serving (embedded-ui feature)
// ---------------------------------------------------------------------------

/// Embedded platform-control UI assets built by `cd ui && pnpm run build`.
#[cfg(feature = "embedded-ui")]
#[derive(rust_embed::Embed)]
#[folder = "../../ui/dist"]
#[prefix = ""]
#[include = "*.html"]
#[include = "*.js"]
#[include = "*.css"]
#[include = "*.svg"]
#[include = "*.png"]
#[include = "*.ico"]
#[include = "*.woff2"]
#[include = "*.json"]
struct UiAssets;

/// Serve embedded SPA assets with SPA fallback to index.html.
#[cfg(feature = "embedded-ui")]
async fn static_handler(uri: axum::http::Uri) -> impl axum::response::IntoResponse {
    use axum::body::Body;
    use axum::http::{header, StatusCode};
    use axum::response::Response;

    let path = uri.path().trim_start_matches('/');

    if let Some(file) = UiAssets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
            .body(Body::from(file.data.to_vec()))
            .unwrap_or_else(|_| {
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .expect("fallback response should build")
            });
    }

    // SPA fallback: serve index.html for all unmatched paths.
    if let Some(index) = UiAssets::get("index.html") {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(index.data.to_vec()))
            .unwrap_or_else(|_| {
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .expect("fallback response should build")
            });
    }

    Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from("UI not built. Run: cd ui && pnpm run build"))
        .expect("503 response should build")
}

/// GET /v1/privacy/info — returns gateway privacy capabilities.
/// Clients call this to discover what's available before initiating handshake.
#[cfg(feature = "privacy")]
async fn privacy_info(
    axum::extract::State(state): axum::extract::State<GatewayState>,
) -> axum::Json<serde_json::Value> {
    use base64::Engine as _;

    let noise_enabled = state.noise_sessions.is_some();
    let relay_mode = state.relay_mode;

    let (public_key, key_fingerprint) = if let Some(ref kp) = state.noise_keypair {
        let pk = base64::engine::general_purpose::STANDARD.encode(kp.public);
        // Fingerprint: first 8 bytes of routing_id (SHA-256 of public key), hex-encoded.
        let routing_id = kp.routing_id();
        let fingerprint = routing_id[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        (Some(pk), Some(fingerprint))
    } else {
        (None, None)
    };

    let mut supported_patterns = vec!["XX"];
    if public_key.is_some() {
        supported_patterns.push("IK");
    }

    axum::Json(serde_json::json!({
        "noise_enabled": noise_enabled,
        "handshake_pattern": "XX",
        "public_key": public_key,
        "key_fingerprint": key_fingerprint,
        "sealed_envelopes_enabled": relay_mode,
        "relay_mode": relay_mode,
        "supported_patterns": supported_patterns,
    }))
}
