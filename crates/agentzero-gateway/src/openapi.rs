//! Auto-generated OpenAPI 3.1 specification for the AgentZero gateway.
//!
//! Builds the spec as a `serde_json::Value`. Served at `GET /v1/openapi.json`.

use serde_json::{json, Value};

/// Build the complete OpenAPI 3.1 specification for the gateway.
pub fn build_openapi_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "AgentZero Gateway API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "HTTP/WebSocket API for AgentZero agent runtime. Supports chat completions (OpenAI-compatible), async job submission, agent management, and health monitoring."
        },
        "servers": [{ "url": "/" }],
        "components": {
            "securitySchemes": {
                "BearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "API Key or Session Token",
                    "description": "Pass an API key (az_...) or paired session token as Bearer token."
                }
            },
            "schemas": {
                "ErrorResponse": {
                    "type": "object",
                    "required": ["error"],
                    "properties": {
                        "error": {
                            "type": "object",
                            "required": ["type", "message"],
                            "properties": {
                                "type": { "type": "string" },
                                "message": { "type": "string" }
                            }
                        }
                    }
                },
                "HealthResponse": {
                    "type": "object",
                    "properties": {
                        "status": { "type": "string" },
                        "service": { "type": "string" },
                        "version": { "type": "string" }
                    }
                },
                "AsyncSubmitRequest": {
                    "type": "object",
                    "required": ["message"],
                    "properties": {
                        "message": { "type": "string", "description": "The user message to process" },
                        "mode": { "type": "string", "enum": ["steer", "followup", "collect", "interrupt"] },
                        "run_id": { "type": "string", "description": "Run ID for followup mode" },
                        "model": { "type": "string", "description": "Model override" }
                    }
                },
                "AsyncSubmitResponse": {
                    "type": "object",
                    "required": ["run_id", "accepted_at"],
                    "properties": {
                        "run_id": { "type": "string" },
                        "accepted_at": { "type": "string", "format": "date-time" }
                    }
                },
                "JobStatusResponse": {
                    "type": "object",
                    "properties": {
                        "run_id": { "type": "string" },
                        "status": { "type": "string", "enum": ["queued", "running", "completed", "failed", "cancelled"] },
                        "agent_id": { "type": "string" },
                        "result": { "type": "string" },
                        "error": { "type": "string" }
                    }
                },
                "PingRequest": {
                    "type": "object",
                    "required": ["message"],
                    "properties": {
                        "message": { "type": "string" }
                    }
                },
                "PingResponse": {
                    "type": "object",
                    "properties": {
                        "ok": { "type": "boolean" },
                        "echo": { "type": "string" }
                    }
                }
            }
        },
        "paths": {
            "/health": {
                "get": {
                    "tags": ["Health"],
                    "summary": "Health check",
                    "description": "Returns gateway health status.",
                    "responses": {
                        "200": {
                            "description": "Healthy",
                            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/HealthResponse" } } }
                        }
                    }
                }
            },
            "/health/ready": {
                "get": {
                    "tags": ["Health"],
                    "summary": "Readiness check",
                    "description": "Returns whether the gateway is ready to serve traffic.",
                    "responses": { "200": { "description": "Ready" } }
                }
            },
            "/metrics": {
                "get": {
                    "tags": ["Observability"],
                    "summary": "Prometheus metrics",
                    "description": "Returns Prometheus-format metrics.",
                    "responses": { "200": { "description": "Prometheus metrics in text format" } }
                }
            },
            "/v1/ping": {
                "post": {
                    "tags": ["Utility"],
                    "summary": "Ping",
                    "description": "Echo back a message to verify connectivity.",
                    "requestBody": {
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/PingRequest" } } }
                    },
                    "responses": {
                        "200": {
                            "description": "Pong",
                            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/PingResponse" } } }
                        }
                    }
                }
            },
            "/v1/chat/completions": {
                "post": {
                    "tags": ["Chat"],
                    "summary": "Chat completions (OpenAI-compatible)",
                    "description": "Send messages to the agent and receive a completion response. Supports streaming via `stream: true`.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "Completion response" },
                        "401": { "description": "Authentication required", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
                        "429": { "description": "Rate limited" }
                    }
                }
            },
            "/v1/models": {
                "get": {
                    "tags": ["Chat"],
                    "summary": "List models",
                    "description": "Returns available models (OpenAI-compatible format).",
                    "security": [{ "BearerAuth": [] }],
                    "responses": { "200": { "description": "Model list" } }
                }
            },
            "/api/chat": {
                "post": {
                    "tags": ["Chat"],
                    "summary": "Chat (legacy)",
                    "description": "Send a chat message and receive a response. Uses paired session authentication.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "Chat response" },
                        "401": { "description": "Authentication required" }
                    }
                }
            },
            "/v1/runs": {
                "post": {
                    "tags": ["Runs"],
                    "summary": "Submit async run",
                    "description": "Submit a message for asynchronous agent processing.",
                    "security": [{ "BearerAuth": [] }],
                    "requestBody": {
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/AsyncSubmitRequest" } } }
                    },
                    "responses": {
                        "202": {
                            "description": "Run accepted",
                            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/AsyncSubmitResponse" } } }
                        },
                        "401": { "description": "Authentication required" }
                    }
                },
                "get": {
                    "tags": ["Runs"],
                    "summary": "List runs",
                    "description": "List submitted runs, optionally filtered by status.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{
                        "name": "status",
                        "in": "query",
                        "required": false,
                        "schema": { "type": "string" }
                    }],
                    "responses": { "200": { "description": "Run list" } }
                }
            },
            "/v1/runs/{run_id}": {
                "get": {
                    "tags": ["Runs"],
                    "summary": "Get run status",
                    "description": "Get the status and result of a submitted run.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "run_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "200": {
                            "description": "Run status",
                            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/JobStatusResponse" } } }
                        },
                        "404": { "description": "Run not found" }
                    }
                },
                "delete": {
                    "tags": ["Runs"],
                    "summary": "Cancel run",
                    "description": "Cancel a running or queued run.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "run_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Run cancelled" } }
                }
            },
            "/v1/runs/{run_id}/result": {
                "get": {
                    "tags": ["Runs"],
                    "summary": "Get run result",
                    "description": "Get the final result of a completed run.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "run_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "200": { "description": "Run result" },
                        "404": { "description": "Run not found" }
                    }
                }
            },
            "/v1/runs/{run_id}/events": {
                "get": {
                    "tags": ["Runs"],
                    "summary": "Get run events",
                    "description": "Get event log for a run.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "run_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Event list" } }
                }
            },
            "/v1/runs/{run_id}/transcript": {
                "get": {
                    "tags": ["Runs"],
                    "summary": "Get run transcript",
                    "description": "Get the conversation transcript for a run.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "run_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Transcript entries" } }
                }
            },
            "/v1/runs/{run_id}/stream": {
                "get": {
                    "tags": ["Runs"],
                    "summary": "Stream run events (SSE)",
                    "description": "Server-Sent Events stream for a running job.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "run_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "SSE event stream" } }
                }
            },
            "/v1/agents": {
                "get": {
                    "tags": ["Agents"],
                    "summary": "List agents",
                    "description": "List configured agents and their capabilities.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": { "200": { "description": "Agent list" } }
                }
            },
            "/v1/estop": {
                "post": {
                    "tags": ["Admin"],
                    "summary": "Emergency stop",
                    "description": "Immediately halt all running agents. Requires admin scope.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "Agents stopped" },
                        "403": { "description": "Insufficient scope" }
                    }
                }
            },
            "/v1/openapi.json": {
                "get": {
                    "tags": ["Utility"],
                    "summary": "OpenAPI specification",
                    "description": "Returns this OpenAPI 3.1 specification as JSON.",
                    "responses": { "200": { "description": "OpenAPI 3.1 JSON specification" } }
                }
            },
            "/ws/chat": {
                "get": {
                    "tags": ["WebSocket"],
                    "summary": "WebSocket chat",
                    "description": "Interactive WebSocket connection for real-time chat with the agent.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "101": { "description": "WebSocket upgrade" },
                        "400": { "description": "Not a WebSocket upgrade request" }
                    }
                }
            },
            "/ws/runs/{run_id}": {
                "get": {
                    "tags": ["WebSocket"],
                    "summary": "WebSocket run subscription",
                    "description": "Subscribe to real-time updates for a specific run via WebSocket.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "run_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "101": { "description": "WebSocket upgrade" },
                        "400": { "description": "Not a WebSocket upgrade request" }
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_is_valid_json() {
        let spec = build_openapi_spec();
        let json = serde_json::to_string_pretty(&spec).expect("spec should serialize to JSON");
        assert!(json.contains("\"openapi\""));
        assert!(json.contains("AgentZero Gateway API"));
    }

    #[test]
    fn spec_includes_key_endpoints() {
        let spec = build_openapi_spec();
        let json = serde_json::to_string(&spec).expect("spec serializes");
        assert!(json.contains("/health"), "missing /health");
        assert!(
            json.contains("/v1/chat/completions"),
            "missing /v1/chat/completions"
        );
        assert!(json.contains("/v1/runs"), "missing /v1/runs");
        assert!(json.contains("/v1/models"), "missing /v1/models");
        assert!(json.contains("/v1/agents"), "missing /v1/agents");
        assert!(json.contains("/v1/estop"), "missing /v1/estop");
        assert!(
            json.contains("/v1/openapi.json"),
            "missing /v1/openapi.json"
        );
    }

    #[test]
    fn spec_has_security_scheme() {
        let spec = build_openapi_spec();
        let json = serde_json::to_string(&spec).expect("spec serializes");
        assert!(json.contains("BearerAuth"), "missing security scheme");
    }

    #[test]
    fn spec_has_schemas() {
        let spec = build_openapi_spec();
        let json = serde_json::to_string(&spec).expect("spec serializes");
        assert!(
            json.contains("ErrorResponse"),
            "missing ErrorResponse schema"
        );
        assert!(
            json.contains("AsyncSubmitRequest"),
            "missing AsyncSubmitRequest schema"
        );
        assert!(
            json.contains("JobStatusResponse"),
            "missing JobStatusResponse schema"
        );
    }

    #[test]
    fn spec_version_matches_crate() {
        let spec = build_openapi_spec();
        let version = spec["info"]["version"].as_str().expect("version present");
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
    }
}
