---
title: Threat Model
description: Security threat model covering the AgentZero runtime surface.
---

## Scope

This document covers the AgentZero runtime surface:
- CLI execution (`bin/agentzero`, `crates/agentzero-cli`)
- provider network calls (`crates/agentzero-infra::provider`)
- memory backends (SQLite default, optional Turso)
- built-in tools (`read_file`, `shell`)
- gateway HTTP endpoint (`crates/agentzero-gateway`)

Controls are codified in `crates/agentzero-core/src/security/`:
- Risk tiers and required controls: `policy.rs`
- Secret redaction guardrails: `redaction.rs`

Baseline version: `2026-02-27`

## Assets

- API secrets: `OPENAI_API_KEY`, `TURSO_AUTH_TOKEN`
- User content and memory history
- Local workspace files
- Command execution surface (`shell` tool)
- Encryption keys / key-encryption-material for local and remote stores

## Secret Classification

| Class | Examples | Required Handling |
| --- | --- | --- |
| Secret | API keys/tokens (`OPENAI_API_KEY`, `TURSO_AUTH_TOKEN`), bearer tokens, encryption keys, signed credentials, secret-bearing headers | Redact in logs/errors, encrypt at rest, protect in transit (TLS), never commit to VCS |
| Sensitive (non-secret) | User prompts/responses, memory entries, workspace content paths, operational metadata that may reveal usage patterns | Minimize exposure, avoid unnecessary logging, apply access controls and retention limits |
| Non-sensitive | Tool names, feature flags, static defaults, public endpoint paths without credentials | Standard handling; no special crypto requirement unless combined with secret context |

## Trust Boundaries

1. User terminal input -> CLI command parser.
2. CLI process -> external providers over network.
3. CLI process -> local filesystem and subprocess execution.
4. Gateway listeners -> remote HTTP callers.
5. Process memory/log output -> local terminal/CI logs.

## Attacker Model

- Remote attacker sending malformed or malicious request payloads.
- Local attacker attempting command/path abuse through tool inputs.
- Insider or CI observer extracting secrets from logs or error output.
- Dependency/service failures returning unexpected error payloads.

## Risk Tiers and Required Controls

### P0 Critical

- Domains: `tool_execution`, `channel_ingress`
- Required controls:
  - deny-by-default behavior
  - explicit allowlists
  - timeout and bounded execution
  - redaction of secrets in all surfaced errors/logs

### P1 High

- Domains: `provider_network`, `remote_memory`
- Required controls:
  - authenticated transport
  - strict error handling with redaction
  - timeout/retry bounds

### P2 Moderate

- Domain examples: local non-secret metadata output
- Required controls:
  - structured validation
  - avoid sensitive data in diagnostics

## Current Sprint 0 Controls

- Centralized error/panic redaction in `agentzero_core::security::redaction`.
- Redacted user-facing CLI error chain output in `bin/agentzero/bins/cli.rs`.
- Policy baseline for domain-to-tier and control mapping in `agentzero_core::security::policy`.
- Global tool restrictions are configured centrally in `agentzero.toml` under `[security.*]` and enforced at tool construction.
- `read_file` fail-closed policy: blocks absolute/traversal paths, enforces allowed root, blocks binary files, and caps reads at 64 KiB.
- `shell` fail-closed policy: deny-by-default allowlist, argument count bound, and shell metacharacter rejection.
- Audit trail policy: `[security.audit]` controls append-only step logging for command execution, tool calls, provider calls, and memory writes.

## Encryption Requirements (Mandatory)

- In-flight: All provider, Turso, and MCP remote traffic must use TLS.
- At-rest: Persisted secret material and sensitive local artifacts must be encrypted at rest.
- Key management: Keys must not be hardcoded in source or config files committed to VCS.
- Fail-closed behavior is required when encryption preconditions are not met in production mode.

## WASM Isolation Policy

- network disabled by default
- filesystem write disabled by default
- bounded execution time and memory
- Plugin preflight must reject capability or limit violations before execution.

## Plugin System Threats (Sprint 20.6)

The plugin system (`agentzero-plugins` crate) introduces additional attack surface. The detailed threat model is maintained in `docs/security/THREAT_MODEL.md` and covers:

### Plugin Package Installation

- **Path traversal via tar extraction** — Mitigated: entries containing `..`, absolute paths, or symlinks are rejected.
- **Symlink entries** — Mitigated: all symlink and hard-link entry types rejected before content read.
- **SHA-256 integrity** — Mitigated: WASM binary digest verified against manifest on install.
- **Concurrent install/remove corruption** — Mitigated: exclusive file lock during operations.

### Plugin Discovery and Loading

- **Version comparison ordering** — Mitigated: `semver::Version` parsing with string fallback.
- **Tier override priority** — Accepted risk: development plugins can override global (by design).

