---
title: Gateway Deployment Guide
description: Deploy AgentZero's HTTP gateway standalone, behind a reverse proxy, or in Docker.
---

AgentZero's gateway exposes an HTTP API for programmatic access to the agent runtime. This guide covers three deployment patterns.

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

### Dockerfile

```dockerfile
FROM rust:1.80-slim AS builder
WORKDIR /build
COPY . .
RUN cargo build -p agentzero --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/agentzero /usr/local/bin/agentzero

# Create data directory
RUN mkdir -p /data
ENV AGENTZERO_DATA_DIR=/data

EXPOSE 8080
ENTRYPOINT ["agentzero"]
CMD ["gateway", "--host", "0.0.0.0", "--port", "8080"]
```

### Build and run

```bash
docker build -t agentzero .
docker run -d \
  --name agentzero \
  -p 8080:8080 \
  -v agentzero-data:/data \
  -e OPENAI_API_KEY="sk-..." \
  agentzero
```

### docker-compose

```yaml
version: "3.8"
services:
  agentzero:
    build: .
    ports:
      - "8080:8080"
    volumes:
      - agentzero-data:/data
      - ./agentzero.toml:/data/agentzero.toml:ro
    environment:
      - OPENAI_API_KEY=${OPENAI_API_KEY}
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 30s
      timeout: 5s
      retries: 3

volumes:
  agentzero-data:
```

### Configuration in Docker

Mount your `agentzero.toml` into the container's data directory, or use environment variables:

```bash
docker run -d \
  -p 8080:8080 \
  -v ./agentzero.toml:/data/agentzero.toml:ro \
  -e OPENAI_API_KEY="sk-..." \
  agentzero
```

:::caution
When running in Docker, set `allow_public_bind = true` in your gateway config since Docker networking requires binding to `0.0.0.0`.
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
