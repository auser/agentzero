# Threat Model

## Scope
This document covers the current AgentZero runtime surface in Sprint 0:
- CLI execution (`bin/agentzero`, `crates/agentzero-cli`)
- provider network calls (`crates/agentzero-infra::provider`)
- memory backends (SQLite default, optional Turso)
- built-in tools (`read_file`, `shell`)
- gateway HTTP endpoint (`crates/agentzero-gateway`)

Controls are codified in `crates/agentzero-security`:
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

Notes:
- If uncertain, classify as `Secret` by default.
- Derived artifacts containing embedded credentials are `Secret` even if the source type is normally non-secret.

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

## Remote Memory Risks (Turso)
- Token theft:
- Exposure of `TURSO_AUTH_TOKEN` through logs/errors/debug output can grant remote DB access.
- Control:
- Token must be redacted in diagnostics and never included in connection error context.
- Exfiltration:
- Compromised endpoint or misrouted traffic can leak memory contents.
- Control:
- Restrict endpoints to `libsql://` or `https://`; reject insecure or downgraded transport flags.
- MITM:
- Interception/alteration of remote memory traffic.
- Control:
- TLS-only transport policy; fail closed on insecure URL schemes or explicit TLS disable attempts.

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
- Centralized error/panic redaction in `agentzero-security::redaction`.
- Redacted user-facing CLI error chain output in `bin/agentzero/bins/cli.rs`.
- Redacted panic hook to prevent secret leakage in panic payload output.
- Policy baseline for domain-to-tier and control mapping in `agentzero-security::policy`.
- Global tool restrictions are configured centrally in `agentzero.toml` under `[security.*]` and enforced at tool construction.
- `read_file` fail-closed policy: blocks absolute/traversal paths, enforces allowed root, blocks binary files, and caps reads at 64 KiB.
- `shell` fail-closed policy: deny-by-default allowlist, argument count bound, and shell metacharacter rejection.
- `mcp` fail-closed policy: requires explicit server allowlist (`AGENTZERO_MCP_SERVERS`) and rejects unknown servers.
- `plugin_exec` fail-closed policy: optional and off by default; requires explicit JSON config (`AGENTZERO_PLUGIN_TOOL`).
- Audit trail policy: `[security.audit]` controls append-only step logging for command execution, tool calls, provider calls, and memory writes.
- Dependency and supply-chain checks are enforced in CI/CD/release workflows via `scripts/run-security-audits.sh` (`cargo audit` + `cargo deny check advisories`).

## Encryption Requirements (Mandatory)
- In-flight:
- All provider, Turso, and MCP remote traffic must use TLS.
- Reject non-TLS endpoints unless explicitly approved for local-only development.
- At-rest:
- Persisted secret material and sensitive local artifacts must be encrypted at rest.
- Key management:
- Keys must not be hardcoded in source or config files committed to VCS.
- Local key material must come from OS/user secret stores or process environment at runtime.
- Remote backend tokens must be scoped to least privilege and rotated on credential events.
- Rotation procedure and recovery behavior must be documented and testable.
- Fail-closed behavior is required when encryption preconditions are not met in production mode.

## WASM Isolation Policy
- Capability defaults:
- network disabled by default
- filesystem write disabled by default
- bounded execution time and memory
- IO/resource limits:
- module size limit must be enforced at preflight
- max execution time and max memory must be bounded by policy
- Fail-closed:
- Plugin preflight must reject capability or limit violations before execution.

## Non-Goals for This Sprint Slice
- Full sandbox policy enforcement for tools (Sprint 0.3).
- WASM runtime isolation and Turso abuse-path enforcement (Sprint 0.5).
