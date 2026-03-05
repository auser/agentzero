---
title: Gateway API
description: HTTP gateway endpoints, authentication, and usage examples.
---

The AgentZero gateway exposes a localhost HTTP API for programmatic access to the agent runtime.

## Starting the Gateway

```bash
agentzero gateway
agentzero gateway --host 127.0.0.1 --port 8080
agentzero gateway --new-pairing
```

:::caution
The gateway binds to `127.0.0.1` (localhost only) by default. Setting `allow_public_bind = true` in config is required for non-loopback interfaces. Use a reverse proxy or tunnel for remote access.
:::

## Endpoints

| Method | Path | Auth | Description |
|---|---|---|---|
| `GET` | `/` | None | Dashboard HTML page |
| `GET` | `/health` | None | Service health probe |
| `GET` | `/metrics` | None | Prometheus-compatible metrics |
| `POST` | `/pair` | Pairing code | Exchange pairing code for bearer token |
| `POST` | `/v1/ping` | Bearer | Echo test endpoint |
| `POST` | `/v1/webhook/:channel` | Bearer | Channel message dispatch |
| `POST` | `/api/chat` | Bearer | Chat with agent (JSON response) |
| `POST` | `/v1/chat/completions` | Bearer | OpenAI-compatible chat completions (supports SSE streaming) |
| `GET` | `/v1/models` | Bearer | List available models (OpenAI-compatible) |
| `GET` | `/ws/chat` | Bearer | WebSocket chat with streaming agent responses |
| `POST` | `/webhook` | Bearer | Legacy webhook endpoint |
| `GET` | `/v1/privacy/info` | None | Privacy capabilities discovery (feature-gated) |
| `POST` | `/v1/noise/handshake/step1` | None | Noise XX handshake step 1 (feature-gated) |
| `POST` | `/v1/noise/handshake/step2` | None | Noise XX handshake step 2 (feature-gated) |
| `POST` | `/v1/noise/handshake/ik` | None | Noise IK handshake (feature-gated) |
| `POST` | `/v1/relay/submit` | None | Submit sealed envelope (relay mode, feature-gated) |
| `GET` | `/v1/relay/poll/:routing_id` | None | Poll sealed envelopes (relay mode, feature-gated) |

## Authentication

The gateway uses a pairing flow:

1. On first start, the gateway prints a one-time pairing code to the terminal
2. POST the pairing code to `/pair` to get a bearer token
3. Use the bearer token in subsequent requests

```bash
# Health check (no auth required)
curl http://127.0.0.1:42617/health
```

```json
{ "status": "ok" }
```

```bash
# Exchange pairing code for token
curl -X POST http://127.0.0.1:42617/pair \
  -H "X-Pairing-Code: <code-from-terminal>"
```

```bash
# Authenticated request
curl -X POST http://127.0.0.1:42617/v1/ping \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json"
```

## Chat Endpoints

### Agent Chat (`POST /api/chat`)

Send a message to the agent and receive a complete JSON response.

```bash
curl -X POST http://127.0.0.1:42617/api/chat \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"message": "What is the weather?", "context": ""}'
```

```json
{ "message": "I can help with that...", "tokens_used_estimate": 42 }
```

Returns `503 Service Unavailable` if the gateway was started without agent configuration.

### OpenAI-Compatible Completions (`POST /v1/chat/completions`)

Accepts the standard OpenAI chat completions format. Set `stream: true` for SSE streaming.

```bash
# Non-streaming
curl -X POST http://127.0.0.1:42617/v1/chat/completions \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4o-mini", "messages": [{"role": "user", "content": "hello"}]}'
```

```bash
# Streaming (SSE)
curl -X POST http://127.0.0.1:42617/v1/chat/completions \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4o-mini", "messages": [{"role": "user", "content": "hello"}], "stream": true}'
```

SSE events follow the OpenAI format:

```
data: {"id":"chatcmpl-...","choices":[{"index":0,"delta":{"content":"token"},"finish_reason":null}]}

data: [DONE]
```

The `model` field is passed through to the agent, allowing model override per request.

### WebSocket Chat (`GET /ws/chat`)

Upgrade to a WebSocket connection for bidirectional streaming chat. Send a text message and receive streaming delta frames:

```json
// Incoming delta
{"type": "delta", "delta": "partial response text"}

// Stream complete
{"type": "done"}

// Error
{"type": "error", "message": "description"}
```

### Models (`GET /v1/models`)

List available models in OpenAI-compatible format.

```bash
curl http://127.0.0.1:42617/v1/models \
  -H "Authorization: Bearer <token>"
```

## Privacy Endpoints

These endpoints are available when the gateway is built with the `privacy` Cargo feature and privacy is configured. See the [Privacy Guide](/guides/privacy/) for details.

### Privacy Info (`GET /v1/privacy/info`)

Discover gateway privacy capabilities before initiating a handshake.

```json
{
  "noise_enabled": true,
  "handshake_pattern": "XX",
  "public_key": "<base64-encoded X25519 public key>",
  "key_fingerprint": "a1b2c3d4e5f6a1b2",
  "sealed_envelopes_enabled": false,
  "relay_mode": false,
  "supported_patterns": ["XX", "IK"]
}
```

### Noise Handshake (XX Pattern)

Two-step mutual authentication handshake:

1. `POST /v1/noise/handshake/step1` — Client sends `{"client_message": "<base64>"}`. Server returns `{"server_message": "<base64>"}`.
2. `POST /v1/noise/handshake/step2` — Client sends `{"client_message": "<base64>"}`. Server returns `{"session_id": "<64-char hex>"}`.

### Noise Handshake (IK Pattern)

Single round-trip handshake when the client knows the server's public key:

`POST /v1/noise/handshake/ik` — Client sends `{"client_message": "<base64>", "server_public_key": "<base64>"}`. Server returns `{"server_message": "<base64>", "session_id": "<64-char hex>"}`.

### Noise Transport

After handshake, all requests include `X-Noise-Session: <session_id>` header with encrypted body. The gateway middleware transparently decrypts request bodies and encrypts response bodies.

### Relay (Sealed Envelopes)

Available when `relay_mode = true` in gateway config:

- `POST /v1/relay/submit` — Submit a sealed envelope. Body: `{"routing_id": "<64-char hex>", "payload": "<base64>", "nonce": "<base64 24-byte>", "ttl_secs": 300}`. Returns HTTP 409 on replay (duplicate nonce).
- `GET /v1/relay/poll/:routing_id` — Poll for envelopes addressed to a routing ID.

The relay strips identifying headers (`X-Forwarded-For`, `X-Real-IP`, `Via`) from all requests.

## Metrics

The `/metrics` endpoint exposes Prometheus-compatible metrics for monitoring:

- `gateway_requests_total{method, path, status}` — Request counter by method, path, and status code
- `gateway_request_duration_seconds{method, path}` — Request latency histogram
- `gateway_errors_total{error_type}` — Error counter by structured error type
- `gateway_ws_connections_total` — WebSocket connection counter
- `gateway_active_connections` — Current active connection gauge

**Privacy metrics** (when `privacy` feature is enabled):

- `agentzero_noise_sessions_active` — Active Noise sessions (gauge)
- `agentzero_noise_handshakes_total{result}` — Handshake attempts by result (counter)
- `agentzero_relay_mailbox_envelopes` — Envelopes in relay mailboxes (gauge)
- `agentzero_relay_submit_total` — Total envelope submissions (counter)
- `agentzero_key_rotation_total{epoch}` — Key rotation events (counter)
- `agentzero_privacy_encrypt_duration_seconds` — Encrypt/decrypt latency (histogram)

**Prometheus scrape config:**

```yaml
scrape_configs:
  - job_name: agentzero-gateway
    static_configs:
      - targets: ['127.0.0.1:42617']
    metrics_path: /metrics
    scrape_interval: 15s
```

## Models

The `/v1/models` endpoint dynamically returns all models from the provider catalog. The response follows the OpenAI format:

```json
{
  "object": "list",
  "data": [
    { "id": "claude-sonnet-4-6", "object": "model", "owned_by": "anthropic" },
    { "id": "gpt-4o", "object": "model", "owned_by": "openai" }
  ]
}
```

## Error Responses

All error responses use a structured JSON format:

```json
{
  "error": {
    "type": "auth_required",
    "message": "authentication required"
  }
}
```

| Error Type | HTTP Status | Description |
|---|---|---|
| `auth_required` | 401 | No bearer token provided |
| `auth_failed` | 403 | Invalid token or pairing code |
| `not_found` | 404 | Unknown endpoint or resource |
| `agent_unavailable` | 503 | Gateway started without agent config |
| `agent_execution_failed` | 500 | Agent runtime error |
| `rate_limited` | 429 | Rate limit exceeded |
| `payload_too_large` | 413 | Request body too large |
| `bad_request` | 400 | Malformed request |

## WebSocket Behavior

The WebSocket endpoint (`/ws/chat`) includes production hardening:

- **Heartbeat** — Server sends a ping every 30 seconds. If no pong is received within 60 seconds, the connection is closed.
- **Idle timeout** — Connections with no messages for 5 minutes are automatically closed.
- **Binary rejection** — Binary WebSocket frames are rejected with an error JSON frame.

## Middleware

The gateway includes built-in middleware for production hardening:

**Rate Limiting** — Sliding window counter that rejects excess requests with `429 Too Many Requests`. Default: 600 requests per 60-second window (10 req/s). Set `rate_limit_max = 0` in config to disable.

**Request Size Limits** — Rejects requests with `Content-Length` exceeding the configured maximum (default: 1 MB) with `413 Payload Too Large`.

**CORS** — Configurable origin allowlist for browser clients. Supports exact origin matching and wildcard (`*`). Handles preflight `OPTIONS` requests automatically.

**Request Metrics** — All requests are automatically instrumented with Prometheus counters and histograms.

**Graceful Shutdown** — On `SIGTERM` or `SIGINT`, the gateway drains active connections before exiting.

## Configuration

```toml
[gateway]
host = "127.0.0.1"          # bind interface
port = 42617                 # bind port
require_pairing = true       # require OTP pairing
allow_public_bind = false    # allow non-loopback bind

[gateway.node_control]
enabled = false
# auth_token = "your-token"
allowed_node_ids = []
```

## Daemon Mode

Run the gateway as a background process with automatic local AI service discovery, PID file management, and log rotation:

```bash
# Start in background
agentzero daemon start --host 127.0.0.1 --port 8080

# Check status (includes PID, uptime, address)
agentzero daemon status
agentzero daemon status --json

# Stop the daemon
agentzero daemon stop

# Run in foreground (for debugging or systemd)
agentzero daemon start --foreground
```

Daemon logs are written to `{data_dir}/daemon.log` with automatic rotation (10 MB max, 5 rotated files).

## Service Installation

Install as a system service for automatic startup:

```bash
# Auto-detect init system (systemd or openrc)
agentzero service install

# Explicit init system
agentzero service --service-init systemd install
agentzero service --service-init openrc install

# Lifecycle
agentzero service start
agentzero service status
agentzero service restart
agentzero service stop
agentzero service uninstall
```

**systemd** installs as a user-level unit (no root required). **OpenRC** installs system-wide (requires sudo).
