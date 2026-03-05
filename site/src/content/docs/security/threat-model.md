---
title: Threat Model
description: Security threat model covering the AgentZero runtime surface.
---

## Scope
This document covers the current AgentZero runtime surface in Sprint 0:
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
