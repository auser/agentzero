# Plan 44: Privacy-First Provider Calls — Request IDs + Mandatory PII Stripping

## Context

**This is a core project goal:** every remote LLM provider call must carry a single-use, PII-free request ID, and no PII may reach the provider in the prompt, system message, or headers.

AgentZero already has significant privacy infrastructure (PiiRedactionGuard, Noise transport, PrivacyBoundary system, GuardrailsLayer pipeline). What's missing is making it **mandatory** rather than opt-in, closing the header leakage surface, and extending PII detection coverage.

### Current state

- `PiiRedactionGuard` exists in `guardrails.rs` — detects email, US phone, SSN, API keys. But it's opt-in via `GuardrailsLayer` and only enabled when the user configures it.
- No request ID header on outbound calls. Only W3C `traceparent` (derived from tracing span IDs, which are process-internal).
- No User-Agent header set on provider calls.
- The `HttpTransport::send()` / `send_chat()` in both `anthropic.rs` and `openai.rs` are the single chokepoints — every outbound call flows through them.
- The pipeline architecture (`PipelineBuilder` → `LlmLayer`) allows composable middleware — we can intercept prompts before they reach the provider.

### Threat model

An LLM provider receives:
1. **The prompt text** — may contain user-authored PII (names, emails, SSNs, credit cards, addresses)
2. **HTTP headers** — may contain correlatable identifiers (API keys are expected; session IDs, User-Agent fingerprints, IP-derived trace IDs are not)
3. **Tool call results** — may contain data from files, databases, or APIs that include PII

We defend against (1) with prompt-level PII stripping, against (2) with opaque request IDs and minimal headers, and against (3) by running the same PII filter on tool results before they're appended to the conversation history.

---

## Phase A: Single-Use Request ID on Every Outbound Call (HIGH)

Every HTTP request to a remote LLM provider gets a unique, opaque, non-correlatable request ID. No PII, no session affinity, no timestamp that could be used for fingerprinting.

- [ ] **`generate_request_id()`** in `transport.rs` — Returns a UUID v4 (128-bit random, no timestamp component, no MAC address). Uses `uuid::Uuid::new_v4().to_string()`. UUID v4 is already in the workspace deps.
- [ ] **`apply_request_id()`** in `transport.rs` — Sets `X-Request-ID: <uuid>` on the request builder. Called alongside `apply_traceparent()` in both transport impls.
- [ ] **Wire into Anthropic transport** — `anthropic.rs` `ReqwestTransport::send()` calls `apply_request_id()` before `.send()`.
- [ ] **Wire into OpenAI transport** — `openai.rs` `ReqwestTransport::send_chat()` calls `apply_request_id()` before `.send()`.
- [ ] **Strip User-Agent** — Explicitly set `User-Agent: agentzero` (generic, no version, no OS info) on all outbound provider calls. Prevents reqwest's default User-Agent from leaking the Rust version and platform.
- [ ] **Log the request ID** — Add `request_id` field to the existing tracing spans (`anthropic_complete`, `openai_complete`) so the ID appears in structured logs for correlation, but never leaves the host.
- [ ] **Tests** — Mock transport captures headers; assert `X-Request-ID` is present, is a valid UUID v4, and differs between calls. Assert `User-Agent` is exactly `agentzero`.

## Phase B: Mandatory PII Stripping on Every Outbound Prompt (HIGH)

Make PII redaction non-optional on all remote provider calls. Local providers (Candle, llama.cpp, Ollama on localhost) are exempt — data never leaves the machine.

