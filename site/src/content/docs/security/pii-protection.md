---
title: PII Protection
description: How AgentZero prevents personally identifiable information from reaching remote LLM providers — a core project safety guarantee.
---

**This is a core project goal, not a feature flag.** Every prompt sent to a remote LLM provider has PII stripped before it leaves the host. Every outbound HTTP request carries an opaque, single-use request ID with no PII, no timestamp, and no correlation data. This behavior cannot be disabled.

## Threat Model

When you call a remote LLM provider (Anthropic, OpenAI, Azure, OpenRouter, etc.), three categories of data leave your machine:

| Category | Risk | AgentZero Defense |
|---|---|---|
| **Prompt text** | May contain user-authored PII (names, emails, SSNs, credit cards) or PII surfaced by tools (file contents, API responses, database query results) | `PrivacyFirstLayer` — mandatory PII redaction on every prompt |
| **HTTP headers** | May contain correlatable identifiers (session IDs, User-Agent fingerprints, IP-derived trace IDs) | Opaque UUID v4 `X-Request-ID`, generic `User-Agent: agentzero` |
| **Tool call results** | May contain data from files, databases, or APIs that include PII | Same `PrivacyFirstLayer` — tool results are sanitized inside `ConversationMessage::ToolResult` before they enter the prompt window |

Local providers (Candle, llama.cpp, Ollama on localhost) are **exempt** — data never leaves the machine, so stripping it would reduce model quality for no security benefit.

## What Gets Redacted

The `PiiRedactionGuard` detects and replaces 9 PII pattern categories:

| Pattern | Example | Replacement |
|---|---|---|
| Email addresses | `alice@example.com` | `[EMAIL_REDACTED]` |
| US phone numbers | `555-123-4567` | `[PHONE_REDACTED]` |
| Social Security Numbers | `123-45-6789` | `[SSN_REDACTED]` |
| API keys | `sk-abcdef...`, `AKIA...`, `ghp_...` | `[API_KEY_REDACTED]` |
| Credit card numbers | `4111 1111 1111 1111` | `[CC_REDACTED]` |
| JWT tokens | `eyJhbGci...` | `[JWT_REDACTED]` |
| SSH private keys | `-----BEGIN RSA PRIVATE KEY-----` | `[SSH_KEY_REDACTED]` |
| Database connection strings | `postgres://user:pass@host/db` | `[DB_CONN_REDACTED]` |
| IPv4 addresses | `203.0.113.42` | `[IP_REDACTED]` |

Patterns are evaluated most-specific-first so that structured patterns (database URIs containing `@`) are replaced before less specific patterns (email addresses matching `@`) can partially match.

IPv4 addresses include private ranges (10.x, 127.x, 192.168.x) — the Rust regex crate does not support lookahead, so we err on the side of over-redaction. False positives are safer than false negatives.

## How It Works

### Architecture

```
User Prompt
  → PrivacyFirstLayer (outermost pipeline layer, always on)
    → PiiRedactionGuard.check_input() on every text field
      → System message content
      → User message content
      → Assistant message content
      → Tool result content
    → Sanitized prompt forwarded to inner layers
      → GuardrailsLayer (optional user-configured guards)
      → MetricsLayer (timing + token counting)
      → CostCapLayer (per-run budget enforcement)
      → Base Provider (makes the actual HTTP call)
        → apply_privacy_headers()
          → X-Request-ID: <uuid-v4>
          → User-Agent: agentzero
          → traceparent (process-internal span ID, no PII)
        → HTTP POST to provider API
```

### Request IDs

Every outbound HTTP request to a remote provider gets a fresh UUID v4 `X-Request-ID` header. UUID v4 is 128 bits of cryptographic randomness — no timestamp component, no MAC address, no session affinity. Each request gets a unique ID that cannot be correlated with any other request, any user, or any session.

The `User-Agent` header is set to the generic string `agentzero` — no version number, no OS fingerprint, no platform information. This prevents provider-side fingerprinting.

These headers are applied at the transport layer (`apply_privacy_headers()` in `transport.rs`), which is the last code that touches the `reqwest::RequestBuilder` before `.send()`. All 5 HTTP send paths (Anthropic sync + 2 streaming, OpenAI sync + 2 streaming) go through this single chokepoint.

### Metrics

Each PII redaction event increments the Prometheus counter `agentzero_pii_redactions_total`. This lets operators monitor for PII in their prompt streams — if the counter is climbing, users are submitting prompts that contain PII, which may indicate a training gap or a need for upstream data masking.

The counter does not record *which* PII was found — only that a redaction occurred. The actual PII never appears in metrics, logs, or any observable surface.

## What Cannot Be Disabled

| Behavior | Why It's Mandatory |
|---|---|
| PII redaction on remote provider prompts | Core project safety guarantee. An opt-out would create a class of deployments where PII can leak. |
| UUID v4 request IDs (no PII, no timestamp) | Correlatable identifiers would allow providers to link requests across sessions. |
| Generic User-Agent | Platform fingerprinting is a passive tracking vector. |

## What Can Be Configured

| Setting | Default | Description |
|---|---|---|
| `[guardrails]` in `agentzero.toml` | Audit mode | Additional user-configured guards (prompt injection detection, Unicode injection, custom patterns) run *inside* the mandatory PrivacyFirstLayer. These are opt-in and configurable. |
| `[privacy]` mode | — | The Noise protocol encrypted transport (`privacy = "encrypted"` or `"full"`) adds transport-level encryption on top of PII stripping. This is complementary — Noise encrypts the *entire* request, while PII stripping removes PII from the *content*. |
| Local provider exemption | Automatic | Local providers (Candle, llama.cpp, Ollama, LMStudio, etc.) are automatically exempt from PII stripping because data never leaves the machine. The exemption is checked via `is_local_provider()` — it cannot be manually overridden to force-strip local prompts. |

## Adding Custom PII Patterns

To add a new detection pattern, extend `PiiRedactionGuard::default()` in `crates/agentzero-providers/src/guardrails.rs`. Each pattern is a named regex with a redaction placeholder:

```rust
PiiPattern {
    name: "my_custom_pattern",
    regex: regex::Regex::new(r"my-regex-here")
        .expect("regex should compile"),
    redaction: "[CUSTOM_REDACTED]",
},
```

Place more specific patterns before less specific ones in the `patterns` vec. The `PrivacyFirstLayer` will automatically pick up the new pattern — no additional wiring required.

## Implementation Files

| File | Role |
|---|---|
| `crates/agentzero-providers/src/privacy_layer.rs` | `PrivacyFirstLayer` — mandatory pipeline wrapper |
| `crates/agentzero-providers/src/guardrails.rs` | `PiiRedactionGuard` — pattern definitions and redaction logic |
| `crates/agentzero-providers/src/transport.rs` | `apply_privacy_headers()` — request ID and User-Agent injection |
| `crates/agentzero-infra/src/runtime.rs` | Pipeline wiring — `PrivacyFirstLayer` added as outermost layer |

## See Also

- [Security Overview](/security/overview/) — Full 12-layer defense-in-depth model
- [Security Boundaries](/security/boundaries/) — Component-level privacy boundary system
- [Threat Model](/security/threat-model/) — Comprehensive threat analysis
- [Provider Setup](/guides/providers/) — GPU acceleration and device detection