### WASM Runtime Execution

- **Sandbox escape** — Mitigated: Wasmtime/wasmi with restricted WASI capabilities.
- **Resource exhaustion** — Mitigated: fuel metering and memory limits via `WasmIsolationPolicy`.
- **Host function abuse** — Mitigated: `allowed_host_calls` manifest allowlist enforced at runtime.

### Conversation Memory Encryption (Sprint 20.6 Phase 6)

- **SQLite plaintext conversation history** — Mitigated: database encrypted at rest using SQLCipher (AES-256-CBC). Encryption key is the shared `StorageKey` (32 bytes) stored at `~/.agentzero/.agentzero-data.key` (mode 0600) or via `AGENTZERO_DATA_KEY` env var.
- **Plaintext-to-encrypted migration** — Mitigated: auto-migration on first encrypted open via `sqlcipher_export`, atomic file swap.

### Plugin State Persistence

- **State file tampering** — Accepted risk: plugin metadata is non-sensitive (enabled/disabled, version). File permissions protect against external tampering.

### Hot-Reload Watcher (Development)

- **Rapid event flooding** — Mitigated: 200ms debounce window, only `.wasm` events processed.

## Gateway Network Threats (Sprint 23)

### WebSocket Abuse

- **Binary frame injection** — Mitigated: binary WebSocket frames are rejected with an error JSON frame; only text frames are processed.
- **Connection flooding** — Mitigated: rate limiting middleware (default 600 req/min) applies to upgrade requests. Idle connections are closed after 5 minutes.
- **Zombie connections** — Mitigated: server sends ping every 30 seconds; connections with no pong response within 60 seconds are terminated.

### Denial of Service

- **Request flooding** — Mitigated: sliding window rate limiter (default: 600 requests per 60-second window). Configurable via `rate_limit_max`; set to `0` to disable.
- **Large payload attacks** — Mitigated: request size limit middleware (default: 1 MB). Requests with `Content-Length` exceeding the limit are rejected with `413`.
- **Monitoring** — Prometheus metrics (`gateway_requests_total`, `gateway_errors_total`, `gateway_active_connections`) enable alerting on traffic anomalies.

## Privacy & Encryption Threats (Sprint 24)

### Key Management

- **Secret key leakage** — Mitigated: `IdentityKeyPair` does not implement `Serialize`; private keys cannot be accidentally serialized to JSON/TOML. Keys are only persisted via dedicated `KeyRingStore`.
- **Stale keys** — Mitigated: automatic key rotation (default 24h interval). Manual rotation via `agentzero privacy rotate-keys --force`. Key epoch tracked in metrics.
- **Key loss on restart** — Mitigated: keys persist to disk via `KeyRingStore`. Gateway reloads persisted keys on startup.

### Transport Security

- **Eavesdropping** — Mitigated: Noise Protocol XX handshake (X25519_ChaChaPoly_BLAKE2s) provides forward secrecy and mutual authentication. All provider traffic encrypted when `mode = "encrypted"` or `"full"`.
- **Replay attacks (transport)** — Mitigated: Noise Protocol provides built-in nonce sequencing; replayed messages are rejected by the cipher state.
- **Session hijacking** — Mitigated: session IDs are 32-byte SHA-256 hashes; sessions expire after configurable timeout (default 1h). Max session count is capped.

### Sealed Envelope Security

- **Envelope replay** — Mitigated: 24-byte nonces tracked in `DashMap`; duplicate nonces rejected with HTTP 409. Stale nonces garbage-collected alongside envelope TTL.
- **Traffic analysis** — Mitigated: relay strips identifying headers (X-Forwarded-For, X-Real-IP, Via); User-Agent replaced with generic value. Configurable timing jitter adds randomized delays to submit (10–100 ms) and poll (20–200 ms) responses to prevent timing side-channels.
- **Mailbox flooding** — Mitigated: per-routing-id mailbox size limit. Excess submissions rejected with HTTP 429.

### Privacy Boundary Enforcement

- **Provider leakage** — Mitigated: `local_only` mode validates provider kind at config load, rejects cloud providers at delegation validation, and blocks non-localhost base URLs.
- **Tool network leakage** — Mitigated: `local_only` disables network tools (web_search, http_request, web_fetch, composio) at policy level and enforces localhost-only domain allowlist.
- **Plugin network leakage** — Mitigated: WASM plugins have `allow_network = false` when network tools are disabled.
- **Boundary escalation** — Mitigated: config validation rejects agent boundaries more permissive than global mode. Runtime resolution enforces child-can't-exceed-parent rule.
- **Encrypted mode without transport** — Mitigated: config validation rejects `privacy.mode = "encrypted"` when `privacy.noise.enabled = false`.

### Memory Boundary Enforcement (Sprint 25)