- [ ] **`PrivacyFirstLayer`** — New `LlmLayer` in `crates/agentzero-providers/src/privacy_layer.rs`. Wraps any `Provider` and runs `PiiRedactionGuard::check_input()` on every prompt before forwarding to the inner provider. If PII is detected, it is **always sanitized** (replaced with `[EMAIL_REDACTED]` etc.) — never blocked, never audit-only. The prompt that reaches the provider is always clean.
- [ ] **Apply to tool results** — When tool results are appended to the conversation via `ConversationMessage`, run the same PII filter before the text enters the prompt window. This catches PII in file contents, API responses, and database query results that tools surface.
- [ ] **Exempt local providers** — `PrivacyFirstLayer` checks `agentzero_core::common::local_providers::is_local_provider(kind)` and passes through without redaction for local kinds. Local data stays local.
- [ ] **Wire unconditionally in `build_runtime_execution()`** — `PrivacyFirstLayer` is always the outermost layer in the pipeline, regardless of user config. It cannot be disabled. The opt-in `GuardrailsLayer` sits inside it for additional user-configured guards.
- [ ] **Metrics** — Prometheus counter `agentzero_pii_redactions_total{pattern="email|phone|ssn|..."}` incremented on each redaction. Operators can monitor for PII in their prompt streams without seeing the actual PII.
- [ ] **Tests** — Provider receives a prompt with an email embedded; assert the provider never sees the email. Provider receives a prompt from a local provider; assert the email is NOT redacted (local exemption). Counter increments on redaction.

## Phase C: Extended PII Pattern Coverage (MEDIUM)

Extend `PiiRedactionGuard` to catch a broader set of PII patterns. Each pattern is a separate regex with a named redaction placeholder.

- [ ] **Credit card numbers** — Luhn-validated 13-19 digit sequences. Redaction: `[CC_REDACTED]`.
- [ ] **JWT tokens** — `eyJ` prefix + base64 segments. Redaction: `[JWT_REDACTED]`.
- [ ] **Private SSH keys** — `-----BEGIN (RSA|DSA|EC|OPENSSH) PRIVATE KEY-----` blocks. Redaction: `[SSH_KEY_REDACTED]`.
- [ ] **Database connection strings** — `postgres://`, `mysql://`, `mongodb://`, `redis://` with embedded credentials. Redaction: `[DB_CONN_REDACTED]`.
- [ ] **IP addresses** — IPv4 dotted-quad (private ranges exempt: 127.x, 10.x, 172.16-31.x, 192.168.x). Redaction: `[IP_REDACTED]`.
- [ ] **AWS access keys** — Already covered (`AKIA` prefix). Add AWS secret keys (`[A-Za-z0-9/+=]{40}` after an AKIA line). Redaction: `[AWS_SECRET_REDACTED]`.
- [ ] **Physical addresses** — Heuristic: 3-5 digit number followed by a street name pattern + city/state/zip. High false-positive rate, so this one uses `Enforcement::Audit` by default rather than `Sanitize`.
- [ ] **Tests** — Each new pattern has a positive match test and a false-positive exclusion test (e.g., "192.168.1.1" is NOT redacted as a public IP).

## Phase D: Validation + Documentation (LOW)

- [ ] **`cargo clippy --workspace --all-targets`** — 0 warnings
- [ ] **All existing tests pass** — no regressions
- [ ] **Config reference** — Document `[privacy]` section: `pii_redaction = "always"` (default, cannot be disabled for remote providers), `pii_additional_patterns = [...]` for custom patterns
- [ ] **Site docs** — New `/security/pii-protection/` page explaining the threat model, what's redacted, how request IDs work, and the local-provider exemption
- [ ] **SPRINT.md** — Sprint 85 section

## Acceptance Criteria

- [ ] Every outbound HTTP request to a remote LLM provider carries `X-Request-ID: <uuid-v4>` and `User-Agent: agentzero`
- [ ] No two requests share the same `X-Request-ID`
- [ ] The `X-Request-ID` contains no PII, no timestamp, no session affinity
- [ ] Every prompt reaching a remote provider has had PII stripped (emails, phones, SSNs, API keys, credit cards, JWTs, SSH keys, DB connection strings)
- [ ] Local providers (Candle, llama.cpp, Ollama) are exempt from PII stripping
- [ ] PII redaction cannot be disabled for remote providers — it is unconditional
- [ ] Prometheus counter tracks redaction events by pattern type
- [ ] 0 clippy warnings, all tests pass

## Out of Scope

- Response-side PII detection (the provider's output). This is a different threat model — the provider is generating text, not receiving user PII.
- IP address anonymization at the network level (Tor, VPN). Out of scope for the application layer.
- Differential privacy or k-anonymity on prompt embeddings. Research-grade, not production-ready.
