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
| `GET` | `/health` | None | Service health probe |
| `POST` | `/v1/ping` | Bearer | Echo test endpoint |
| `POST` | `/v1/webhook/:channel` | Bearer | Channel message dispatch |

## Authentication

The gateway uses a pairing flow:

1. On first start, the gateway prints a one-time pairing code to the terminal
2. POST the pairing code to get a bearer token
3. Use the bearer token in subsequent requests

```bash
# Step 1: Note the pairing code from gateway startup output
# Step 2: Health check (no auth required)
curl http://127.0.0.1:42617/health
```

```json
{ "status": "ok" }
```

```bash
# Step 3: Authenticated request
curl -X POST http://127.0.0.1:42617/v1/ping \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json"
```

## Middleware

The gateway includes built-in middleware for production hardening:

**Rate Limiting** — Sliding window counter that rejects excess requests with `429 Too Many Requests`.

**Request Size Limits** — Rejects requests with `Content-Length` exceeding the configured maximum (default: 10 MB) with `413 Payload Too Large`.

**CORS** — Configurable origin allowlist for browser clients. Supports exact origin matching and wildcard (`*`). Handles preflight `OPTIONS` requests automatically.

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
