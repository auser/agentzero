---
title: Production Setup
description: Deploy a private, encrypted, hardened, and monitored AgentZero instance for production use.
---

This guide walks through setting up a **private, production-ready, secure, and encrypted** AgentZero deployment. By the end you will have:

- All data encrypted at rest (conversations, credentials, tokens)
- TLS encryption in transit via a hardened reverse proxy
- Pairing-based authentication with OTP for sensitive operations
- Restrictive autonomy controls, audit logging, and leak prevention
- Observability via OpenTelemetry and Prometheus metrics
- Cost guardrails and action-rate limits

For basic gateway deployment patterns (standalone, Docker, daemon lifecycle), see the [Gateway Deployment Guide](/guides/deployment/). For end-to-end encrypted transport and privacy modes, see the [Privacy Guide](/guides/privacy/).

---

## 1. Encryption Key

AgentZero encrypts all persistent data with a single 32-byte key:

| Store | Cipher | What's protected |
|---|---|---|
| Conversation memory | SQLCipher (AES-256-CBC) | All chat history |
| Credential stores | XChaCha20Poly1305 (AEAD) | API keys, OAuth tokens, auth profiles |
| Gateway tokens | XChaCha20Poly1305 (AEAD) | Paired bearer tokens |

### Generate your key

```bash
openssl rand -base64 32 > ~/.agentzero/.agentzero-data.key
chmod 600 ~/.agentzero/.agentzero-data.key
```

The key file is read automatically at startup. Alternatively, set it as an environment variable:

```bash
export AGENTZERO_DATA_KEY="$(cat ~/.agentzero/.agentzero-data.key)"
```

Both base64 and 64-character hex formats are accepted.

:::tip
If you skip this step, AgentZero auto-generates a key on first run and writes it to `~/.agentzero/.agentzero-data.key` with mode `0600`. Explicit creation is recommended in production so you can back up the key securely.
:::

:::caution
**Back up this key.** Without it, encrypted databases and credential stores are unrecoverable. Store a copy in your secrets manager or offline backup.
:::

### Legacy migration

If you have an existing plaintext SQLite database, AgentZero automatically encrypts it with SQLCipher on first open. The original plaintext file is replaced atomically — no manual steps required.

---

## 2. Production Configuration

Create `~/.agentzero/agentzero.toml` with hardened defaults. Every section below is annotated with why each value matters.

```toml
# ─── Provider ────────────────────────────────────────────
[provider]
kind = "openrouter"                              # or anthropic, openai, ollama
base_url = "https://openrouter.ai/api/v1"
model = "anthropic/claude-sonnet-4-6"
default_temperature = 0.3                        # lower = more deterministic

# ─── Memory ──────────────────────────────────────────────
[memory]
backend = "sqlite"
sqlite_path = "~/.agentzero/agentzero.db"        # encrypted via SQLCipher

# ─── Agent ───────────────────────────────────────────────
[agent]
mode = "production"
max_tool_iterations = 15                         # cap runaway tool loops
request_timeout_ms = 60000                       # 60s per LLM request
memory_window_size = 40                          # context window (messages)
max_prompt_chars = 8000
parallel_tools = true                            # concurrent tool execution
tool_dispatcher = "auto"
compact_context = true                           # compress large contexts

# Loop detection — catch stuck agents early
loop_detection_no_progress_threshold = 3
loop_detection_ping_pong_cycles = 2
loop_detection_failure_streak = 3

# ─── Security ────────────────────────────────────────────
[security]
allowed_root = "/srv/agentzero/workspace"        # restrict filesystem scope
allowed_commands = ["ls", "pwd", "cat", "echo", "git", "cargo", "npm"]

[security.read_file]
max_read_bytes = 65536                           # 64 KiB
allow_binary = false

[security.write_file]
enabled = true
max_write_bytes = 65536

[security.shell]
max_args = 8
max_arg_length = 256
max_output_bytes = 16384
forbidden_chars = ";&|><$`\n\r"
context_aware_parsing = true

[security.audit]
enabled = true
path = "/var/log/agentzero/audit.log"

[security.url_access]
block_private_ip = true
allow_loopback = false
enforce_domain_allowlist = false                  # set true + populate below to restrict
domain_allowlist = []
domain_blocklist = []

[security.otp]
enabled = true
method = "totp"
token_ttl_secs = 30
cache_valid_secs = 300
gated_actions = ["shell", "file_write", "browser_open", "browser", "memory_forget"]

[security.estop]
enabled = true
state_file = "~/.agentzero/estop-state.json"
require_otp_to_resume = true

[security.outbound_leak_guard]
enabled = true
action = "block"                                 # block, not just redact, in production
sensitivity = 0.8                                # higher = catches more

