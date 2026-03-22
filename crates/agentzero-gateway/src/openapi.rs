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
            "description": "HTTP/WebSocket API for AgentZero agent runtime. Supports chat completions (OpenAI-compatible), async job submission, agent management, tool execution, memory, cron scheduling, and real-time event streaming."
        },
        "servers": [{ "url": "/" }],
        "tags": [
            { "name": "Health", "description": "Health and readiness probes" },
            { "name": "Chat", "description": "OpenAI-compatible chat completions" },
            { "name": "Runs", "description": "Async job submission and monitoring" },
            { "name": "Agents", "description": "Agent CRUD and statistics" },
            { "name": "Tools", "description": "Tool listing and execution" },
            { "name": "Memory", "description": "Conversation memory management" },
            { "name": "Cron", "description": "Scheduled job management" },
            { "name": "Config", "description": "Runtime configuration" },
            { "name": "Events", "description": "Real-time event streaming" },
            { "name": "Webhooks", "description": "Inbound webhook ingestion" },
            { "name": "MCP", "description": "Model Context Protocol" },
            { "name": "A2A", "description": "Agent-to-Agent communication" },
            { "name": "Admin", "description": "Administrative operations" },
            { "name": "WebSocket", "description": "WebSocket endpoints" },
            { "name": "Observability", "description": "Metrics and topology" },
            { "name": "Utility", "description": "Utility endpoints" }
        ],
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
                "LivenessResponse": {
                    "type": "object",
                    "properties": {
                        "alive": { "type": "boolean" }
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
                "AgentResponse": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "name": { "type": "string" },
                        "model": { "type": "string" },
                        "system_prompt": { "type": "string" },
                        "keywords": { "type": "array", "items": { "type": "string" } },
                        "enabled": { "type": "boolean" }
                    }
                },
                "CreateAgentRequest": {
                    "type": "object",
                    "required": ["name"],
                    "properties": {
                        "name": { "type": "string" },
                        "model": { "type": "string" },
                        "system_prompt": { "type": "string" },
                        "keywords": { "type": "array", "items": { "type": "string" } }
                    }
                },
                "UpdateAgentRequest": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "model": { "type": "string" },
                        "system_prompt": { "type": "string" },
                        "keywords": { "type": "array", "items": { "type": "string" } },
                        "enabled": { "type": "boolean" }
                    }
                },
                "ToolResponse": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "description": { "type": "string" },
                        "category": { "type": "string" },
                        "input_schema": { "type": "object" }
                    }
                },
                "ToolExecuteRequest": {
                    "type": "object",
                    "required": ["name"],
                    "properties": {
                        "name": { "type": "string" },
                        "input": { "type": "object" }
                    }
                },
                "MemoryEntry": {
                    "type": "object",
                    "properties": {
                        "role": { "type": "string" },
                        "content": { "type": "string" },
                        "conversation_id": { "type": "string" },
                        "created_at": { "type": "integer" }
                    }
                },
                "RecallRequest": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer" }
                    }
                },
                "ForgetRequest": {
                    "type": "object",
                    "properties": {
                        "conversation_id": { "type": "string" }
                    }
                },
                "CronJob": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "schedule": { "type": "string", "description": "Cron expression" },
                        "action": { "type": "string" },
                        "enabled": { "type": "boolean" },
                        "last_run": { "type": "string", "format": "date-time" },
                        "next_run": { "type": "string", "format": "date-time" }
                    }
                },
                "CreateCronRequest": {
                    "type": "object",
                    "required": ["schedule", "action"],
                    "properties": {
                        "schedule": { "type": "string" },
                        "action": { "type": "string" }
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
                },
                "PairRequest": {
                    "type": "object",
                    "required": ["code"],
                    "properties": {
                        "code": { "type": "string", "description": "One-time pairing code" }
                    }
                },
                "PairResponse": {
                    "type": "object",
                    "properties": {
                        "token": { "type": "string", "description": "Session token for subsequent requests" }
                    }
                }
            }
        },
        "paths": {
            // --- Health ---
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
                    "description": "Returns whether the gateway is ready to serve traffic (dependency checks).",
                    "responses": { "200": { "description": "Ready" } }
                }
            },
            "/health/live": {
                "get": {
                    "tags": ["Health"],
                    "summary": "Liveness probe",
                    "description": "Spawns a trivial tokio task to verify async runtime health. Returns within 1s.",
                    "responses": {
                        "200": {
                            "description": "Alive",
                            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/LivenessResponse" } } }
                        }
                    }
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
            // --- Auth ---
            "/pair": {
                "post": {
                    "tags": ["Utility"],
                    "summary": "Pair with gateway",
                    "description": "Exchange a one-time pairing code for a session token.",
                    "requestBody": {
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/PairRequest" } } }
                    },
                    "responses": {
                        "200": {
                            "description": "Paired successfully",
                            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/PairResponse" } } }
                        },
                        "401": { "description": "Invalid pairing code" }
                    }
                }
            },
            // --- Chat ---
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
            // --- Runs ---
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
                    "description": "Server-Sent Events stream for a running job. Supports `?token=` query param auth.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "run_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "SSE event stream" } }
                }
            },
            // --- Agents ---
            "/v1/agents": {
                "get": {
                    "tags": ["Agents"],
                    "summary": "List agents",
                    "description": "List configured agents and their capabilities.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": {
                            "description": "Agent list",
                            "content": { "application/json": { "schema": { "type": "array", "items": { "$ref": "#/components/schemas/AgentResponse" } } } }
                        }
                    }
                },
                "post": {
                    "tags": ["Agents"],
                    "summary": "Create agent",
                    "description": "Create a new persistent agent.",
                    "security": [{ "BearerAuth": [] }],
                    "requestBody": {
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CreateAgentRequest" } } }
                    },
                    "responses": {
                        "201": {
                            "description": "Agent created",
                            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/AgentResponse" } } }
                        }
                    }
                }
            },
            "/v1/agents/{agent_id}": {
                "get": {
                    "tags": ["Agents"],
                    "summary": "Get agent",
                    "description": "Get details of a specific agent.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "agent_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "200": {
                            "description": "Agent details",
                            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/AgentResponse" } } }
                        },
                        "404": { "description": "Agent not found" }
                    }
                },
                "patch": {
                    "tags": ["Agents"],
                    "summary": "Update agent",
                    "description": "Update an existing agent's configuration.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "agent_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "requestBody": {
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UpdateAgentRequest" } } }
                    },
                    "responses": {
                        "200": {
                            "description": "Agent updated",
                            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/AgentResponse" } } }
                        },
                        "404": { "description": "Agent not found" }
                    }
                },
                "delete": {
                    "tags": ["Agents"],
                    "summary": "Delete agent",
                    "description": "Remove a persistent agent.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "agent_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "200": { "description": "Agent deleted" },
                        "404": { "description": "Agent not found" }
                    }
                }
            },
            "/v1/agents/{agent_id}/stats": {
                "get": {
                    "tags": ["Agents"],
                    "summary": "Agent statistics",
                    "description": "Get runtime statistics for a specific agent (runs, tokens, cost).",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "agent_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Agent statistics" } }
                }
            },
            // --- Topology ---
            "/v1/topology": {
                "get": {
                    "tags": ["Observability"],
                    "summary": "Agent topology",
                    "description": "Get the current agent topology DAG (nodes, edges, delegation relationships).",
                    "security": [{ "BearerAuth": [] }],
                    "responses": { "200": { "description": "Topology snapshot" } }
                }
            },
            // --- Tools ---
            "/v1/tools": {
                "get": {
                    "tags": ["Tools"],
                    "summary": "List tools",
                    "description": "List available tools with metadata and input schemas.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": {
                            "description": "Tool list",
                            "content": { "application/json": { "schema": { "type": "array", "items": { "$ref": "#/components/schemas/ToolResponse" } } } }
                        }
                    }
                }
            },
            "/v1/tool-execute": {
                "post": {
                    "tags": ["Tools"],
                    "summary": "Execute tool",
                    "description": "Execute a specific tool with the given input parameters.",
                    "security": [{ "BearerAuth": [] }],
                    "requestBody": {
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ToolExecuteRequest" } } }
                    },
                    "responses": {
                        "200": { "description": "Tool execution result" },
                        "404": { "description": "Tool not found" }
                    }
                }
            },
            // --- Memory ---
            "/v1/memory": {
                "get": {
                    "tags": ["Memory"],
                    "summary": "List memory",
                    "description": "Browse conversation memory entries.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": {
                            "description": "Memory entries",
                            "content": { "application/json": { "schema": { "type": "array", "items": { "$ref": "#/components/schemas/MemoryEntry" } } } }
                        }
                    }
                }
            },
            "/v1/memory/recall": {
                "post": {
                    "tags": ["Memory"],
                    "summary": "Recall memory",
                    "description": "Search conversation memory by query.",
                    "security": [{ "BearerAuth": [] }],
                    "requestBody": {
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/RecallRequest" } } }
                    },
                    "responses": {
                        "200": {
                            "description": "Matching memory entries",
                            "content": { "application/json": { "schema": { "type": "array", "items": { "$ref": "#/components/schemas/MemoryEntry" } } } }
                        }
                    }
                }
            },
            "/v1/memory/forget": {
                "post": {
                    "tags": ["Memory"],
                    "summary": "Forget memory",
                    "description": "Delete conversation memory entries matching the filter.",
                    "security": [{ "BearerAuth": [] }],
                    "requestBody": {
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ForgetRequest" } } }
                    },
                    "responses": { "200": { "description": "Memory deleted" } }
                }
            },
            // --- Cron ---
            "/v1/cron": {
                "get": {
                    "tags": ["Cron"],
                    "summary": "List cron jobs",
                    "description": "List all scheduled cron jobs.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": {
                            "description": "Cron job list",
                            "content": { "application/json": { "schema": { "type": "array", "items": { "$ref": "#/components/schemas/CronJob" } } } }
                        }
                    }
                },
                "post": {
                    "tags": ["Cron"],
                    "summary": "Create cron job",
                    "description": "Create a new scheduled cron job.",
                    "security": [{ "BearerAuth": [] }],
                    "requestBody": {
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CreateCronRequest" } } }
                    },
                    "responses": {
                        "201": {
                            "description": "Cron job created",
                            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CronJob" } } }
                        }
                    }
                }
            },
            "/v1/cron/{id}": {
                "patch": {
                    "tags": ["Cron"],
                    "summary": "Update cron job",
                    "description": "Update an existing cron job (schedule, action, enabled).",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "200": {
                            "description": "Cron job updated",
                            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CronJob" } } }
                        },
                        "404": { "description": "Cron job not found" }
                    }
                },
                "delete": {
                    "tags": ["Cron"],
                    "summary": "Delete cron job",
                    "description": "Remove a scheduled cron job.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "200": { "description": "Cron job deleted" },
                        "404": { "description": "Cron job not found" }
                    }
                }
            },
            // --- Config ---
            "/v1/config": {
                "get": {
                    "tags": ["Config"],
                    "summary": "Get configuration",
                    "description": "Get the current runtime configuration.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": { "200": { "description": "Current configuration (JSON)" } }
                },
                "put": {
                    "tags": ["Config"],
                    "summary": "Update configuration",
                    "description": "Update runtime configuration with validation and hot-reload.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "Configuration updated and hot-reloaded" },
                        "400": { "description": "Invalid configuration" }
                    }
                }
            },
            // --- Approvals ---
            "/v1/approvals": {
                "get": {
                    "tags": ["Admin"],
                    "summary": "List pending approvals",
                    "description": "List tool execution requests waiting for human approval.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": { "200": { "description": "Approval queue" } }
                }
            },
            // --- Events ---
            "/v1/events": {
                "get": {
                    "tags": ["Events"],
                    "summary": "Global event stream (SSE)",
                    "description": "Server-Sent Events stream for all gateway events. Supports `?token=` query param auth and `?topic=` filter.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "topic", "in": "query", "required": false, "schema": { "type": "string" }, "description": "Filter events by topic" }
                    ],
                    "responses": { "200": { "description": "SSE event stream" } }
                }
            },
            // --- Webhooks ---
            "/v1/webhook/{channel}": {
                "post": {
                    "tags": ["Webhooks"],
                    "summary": "Inbound webhook",
                    "description": "Receive inbound webhook payloads from external services (Slack, Discord, etc.).",
                    "parameters": [{ "name": "channel", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Webhook accepted" } }
                }
            },
            "/v1/hooks/{channel}/{agent_id}": {
                "post": {
                    "tags": ["Webhooks"],
                    "summary": "Agent-specific webhook",
                    "description": "Inbound webhook routed to a specific agent.",
                    "parameters": [
                        { "name": "channel", "in": "path", "required": true, "schema": { "type": "string" } },
                        { "name": "agent_id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Webhook accepted" } }
                }
            },
            // --- MCP ---
            "/mcp/message": {
                "post": {
                    "tags": ["MCP"],
                    "summary": "MCP message",
                    "description": "Handle a Model Context Protocol message (tool calls, resource access).",
                    "security": [{ "BearerAuth": [] }],
                    "responses": { "200": { "description": "MCP response" } }
                }
            },
            // --- A2A ---
            "/.well-known/agent.json": {
                "get": {
                    "tags": ["A2A"],
                    "summary": "Agent card",
                    "description": "Returns the A2A agent card describing this agent's capabilities.",
                    "responses": { "200": { "description": "Agent card (JSON)" } }
                }
            },
            "/a2a": {
                "post": {
                    "tags": ["A2A"],
                    "summary": "A2A RPC",
                    "description": "Google A2A protocol RPC endpoint for agent-to-agent communication.",
                    "responses": { "200": { "description": "A2A response" } }
                }
            },
            // --- Admin ---
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
            // --- Utility ---
            "/v1/openapi.json": {
                "get": {
                    "tags": ["Utility"],
                    "summary": "OpenAPI specification",
                    "description": "Returns this OpenAPI 3.1 specification as JSON.",
                    "responses": { "200": { "description": "OpenAPI 3.1 JSON specification" } }
                }
            },
            "/docs": {
                "get": {
                    "tags": ["Utility"],
                    "summary": "API documentation",
                    "description": "Interactive API documentation powered by Scalar.",
                    "responses": { "200": { "description": "HTML documentation page" } }
                }
            },
            // --- WebSocket ---
            "/ws/chat": {
                "get": {
                    "tags": ["WebSocket"],
                    "summary": "WebSocket chat",
                    "description": "Interactive WebSocket connection for real-time chat with the agent. Supports `?token=` query param auth.",
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
        assert!(json.contains("/health/live"), "missing /health/live");
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
        assert!(json.contains("/v1/tools"), "missing /v1/tools");
        assert!(
            json.contains("/v1/tool-execute"),
            "missing /v1/tool-execute"
        );
        assert!(json.contains("/v1/memory"), "missing /v1/memory");
        assert!(
            json.contains("/v1/memory/recall"),
            "missing /v1/memory/recall"
        );
        assert!(
            json.contains("/v1/memory/forget"),
            "missing /v1/memory/forget"
        );
        assert!(json.contains("/v1/cron"), "missing /v1/cron");
        assert!(json.contains("/v1/config"), "missing /v1/config");
        assert!(json.contains("/v1/topology"), "missing /v1/topology");
        assert!(json.contains("/v1/events"), "missing /v1/events");
        assert!(json.contains("/v1/approvals"), "missing /v1/approvals");
        assert!(json.contains("/mcp/message"), "missing /mcp/message");
        assert!(json.contains("/a2a"), "missing /a2a");
        assert!(
            json.contains("/.well-known/agent.json"),
            "missing /.well-known/agent.json"
        );
        assert!(json.contains("/docs"), "missing /docs");
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
        assert!(
            json.contains("AgentResponse"),
            "missing AgentResponse schema"
        );
        assert!(json.contains("ToolResponse"), "missing ToolResponse schema");
        assert!(json.contains("MemoryEntry"), "missing MemoryEntry schema");
        assert!(json.contains("CronJob"), "missing CronJob schema");
        assert!(json.contains("PairRequest"), "missing PairRequest schema");
    }

    #[test]
    fn spec_version_matches_crate() {
        let spec = build_openapi_spec();
        let version = spec["info"]["version"].as_str().expect("version present");
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn spec_has_tags() {
        let spec = build_openapi_spec();
        let tags = spec["tags"].as_array().expect("tags is array");
        assert!(
            tags.len() >= 10,
            "expected at least 10 tags, got {}",
            tags.len()
        );
    }
}