- **Cross-boundary memory leakage** — Mitigated: `MemoryEntry` carries `privacy_boundary` and `source_channel` fields. `recent_for_boundary()` filters entries by boundary, ensuring agents with different boundaries see isolated conversation histories.
- **Backward-compatible migration** — Mitigated: SQLite schema adds columns with `DEFAULT ''` / `DEFAULT NULL`. Pre-existing entries get empty boundary (visible to all, matching pre-boundary behavior). `#[serde(default)]` on struct fields handles old JSON gracefully.
- **Source channel isolation** — Mitigated: memory queries can filter by `source_channel`, preventing cross-channel data leakage when channels have different trust levels.

### Channel Privacy Enforcement (Sprint 25)

- **Local-only content to cloud channels** — Mitigated: `dispatch_with_boundary()` blocks messages with `local_only` boundary from being sent to non-local channels (Telegram, Discord, Slack, etc.). Only `cli` and `transcription` are treated as local.
- **Leak guard boundary check** — Mitigated: `LeakGuardPolicy.check_boundary()` provides defense-in-depth by blocking `local_only` content from non-local channel dispatch, independent of the channel registry's own check.
- **IK handshake with wrong server key** — Mitigated: IK pattern validates server static key during handshake; mismatched keys cause handshake failure rather than silent degradation.

## New Attack Surfaces (Sprint 85–86)

The following attack surfaces were added to AgentZero in Sprints 39–85 and are not yet covered in the sections above. Mitigations marked **[PLANNED]** are scheduled for Sprint 86 Phase A (Capability-Based Security Phase 1).

### MCP Server Mode

When AgentZero runs as an MCP server (stdio or HTTP), connecting clients receive access to the full tool set enabled on the server — there is no per-session capability narrowing.

- **Full tool set exposure** — Risk: a connecting MCP client (e.g., Claude Desktop) receives all tools enabled in `agentzero.toml`, regardless of what the client actually needs. Residual risk: HIGH.
- **Mitigation [PLANNED]** — Sprint 86 Phase A will introduce `CapabilitySet` and per-MCP-session scoping via `[mcp_sessions.<name>]` config. Until then, limit enabled tools to the minimum needed when running in MCP server mode.

### A2A Protocol (Agent-to-Agent)

External agents can submit tasks to AgentZero via the A2A protocol. Currently there is no capability negotiation — tasks run with the full server policy.

- **Unconstrained task execution** — Risk: an external A2A agent can trigger any enabled tool by submitting an appropriate task. Residual risk: HIGH.
- **Mitigation [PLANNED]** — Sprint 86 Phase A will enforce `max_capabilities` from `[a2a.agents.<name>]` config via capability intersection at task submission time.

### Autopilot Self-Modification

The autopilot engine can create new agents and missions. Created entities currently inherit the full server-wide `ToolSecurityPolicy` rather than being bounded by the autopilot's own permissions.

- **Permission escalation via proposal** — Risk: an autopilot proposal that creates a new agent could grant that agent more capabilities than the autopilot itself has. Residual risk: MEDIUM.
- **Mitigation [PLANNED]** — The `Delegate { max_capabilities }` capability type (Sprint 86 Phase A) will enforce that created agents receive at most the intersection of the creator's capability set and the new agent's configured capabilities.

### Memory Poisoning

Agents writing to shared memory can influence other agents' reasoning. There is currently no per-agent memory isolation enforced at the policy layer.

- **Cross-agent memory influence** — Risk: a compromised or misbehaving agent writing adversarial content to memory (e.g., prompt injection payloads) can affect all other agents reading from the same memory store. Residual risk: MEDIUM.
- **Mitigation [PLANNED]** — The `Memory { scope: Option<String> }` capability (Sprint 86 Phase A) will allow per-agent memory scoping. Defense-in-depth: `MemoryEntry.privacy_boundary` (Sprint 25) already provides boundary-level isolation; capability scoping adds finer-grained agent-level control.

### Dynamic Tool Creation

AgentZero can create WASM tools at runtime via the `tool_create` tool (Sprint 73). Dynamically created tools currently execute with the server's full policy rather than being bounded by the creator agent's permissions.

- **Permission inheritance beyond creator scope** — Risk: an agent with limited permissions creates a dynamic tool; that tool executes with server-wide permissions on its next invocation. Residual risk: HIGH until Sprint 86 complete.
- **Current mitigation** — `enable_dynamic_tools = false` by default (fail-closed). Sprint 84B added a runtime kill-switch (`codegen_enabled` in config). These are coarse gates.
- **Mitigation [PLANNED]** — Sprint 86 Phase A will wire `CapabilitySet` into `ToolSecurityPolicy`. Dynamically created tools will be capability-checked via `allows_tool("tool_create")`, and `DynamicToolDef` will carry a `capability_set` field so evolved tools cannot exceed their creator's scope.