[security.perplexity_filter]
enable_perplexity_filter = true

[security.syscall_anomaly]
enabled = true
strict_mode = true                               # strict in production
alert_on_unknown_syscall = true
max_denied_events_per_minute = 5
max_alerts_per_minute = 30

[security.mcp]
enabled = false                                  # enable only if needed
allowed_servers = []                             # restrict to specific servers by name

[security.plugin]
enabled = false                                  # enable only if needed

# ─── Autonomy ────────────────────────────────────────────
[autonomy]
level = "supervised"                             # human-in-the-loop
workspace_only = true
forbidden_paths = ["/etc", "/root", "/proc", "/sys", "~/.ssh", "~/.gnupg", "~/.aws"]
max_actions_per_hour = 30
max_cost_per_day_cents = 500
require_approval_for_medium_risk = true
block_high_risk_commands = true

# ─── Gateway ─────────────────────────────────────────────
[gateway]
host = "127.0.0.1"                               # localhost only — proxy handles public
port = 42617
require_pairing = true
allow_public_bind = false

# ─── Observability ───────────────────────────────────────
[observability]
backend = "otel"
otel_endpoint = "http://localhost:4318"           # your OTel collector
otel_service_name = "agentzero"
runtime_trace_mode = "file"
runtime_trace_path = "/var/log/agentzero/runtime-trace.jsonl"
runtime_trace_max_entries = 500

# ─── Cost Controls ───────────────────────────────────────
[cost]
enabled = true
daily_limit_usd = 10.0
monthly_limit_usd = 200.0
warn_at_percent = 80
```

### Production mode validation

Set `AGENTZERO_ENV=production` to enforce strict startup validation:

```bash
export AGENTZERO_ENV=production
```

In production mode, the gateway validates on startup that:
- TLS is configured (`[gateway.tls]`) **or** `allow_insecure = true` is explicitly set
- Authentication is enabled (`require_pairing = true`)

If validation fails, the gateway refuses to start with a clear error message. In development mode (default), these checks are skipped.

Verify your config:

```bash
agentzero config show          # secrets masked
agentzero config show --raw    # secrets visible (for debugging)
```

---

## 3. TLS Termination

The gateway speaks plain HTTP by design — TLS is handled by a reverse proxy in front of it. This separation keeps certificate management out of the application layer.

### nginx (recommended for full control)

```nginx
# /etc/nginx/sites-available/agentzero
upstream agentzero_backend {
    server 127.0.0.1:42617;
    keepalive 16;
}

server {
    listen 80;
    server_name agent.example.com;
    return 301 https://$server_name$request_uri;
}

server {
    listen 443 ssl http2;
    server_name agent.example.com;

    # ── TLS certificates ──────────────────────────────
    ssl_certificate     /etc/letsencrypt/live/agent.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/agent.example.com/privkey.pem;

    # ── Protocol & ciphers ────────────────────────────
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305;
    ssl_prefer_server_ciphers on;
    ssl_session_timeout 1d;
    ssl_session_cache shared:SSL:10m;
    ssl_session_tickets off;

    # ── OCSP stapling ─────────────────────────────────
    ssl_stapling on;
    ssl_stapling_verify on;
    ssl_trusted_certificate /etc/letsencrypt/live/agent.example.com/chain.pem;
    resolver 1.1.1.1 8.8.8.8 valid=300s;
    resolver_timeout 5s;

    # ── Security headers ──────────────────────────────
    add_header Strict-Transport-Security "max-age=63072000; includeSubDomains; preload" always;
    add_header X-Frame-Options DENY always;
    add_header X-Content-Type-Options nosniff always;
    add_header Referrer-Policy strict-origin-when-cross-origin always;
    add_header X-XSS-Protection "1; mode=block" always;

    # ── Request limits ────────────────────────────────
    client_max_body_size 1m;

    # ── Proxy to AgentZero ────────────────────────────
    location / {
        proxy_pass http://agentzero_backend;
        proxy_http_version 1.1;

        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # WebSocket support (for /ws/chat)
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";

        # Timeouts for long-running agent conversations
        proxy_read_timeout 3600s;
        proxy_send_timeout 300s;
        proxy_connect_timeout 10s;
    }

    # ── Health check (no auth, for load balancers) ────
    location = /health {
        proxy_pass http://agentzero_backend/health;
        access_log off;
    }
}
```

Enable and test:

```bash
sudo ln -s /etc/nginx/sites-available/agentzero /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
curl -I https://agent.example.com/health
```

### Caddy (automatic TLS)

If you prefer automatic certificate management with zero config:

```
agent.example.com {
    reverse_proxy 127.0.0.1:42617
}
```

Caddy handles Let's Encrypt issuance, renewal, OCSP stapling, and HTTP/2 automatically.

---

## 4. Authentication

### Pairing flow (recommended)

With `require_pairing = true` (the default), the gateway generates a one-time pairing code on startup:

```
─────────────────────────────────────────
  AgentZero Gateway
  Listening on 127.0.0.1:42617
  Pairing code: A7X-K9M-2PQ
