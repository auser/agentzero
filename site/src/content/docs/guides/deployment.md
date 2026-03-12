---
title: Gateway Deployment Guide
description: Deploy AgentZero's HTTP gateway standalone, behind a reverse proxy, or in Docker.
---

AgentZero's gateway exposes an HTTP API for programmatic access to the agent runtime. This guide covers three deployment patterns.

:::tip
For a complete production-hardened setup covering encryption, TLS, authentication, security policies, and monitoring, see the [Production Setup Guide](/guides/production/).
:::

## Standalone (Direct)

The simplest deployment — run the gateway directly:

```bash
# Foreground (for development/testing)
agentzero gateway --host 127.0.0.1 --port 8080

# Background daemon (for production)
agentzero daemon start --host 127.0.0.1 --port 8080
```

Verify it's running:

```bash
curl http://127.0.0.1:8080/health
# → {"status":"ok"}
```

### Configuration

```toml
[gateway]
host = "127.0.0.1"          # bind address
port = 42617                 # bind port
require_pairing = true       # require OTP pairing for auth
allow_public_bind = false    # must be true for non-loopback
```

### Daemon lifecycle

```bash
agentzero daemon start --port 8080   # start in background
agentzero daemon status              # check running state
agentzero daemon status --json       # JSON output with PID, uptime, log path
agentzero daemon stop                # graceful shutdown (SIGTERM → SIGKILL)
```

Logs are written to `{data_dir}/daemon.log` with automatic rotation (10 MB max, 5 rotated files kept).

### System service

For auto-start on boot:

```bash
agentzero service install    # auto-detects systemd or openrc
agentzero service start
agentzero service status
```

---

## Behind a Reverse Proxy

For production deployments with TLS, load balancing, or public access.

### nginx

```nginx
upstream agentzero {
    server 127.0.0.1:8080;
}

server {
    listen 443 ssl;
    server_name agent.example.com;

    ssl_certificate     /etc/ssl/certs/agent.pem;
    ssl_certificate_key /etc/ssl/private/agent.key;

    location / {
        proxy_pass http://agentzero;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # WebSocket support for /ws/chat
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_read_timeout 3600s;
    }
}
```

### Caddy

```
agent.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

Caddy handles TLS automatically via Let's Encrypt.

### Important notes

- The gateway binds to `127.0.0.1` by default. Keep it on localhost when behind a proxy.
- WebSocket connections (`/ws/chat`) require the proxy to support HTTP Upgrade.
- Set appropriate `proxy_read_timeout` for long-running agent conversations.
- The `/health` endpoint requires no authentication — use it for load balancer health checks.

---

## Docker

The repository includes a `Dockerfile` and `docker-compose.yml` at the project root. The image builds the **server variant** (~7 MB binary) with SQLite, WASM plugins, and the HTTP gateway. TUI and interactive features are excluded since they are not needed in a container.

### Quick start

```bash
git clone https://github.com/auser/agentzero.git
cd agentzero
echo "OPENAI_API_KEY=sk-..." > .env
docker compose up -d
curl http://localhost:8080/health
```

### Build and run manually

```bash
docker build -t agentzero .
docker run -d \
  --name agentzero \
  -p 8080:8080 \
  -v agentzero-data:/data \
  -e OPENAI_API_KEY="sk-..." \
  agentzero
```

Or use the Justfile shortcuts:

```bash
just docker-build
just docker-up      # docker compose up -d
just docker-down    # docker compose down
```

### Resource limits and production mode

The default `docker-compose.yml` includes resource constraints (512 MB memory, 1.0 CPU) and supports `AGENTZERO_ENV=production` for startup validation:

```yaml
environment:
  - OPENAI_API_KEY=${OPENAI_API_KEY:-}
  - AGENTZERO_ENV=production    # enforces TLS + auth on startup
deploy:
  resources:
    limits:
      memory: 512M
      cpus: "1.0"
    reservations:
      memory: 128M
      cpus: "0.25"
```

The healthcheck automatically falls back to HTTPS if HTTP fails, supporting both TLS and non-TLS deployments.

### Custom configuration

The image ships with a minimal default config that sets `allow_public_bind = true` (required for Docker networking). To use your own config, mount it into the container:

```bash
docker run -d \
  -p 8080:8080 \
  -v agentzero-data:/data \
  -v ./agentzero.toml:/data/agentzero.toml:ro \
  -e OPENAI_API_KEY="sk-..." \
  -e AGENTZERO_CONFIG=/data/agentzero.toml \
  agentzero
```

:::caution
When running in Docker, your config must include `allow_public_bind = true` in the `[gateway]` section since Docker networking requires binding to `0.0.0.0`.
:::

---

## Endpoint Reference

| Endpoint | Method | Auth | Description |
|---|---|---|---|
| `/health` | GET | None | Health check probe |
| `/api/chat` | POST | Bearer | Send a chat message |
| `/v1/chat/completions` | POST | Bearer | OpenAI-compatible completions |
| `/v1/models` | GET | Bearer | List available models |
| `/ws/chat` | GET | Bearer | WebSocket chat |
| `/pair` | POST | None | Exchange pairing code for bearer token |
| `/v1/ping` | POST | Bearer | Connectivity check |
| `/v1/webhook/:channel` | POST | Bearer | Channel message dispatch |
| `/metrics` | GET | None | Prometheus-style metrics |

---

## Security Checklist

- [ ] Keep the gateway on `127.0.0.1` unless you need public access
- [ ] Set `allow_public_bind = true` explicitly if binding to `0.0.0.0`
- [ ] Use TLS termination via a reverse proxy for public deployments
- [ ] Use the pairing flow (`require_pairing = true`) — the default
- [ ] Set rate limiting appropriate for your use case
- [ ] Monitor `/health` and `/metrics` endpoints
- [ ] Configure log rotation (automatic in daemon mode)
