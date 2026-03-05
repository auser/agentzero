---
title: Privacy Guide
description: Configure privacy modes, per-component boundaries, Noise Protocol encryption, sealed envelopes, and key rotation in AgentZero.
---

AgentZero provides layered privacy controls: from simple one-line modes to per-agent, per-tool, and per-channel boundaries. All privacy features are behind the `privacy` Cargo feature flag.

## Quick Start

Add one line to your `agentzero.toml`:

```toml
[privacy]
mode = "local_only"  # All traffic stays on your machine
```

That's it. AgentZero will:
- Only allow local providers (Ollama, llama.cpp, LM Studio, vLLM, SGLang)
- Block all outbound network tools (web_search, http_request, web_fetch)
- Reject non-localhost provider URLs

## Privacy Modes

| Mode | Cloud Providers | Encryption | Sealed Envelopes | Key Rotation |
|------|----------------|------------|-------------------|--------------|
| `off` | Allowed | No | No | No |
| `local_only` | Blocked | No | No | No |
| `encrypted` | Allowed | Noise Protocol | No | Auto |
| `full` | Allowed | Noise Protocol | Yes | Auto |

### `off` (default)

No privacy restrictions. All providers and tools work normally.

### `local_only`

The strictest mode. Ensures no data leaves your machine:
- Only local providers are allowed (ollama, llamacpp, lmstudio, vllm, sglang, osaurus, whispercpp)
- Network tools are disabled (web_search, http_request, web_fetch, composio)
- WASM plugins have network access revoked
- Provider base URLs must be localhost

### `encrypted`

All communication with the gateway is encrypted using the Noise Protocol (XX handshake, X25519_ChaChaPoly_BLAKE2s). Cloud providers are allowed because traffic is protected in transit. Key rotation runs automatically.

### `full`

Everything in `encrypted` plus sealed envelope support for zero-knowledge routing. The gateway can relay encrypted messages without reading their content.

## Per-Component Privacy Boundaries

For fine-grained control, assign privacy boundaries to individual agents, tools, and channels.

### Agent Boundaries

```toml
[agents.research]
provider = "anthropic"
model = "claude-sonnet-4-6"
privacy_boundary = "encrypted_only"  # Must use encrypted transport

[agents.local-draft]
provider = "ollama"
model = "llama3"
privacy_boundary = "local_only"  # No network access at all
```

Boundary values: `inherit` (use parent/global), `local_only`, `encrypted_only`, `any`.

**Rule:** A child boundary can never be more permissive than its parent. If the global mode is `local_only`, an agent can't have `privacy_boundary = "any"` (config validation will reject it).

### Tool Boundaries

```toml
[security.tool_boundaries]
shell = "local_only"       # Shell can't make network calls
web_search = "any"         # Web search allowed everywhere
http_request = "encrypted_only"  # HTTP only through encrypted transport
```

### Provider Restrictions

```toml
[agents.research]
provider = "anthropic"
model = "claude-sonnet-4-6"
allowed_providers = ["anthropic", "openrouter"]  # Only these providers
blocked_providers = ["openai"]                    # Never use OpenAI
```

## Noise Protocol Encryption

When `mode = "encrypted"` or `mode = "full"`, the gateway uses the [Noise Protocol](http://noiseprotocol.org/) for end-to-end encryption.

**Handshake pattern:** XX (mutual authentication)
**Cipher suite:** X25519_ChaChaPoly_BLAKE2s

### How It Works

1. Client calls `GET /v1/privacy/info` to discover gateway capabilities
2. Client initiates XX handshake via `POST /v1/noise/handshake/step1`
3. Server responds with its ephemeral + static keys
4. Client completes handshake via `POST /v1/noise/handshake/step2`
5. Server returns a session ID
6. All subsequent requests use `X-Noise-Session: <id>` with encrypted bodies

### Configuration

```toml
[privacy.noise]
enabled = true
handshake_pattern = "XX"       # XX (mutual auth) or IK (known server key)
session_timeout_secs = 3600    # Sessions expire after 1 hour
max_sessions = 1000            # Maximum concurrent sessions
```

## Sealed Envelopes & Relay Mode

When `mode = "full"`, sealed envelopes enable zero-knowledge routing:

```toml
[privacy.sealed_envelopes]
enabled = true
max_envelope_bytes = 65536

[gateway]
relay_mode = true
```

Sealed envelopes are encrypted packets that the relay routes by `routing_id` without reading their content. Features:

- **Replay protection** — Each envelope carries a nonce; duplicates are rejected (HTTP 409)
- **TTL-based expiry** — Envelopes expire after a configurable TTL
- **Metadata stripping** — The relay strips identifying headers (X-Forwarded-For, X-Real-IP, Via)

### API

```bash
# Submit an envelope
curl -X POST /v1/relay/submit -d '{
  "routing_id": "<64-char hex>",
  "payload": "<base64-encoded sealed envelope>",
  "nonce": "<base64-encoded 24-byte nonce>",
  "ttl_secs": 300
}'

# Poll for envelopes
curl /v1/relay/poll/<routing_id>
```

## Key Rotation

Keys rotate automatically in `encrypted` and `full` modes:

```toml
[privacy.key_rotation]
enabled = true
interval_hours = 24        # Rotate every 24 hours
persist_path = "keys/"     # Store keys on disk for restart recovery
```

### Manual Rotation

```bash
agentzero privacy rotate-keys          # Rotate if interval elapsed
agentzero privacy rotate-keys --force  # Force immediate rotation
agentzero privacy key-info             # Show current key fingerprint + epoch
```

## Monitoring

Privacy metrics are exposed on the `/metrics` Prometheus endpoint:

| Metric | Type | Description |
|--------|------|-------------|
| `agentzero_noise_sessions_active` | Gauge | Active Noise sessions |
| `agentzero_noise_handshakes_total{result}` | Counter | Handshake attempts (success/failure) |
| `agentzero_relay_mailbox_envelopes` | Gauge | Envelopes in relay mailboxes |
| `agentzero_relay_submit_total` | Counter | Total envelope submissions |
| `agentzero_key_rotation_total{epoch}` | Counter | Key rotation events |
| `agentzero_privacy_encrypt_duration_seconds` | Histogram | Encrypt/decrypt latency |