─────────────────────────────────────────
```

Exchange the pairing code for a persistent bearer token:

```bash
curl -X POST http://127.0.0.1:42617/pair \
  -H "X-Pairing-Code: A7X-K9M-2PQ"
# → {"token":"eyJhb..."}
```

Use the token for all subsequent requests:

```bash
curl https://agent.example.com/v1/models \
  -H "Authorization: Bearer eyJhb..."
```

Paired tokens are persisted (encrypted) at `~/.agentzero/gateway-paired-tokens.json`. To revoke all tokens and generate a new pairing code:

```bash
agentzero gateway --new-pairing
```

### Static bearer token (alternative)

For automation where pairing isn't practical:

```bash
export AGENTZERO_GATEWAY_BEARER_TOKEN="$(openssl rand -hex 32)"
```

### OTP for sensitive operations

With `[security.otp]` enabled, high-risk operations (shell execution, file writes, browser actions) require a time-based one-time password. The TOTP secret is displayed on gateway startup — enroll it in your authenticator app.

---

## 5. Security Hardening

The production config in Step 2 enables all of these. Here's what each layer protects:

### Filesystem isolation

| Setting | Effect |
|---|---|
| `allowed_root` | Agent cannot read/write outside this directory |
| `workspace_only = true` | Tool operations confined to workspace |
| `forbidden_paths` | Explicit blocklist for sensitive directories |

### Shell restrictions

| Setting | Effect |
|---|---|
| `allowed_commands` | Only listed commands can execute |
| `max_args = 8` | Limits argument injection surface |
| `forbidden_chars` | Blocks shell metacharacters (`;&\|><$\``) |
| `context_aware_parsing` | Validates full command semantics |

### Network controls

| Setting | Effect |
|---|---|
| `block_private_ip = true` | Prevents SSRF to internal networks |
| `allow_loopback = false` | Blocks requests to localhost services |
| `domain_allowlist` | Restrict outbound to known domains (opt-in) |

### Outbound leak guard

Scans all outbound channel messages (Telegram, Slack, Discord) for credentials:

- API keys (`sk-*`, `AKIA*`, `ghp_*`, `xox*-*`)
- Bearer tokens and JWTs
- Private key blocks
- High-entropy strings (Shannon entropy detection)

Set `action = "block"` in production to reject messages containing credentials rather than redacting them.

### Audit logging

All tool invocations are logged to `[security.audit].path` with timestamps, tool names, arguments, and results. Rotate with your standard log infrastructure.

### Emergency stop

The `[security.estop]` feature provides a kill switch. When triggered, the agent halts all operations. Resuming requires OTP verification when `require_otp_to_resume = true`.

### Prompt injection defense

The perplexity filter detects anomalous prompt patterns (obfuscated text, unusual symbol ratios) and blocks them before they reach the LLM.

### Syscall monitoring

In `strict_mode`, the syscall anomaly detector alerts on unexpected system call patterns and rate-limits denied events to prevent log flooding.

---

## 6. Observability

### OpenTelemetry

With `[observability].backend = "otel"`, AgentZero exports traces and metrics to your collector:

```toml
[observability]
backend = "otel"
otel_endpoint = "http://localhost:4318"    # OTLP HTTP endpoint
otel_service_name = "agentzero"
```

Compatible with Jaeger, Grafana Tempo, Datadog, and any OTLP-compatible backend.

### Runtime traces

```toml
runtime_trace_mode = "file"
runtime_trace_path = "/var/log/agentzero/runtime-trace.jsonl"
runtime_trace_max_entries = 500
```

Each agent turn is logged as a JSON line — useful for debugging tool execution sequences.

### Prometheus metrics

Scrape the `/metrics` endpoint (no auth required):

```yaml
# prometheus.yml
scrape_configs:
  - job_name: agentzero
    static_configs:
      - targets: ["127.0.0.1:42617"]
    metrics_path: /metrics
```

### Health checks

The `/health` endpoint returns `{"status":"ok"}` with no authentication required — use it for load balancer probes, uptime monitors, and readiness checks.

### Log rotation

In daemon mode, logs are automatically rotated:
- **Max size:** 10 MB per file
- **Retention:** 5 rotated files
- **Location:** `{data_dir}/daemon.log`

---

