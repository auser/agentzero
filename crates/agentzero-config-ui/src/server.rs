use axum::body::Body;
use axum::http::{header, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use rust_embed::Embed;

use crate::api;

/// Embedded frontend assets built by Vite.
///
/// During development (when the `ui/dist` directory doesn't exist),
/// the server returns a placeholder page.
#[derive(Embed)]
#[folder = "ui/dist"]
#[prefix = ""]
#[include = "*.html"]
#[include = "*.js"]
#[include = "*.css"]
#[include = "*.svg"]
#[include = "*.png"]
#[include = "*.ico"]
#[include = "*.woff2"]
#[include = "*.json"]
struct Assets;

/// Build the axum router with API routes and static asset serving.
pub fn build_router() -> Router {
    build_router_with_agents(None)
}

/// Build the axum router, optionally including persistent agent management
/// routes when an `AgentStore` is provided.
pub fn build_router_with_agents(
    agent_store: Option<std::sync::Arc<agentzero_orchestrator::agent_store::AgentStore>>,
) -> Router {
    let mut router = Router::new()
        // API routes
        .route("/api/schema", get(api::get_schema))
        .route("/api/tools", get(api::get_tools))
        .route("/api/defaults", get(api::get_defaults))
        .route("/api/import", post(api::import_toml))
        .route("/api/export", post(api::export_toml))
        .route("/api/validate", post(api::validate));

    // Agent management routes (only when a store is available).
    if let Some(store) = agent_store {
        use crate::agents_api;
        use axum::routing::put;

        let agents_router = Router::new()
            .route(
                "/api/agents",
                get(agents_api::list_agents).post(agents_api::create_agent),
            )
            .route(
                "/api/agents/:id",
                get(agents_api::get_agent)
                    .put(agents_api::update_agent)
                    .delete(agents_api::delete_agent),
            )
            .route("/api/agents/:id/status", put(agents_api::set_agent_status))
            .with_state(store);

        router = router.merge(agents_router);
    }

    // Static assets (SPA fallback)
    router.fallback(static_handler)
}

/// Serve embedded static assets, falling back to index.html for SPA routing.
async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    // Try the exact path first
    if let Some(file) = Assets::get(path) {
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

    // SPA fallback: serve index.html for non-asset paths
    if let Some(index) = Assets::get("index.html") {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(index.data.to_vec()))
            .unwrap_or_else(|_| {
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .expect("fallback response should build")
            });
    }

    // No frontend built yet — show a dev placeholder
    Html(DEV_PLACEHOLDER).into_response()
}

const DEV_PLACEHOLDER: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>AgentZero Config UI</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
            background: #0f172a;
            color: #e2e8f0;
            display: flex;
            align-items: center;
            justify-content: center;
            height: 100vh;
            margin: 0;
        }
        .container {
            text-align: center;
            max-width: 600px;
        }
        h1 { color: #3b82f6; }
        code {
            background: #1e293b;
            padding: 2px 8px;
            border-radius: 4px;
            font-size: 0.9em;
        }
        .api-links a {
            color: #60a5fa;
            text-decoration: none;
            margin: 0 12px;
        }
        .api-links a:hover { text-decoration: underline; }
    </style>
</head>
<body>
    <div class="container">
        <h1>AgentZero Config UI</h1>
        <p>The frontend has not been built yet.</p>
        <p>To build: <code>cd crates/agentzero-config-ui/ui && npm install && npm run build</code></p>
        <p style="margin-top: 24px;">API endpoints are live:</p>
        <div class="api-links">
            <a href="/api/schema">/api/schema</a>
            <a href="/api/tools">/api/tools</a>
            <a href="/api/defaults">/api/defaults</a>
        </div>
    </div>
</body>
</html>"#;