## 7. Cost Controls

Prevent runaway spending with hard limits:

```toml
[cost]
enabled = true
daily_limit_usd = 10.0        # hard stop per day
monthly_limit_usd = 200.0     # hard stop per month
warn_at_percent = 80           # alert at 80% of limit

[autonomy]
max_actions_per_hour = 30      # rate-limit tool invocations
max_cost_per_day_cents = 500   # redundant safety net
```

When a limit is hit, the agent stops accepting new requests until the budget window resets.

---

## 8. Deploy as System Service

### Install and start

```bash
# Install service (auto-detects systemd or openrc)
agentzero service install

# Start the service
agentzero service start

# Verify
agentzero service status
agentzero daemon status --json
```

### Verify the deployment

```bash
# Health check
curl -s http://127.0.0.1:42617/health
# → {"status":"ok"}

# Config validation
agentzero config show

# If using a reverse proxy:
curl -I https://agent.example.com/health
```

---

## Optional: Docker Deployment

If you prefer containers, here is a production-hardened `docker-compose.yml`. Bare-metal via system services (above) is the primary recommended path.

```yaml
services:
  agentzero:
    build: .
    ports:
      - "127.0.0.1:42617:8080"               # bind to localhost only
    volumes:
      - agentzero-data:/data
      - ./agentzero.toml:/data/agentzero.toml:ro   # read-only config
    env_file:
      - .env.production                        # API keys, AGENTZERO_DATA_KEY
    environment:
      - AGENTZERO_DATA_DIR=/data
      - AGENTZERO_ENV=production                 # enforce TLS + auth validation
    restart: unless-stopped
    read_only: true                            # read-only root filesystem
    tmpfs:
      - /tmp:size=64m
    deploy:
      resources:
        limits:
          memory: 512m
          cpus: "1.0"
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 30s
      timeout: 5s
      retries: 3
      start_period: 10s

volumes:
  agentzero-data:
```

Place secrets in `.env.production` (not committed to version control):

```bash
OPENAI_API_KEY=sk-...
AGENTZERO_DATA_KEY=<base64-encoded-key>
AGENTZERO_GATEWAY_BEARER_TOKEN=<hex-token>
```

:::caution
When running in Docker, set `allow_public_bind = true` in your gateway config since Docker networking requires binding to `0.0.0.0`. The `127.0.0.1:42617:8080` port mapping in compose ensures the host only exposes it on localhost.
:::

---

## Production Readiness Checklist

### Encryption

- [ ] Data key generated and backed up securely
- [ ] Key file has mode `0600`
- [ ] Conversation database is SQLCipher-encrypted (check with `file ~/.agentzero/agentzero.db`)

### Network

- [ ] Gateway bound to `127.0.0.1` (not `0.0.0.0`)
- [ ] TLS termination configured (nginx or Caddy)
- [ ] HSTS header present (`curl -I https://...`)
- [ ] HTTP → HTTPS redirect active

### Authentication

- [ ] `require_pairing = true`
- [ ] Pairing code securely distributed to clients
- [ ] OTP enabled for sensitive operations
- [ ] No open/unauthenticated access

### Security

- [ ] `allowed_root` points to a dedicated workspace directory
- [ ] `workspace_only = true`
- [ ] `forbidden_paths` covers sensitive system directories
- [ ] Leak guard set to `block` (not `redact`)
- [ ] Audit logging enabled with log rotation
- [ ] Emergency stop enabled
- [ ] `block_private_ip = true`

### Autonomy

- [ ] `level = "supervised"`
- [ ] `max_actions_per_hour` configured
- [ ] `block_high_risk_commands = true`

### Observability

- [ ] OpenTelemetry exporting to collector
- [ ] `/metrics` endpoint scraped by Prometheus
- [ ] `/health` endpoint monitored
- [ ] Daemon log rotation active

### Cost

- [ ] Daily and monthly limits set
- [ ] Warning threshold configured

### Backup & Recovery

- [ ] Encryption key backed up to secrets manager
- [ ] Regular encrypted backups via `agentzero backup export`
- [ ] Backup restore tested via `agentzero backup restore --force`
- [ ] OpenAPI spec available at `/v1/openapi.json`

### Environment

- [ ] `AGENTZERO_ENV=production` set in deployment environment
- [ ] Production validation passing (TLS + auth enforced)

### Verification commands

```bash
# Config is valid and loaded
agentzero config show

# Gateway is running and healthy
curl -s http://127.0.0.1:42617/health

# TLS is working (if using reverse proxy)
curl -sI https://agent.example.com/health | head -5

# Service is running
agentzero daemon status --json

# Audit log is being written
tail -1 /var/log/agentzero/audit.log
```
