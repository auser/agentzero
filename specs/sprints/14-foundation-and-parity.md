# AgentZero Sprint Plan

## Scope
Build a lightweight, maintainable clone with a traits-first architecture and strict phased delivery.

References:
- `docs/ROADMAP.md`
- `docs/adr/0001-scope.md`
- `docs/COMMANDS.md`

## Sprint Cadence
- Sprint length: 1 week.
- Planning: Monday.
- Mid-sprint checkpoint: Wednesday.
- Review/retro: Friday.
- Rule: every merged PR updates this file.

## Tracking Conventions
- Each task uses one of: `[ ]` not started, `[-]` in progress, `[x]` done.
- Mark the acceptance criteria item as done in the same PR that implements the feature.
- If scope changes, update this file before coding.

## Dependencies and Critical Path
- Critical path:
1. Sprint 1 tests/hygiene
2. Sprint 2 config
3. Sprint 3 hardening
- Blocking dependency rule: do not start a sprint if previous sprint acceptance criteria are incomplete.

## Risks and Mitigations
- Risk: shell tool security regressions.
- Mitigation: explicit allowlists, negative tests, bounded output, and no raw `sh -c` in hardened mode.
- Risk: provider API drift.
- Mitigation: mock tests and strict error mapping before adding features.
- Risk: architecture sprawl.
- Mitigation: ADR updates required for any new module family.

## Current State (Completed)
- [x] Workspace split into `bin/` and `crates/`.
- [x] CLI commands exist: `onboard`, `status`, `agent`.
- [x] Core traits exist: `Provider`, `MemoryStore`, `Tool`.
- [x] Shared tracing/bootstrap crate exists (`crates/agentzero-common`).
- [x] Infra implementations exist: OpenAI-compatible provider, SQLite memory, `read_file`, `shell`.
- [x] Optional Turso memory backend is available in `crates/agentzero-memory` and wired to CLI via feature flag.
- [x] WASM plugin runtime exists in `crates/agentzero-plugins` with security preflight validation.
- [x] Baseline gateway crate exists (`crates/agentzero-gateway`) with health and ping endpoints.
- [x] Encrypted persistence crate exists (`crates/agentzero-storage`) for secret-bearing on-disk state.
- [x] Foundational crates extracted for config/provider/memory (`agentzero-config`, `agentzero-providers`, `agentzero-memory`).
- [x] CI exists for `fmt`, `clippy`, and `test`.

## Target Crate Architecture (Major Module = Crate)
- `crates/agentzero-cli`: CLI library (commands, parser, dispatch, command context).
- `bin/agentzero`: thin binary entrypoint (`main.rs`) that calls `agentzero_cli::cli()`.
- `crates/agentzero-core`: domain models, traits, and pure agent orchestration.
- `crates/agentzero-config`: typed config load/validation/env overrides.
- `crates/agentzero-providers`: OpenAI-compatible provider implementation.
- `crates/agentzero-memory`: Unified memory implementation (SQLite default + Turso/libsql feature-gated backend).
- `crates/agentzero-tools-fs`: file-system tools (`read_file`, `write_file`).
- `crates/agentzero-tools-shell`: shell tool + command policy.
- `crates/agentzero-observability`: tracing, redaction, metrics.
- `crates/agentzero-runtime`: request loop, timeout/retry policy, execution pipeline.
- `crates/agentzero-plugins`: plugin lifecycle + WASM container/runtime policy and execution.
- `crates/agentzero-storage`: encrypted on-disk persistence for secret-bearing state.
- `crates/agentzero-testkit`: shared mocks, fixtures, integration harness.
- Exception policy:
- Tiny glue modules may remain in parent crate when splitting adds no maintainability value.
- Every exception must be documented in `specs/SPRINT.md` change log.

## Functional Coverage Checklist
- [x] CLI command surface complete and documented.
- [x] CLI parity with discovered reference command/subcommand/flag surface (from recursive `--help` enumeration of the reference binary). *(completed Sprint 16)*
- [x] Global machine-output parity: every command accepts `--json` and returns a structured JSON object when passed.
- [x] Config system complete (file + env + validation).
- [x] Agent loop hardened (timeouts, retries, bounded context).
- [x] Provider integration stable (error mapping and mocks).
- [x] Memory backend stable and tested.
- [x] Turso memory backend implemented and security-reviewed.
- [x] Tools secure by default with negative tests.
- [x] WASM plugin container runtime implemented with isolation controls.
- [x] Observability baseline (traces + metrics + redaction).
- [x] Performance baseline documented.
- [x] Release pipeline and quality gates complete.
- [x] Required modules delivered: `skills`, `tunnel`, `gateway`, `rag`, `auth`, `cron`, `doctor`.

## Requested Must-Have Track
User-priority modules that must be included in the proper clone:

- [x] `gateway` baseline crate + CLI command exists.
- [x] `auth` module with secure profile/token lifecycle.
- [x] `cron` module with create/list/pause/resume/update/remove.
- [x] `doctor` command/module for diagnostics and health checks.
- [x] `skills` module lifecycle (install/list/test/remove).
- [x] `tunnel` module for secure remote bridge use cases.
- [x] `rag` module with feature-gated ingestion/query pipeline.
- [x] Plugin development workflow (scaffold, validate, test, package, publish/install).
- [x] Step hooks support (`before_*` / `after_*`) for every execution step.

Delivery gate:
- These must be functionally implemented and documented no later than Sprint 13 completion.

## Command Surface Parity Checklist
- [x] `onboard`
- [x] `agent`
- [x] `gateway`
- [x] `daemon`
- [x] `service`
- [x] `doctor`
- [x] `status`
- [x] `update`
- [x] `estop`
- [x] `cron`
- [x] `models`
- [x] `providers`
- [x] `channel`
- [x] `integrations`
- [x] `skill`
- [x] `migrate`
- [x] `auth`
- [x] `hardware`
- [x] `peripheral`
- [x] `memory`
- [x] `config`
- [x] `completions`
- [x] `help`

## Plugin Development Track
Goal: make plugin creation and lifecycle first-class, not just runtime execution.

- [x] Define plugin manifest schema (id, version, permissions/capabilities, hooks, entrypoints).
- [x] Add plugin dev CLI commands:
- `agentzero plugin new`
- `agentzero plugin validate`
- `agentzero plugin test`
- `agentzero plugin package`
- `agentzero plugin install/remove/list`
- [x] Add local plugin dev loop (watch/build/test) with deterministic fixtures.
- [x] Add signature + integrity verification for packaged plugins.
- [x] Add plugin compatibility checks (runtime/API version gates).
- Acceptance:
- Developer can scaffold, test, package, and install a plugin end-to-end with docs only.

## Hook Lifecycle Track (Before/After Each Step)
Goal: support hook points around each execution phase for policy, observability, and extensibility.

- Required hook points:
- [x] `before_run` / `after_run`
- [x] `before_provider_call` / `after_provider_call`
- [x] `before_tool_call` / `after_tool_call`
- [x] `before_plugin_call` / `after_plugin_call`
- [x] `before_memory_write` / `after_memory_write`
- [x] `before_response_emit` / `after_response_emit`
- Hook behavior requirements:
- [x] Hooks can read context and emit events.
- [x] Hooks must be timeout-bounded and fail-closed configurable.
- [x] Hook errors are auditable and policy-controlled (block/warn/ignore by tier).
- [x] Hook chain ordering is deterministic.
- Acceptance:
- Each step emits both before and after hooks with tests for success, timeout, and failure policies.

## Upstream Module Parity Gaps
The following major upstream sections are not yet explicitly covered in our sprint plan and are required for a fuller clone trajectory:

- [x] `approval`
- [x] `auth` + `identity`
- [x] `channels` + `gateway` + `daemon` + `service`
- [x] `health` + `heartbeat` + `doctor`
- [x] `cron`
- [x] `coordination` + `cost` + `goals` + `hooks`
- [x] `integrations`
- [x] `providers` catalog management
- [x] `migration` + `update`
- [x] `plugins` + `skills` (including `skillforge` + `sop` capabilities)
- [x] `rag` + `multimodal`
- [x] `tunnel`
- [x] `hardware` + `peripherals`
- [x] Shared `util` extraction strategy

## Tool Parity Checklist
- [x] `agents_ipc.rs`
- [x] `apply_patch.rs`
- [x] `browser.rs`
- [x] `browser_open.rs`
- [x] `cli_discovery.rs` *(completed Sprint 16)*
- [x] `composio.rs` *(completed Sprint 16)*
- [x] `content_search.rs`
- [x] `cron_add.rs`
- [x] `cron_list.rs`
- [x] `cron_remove.rs`
- [x] `cron_run.rs` (via cron_pause/cron_resume)
- [x] `cron_runs.rs` (via cron_list)
- [x] `cron_update.rs`
- [x] `delegate.rs` (via subagent_spawn)
- [x] `delegate_coordination_status.rs` *(completed Sprint 16)*
- [x] `docx_read.rs`
- [x] `file_edit.rs`
- [x] `file_read.rs` (read_file)
- [x] `file_write.rs` (write_file)
- [x] `git_operations.rs`
- [x] `glob_search.rs`
- [x] `hardware_board_info.rs` *(completed Sprint 16)*
- [x] `hardware_memory_map.rs` *(completed Sprint 16)*
- [x] `hardware_memory_read.rs` *(completed Sprint 16)*
- [x] `http_request.rs`
- [x] `image_info.rs`
- [x] `mcp_client.rs` (mcp_tool bridge)
- [x] `mcp_protocol.rs` (mcp_tool bridge)
- [x] `mcp_tool.rs`
- [x] `mcp_transport.rs` (mcp_tool bridge)
- [x] `memory_forget.rs`
- [x] `memory_recall.rs`
- [x] `memory_store.rs`
- [x] `mod.rs`
- [x] `model_routing_config.rs` *(completed Sprint 15)*
- [x] `pdf_read.rs`
- [x] `process.rs`
- [x] `proxy_config.rs` *(completed Sprint 16)*
- [x] `pushover.rs` *(completed Sprint 16)*
- [ ] `schedule.rs` → *carried forward to Sprint 17*
- [x] `schema.rs` (tool trait + ToolResult)
- [x] `screenshot.rs`
- [x] `shell.rs`
- [x] `sop_advance.rs` *(completed Sprint 16)*
- [x] `sop_approve.rs` *(completed Sprint 16)*
- [x] `sop_execute.rs` *(completed Sprint 16)*
- [x] `sop_list.rs` *(completed Sprint 16)*
- [x] `sop_status.rs` *(completed Sprint 16)*
- [x] `subagent_list.rs`
- [x] `subagent_manage.rs`
- [x] `subagent_registry.rs` (in subagent_tools)
- [x] `subagent_spawn.rs`
- [x] `task_plan.rs`
- [x] `traits.rs` (Tool trait in agentzero-core)
- [x] `url_validation.rs`
- [x] `wasm_module.rs` *(completed Sprint 16)*
- [x] `wasm_tool.rs` *(completed Sprint 16)*
- [x] `web_fetch.rs`
- [x] `web_search_tool.rs`

## Channel Parity Checklist (from upstream `src/channels`)
- [x] `clawdtalk.rs` *(completed Sprint 16)*
- [x] `cli.rs` *(completed Sprint 16)*
- [x] `dingtalk.rs` *(completed Sprint 16)*
- [x] `discord.rs` *(completed Sprint 15/16)*
- [x] `email_channel.rs` *(completed Sprint 16)*
- [x] `imessage.rs` *(completed Sprint 16)*
- [x] `irc.rs` *(completed Sprint 16)*
- [x] `lark.rs` *(completed Sprint 16)*
- [x] `linq.rs` *(completed Sprint 16)*
- [x] `matrix.rs` *(completed Sprint 16)*
- [x] `mattermost.rs` *(completed Sprint 16)*
- [ ] `mqtt.rs` → *carried forward to Sprint 17*
- [x] `nextcloud_talk.rs` *(completed Sprint 16)*
- [x] `nostr.rs` *(completed Sprint 16)*
- [x] `qq.rs` *(completed Sprint 16 — qq_official + napcat)*
- [x] `signal.rs` *(completed Sprint 16)*
- [x] `slack.rs` *(completed Sprint 15/16)*
- [x] `telegram.rs` *(completed Sprint 15/16)*
- [ ] `transcription.rs` → *carried forward to Sprint 17*
- [x] `wati.rs` *(completed Sprint 16)*
- [x] `whatsapp.rs` *(completed Sprint 16)*
- [ ] `whatsapp_storage.rs` → *carried forward to Sprint 17*
- [ ] `whatsapp_web.rs` → *carried forward to Sprint 17*
- [x] `traits.rs` (channel trait surface) *(completed Sprint 15/16)*

## Template Usage Parity
Goal: support the same workspace template model and usage flow.

- [x] `AGENTS.md` template support. *(completed Sprint 16)*
- [x] `BOOT.md` template support. *(completed Sprint 16)*
- [x] `BOOTSTRAP.md` template support. *(completed Sprint 16)*
- [x] `HEARTBEAT.md` template support. *(completed Sprint 16)*
- [x] `IDENTITY` template support. *(completed Sprint 16)*
- [x] `SOUL.md` template support. *(completed Sprint 16)*
- [x] `TOOLS.md` template support. *(completed Sprint 16)*
- [x] `USER` template support. *(completed Sprint 16)*
- [x] Define template load order and session behavior in runtime. *(completed Sprint 16)*
- [x] Add CLI/config support to scaffold and validate template files. *(completed Sprint 16)*
- [x] Add docs for template responsibilities and safe usage boundaries. *(completed Sprint 16)*
- [x] Add tests for template discovery, precedence, and missing-file behavior. *(completed Sprint 16)*
- Acceptance:
- [x] Template loading is deterministic and documented. *(completed Sprint 16)*
- [x] Missing templates fail safely with actionable guidance. *(completed Sprint 16)*
- [x] Main-session vs shared-session template behavior is test-covered. *(completed Sprint 16)*

## Module Parity Mapping (Planned)
- Core runtime (`agent`, `memory`, `providers`, `tools`, `runtime`, `config`, `observability`, `security`): Sprints 0-8.
- Auth/identity/approval: Sprint 9.
- Channel runtime (`channels`, `gateway`, `daemon`, `service`): Sprint 10.
- Reliability/runtime ops (`health`, `heartbeat`, `doctor`, `cron`, `cost`, `coordination`): Sprint 11.
- Ecosystem and extensibility (`integrations`, `plugins`, `skills` with `skillforge`/`sop`, `tunnel`): Sprint 12.
- Data growth and portability (`migration`, `update`, `rag`, `multimodal`): Sprint 13.
- Device support (`hardware`, `peripherals`): Sprint 14 (optional profile for lightweight builds).

## Definition of Done (All Sprints)
- Code compiles and tests pass locally.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` pass.
- Feature has docs updates.
- Feature has at least one negative-path test.
- Non-goals are explicitly documented in PR/spec notes.

## Sprint 0: Security Foundation (Highest Priority)

### 0.1 Security architecture baseline
- [x] Create `docs/security/THREAT_MODEL.md` with trust boundaries and attacker model.
- [x] Define risk tiers for tools/providers/channels and required controls per tier.
- [x] Add `crates/agentzero-security` for shared policy and guardrails.
- Acceptance:
- [x] Threat model and controls are documented and referenced by code.

### 0.2 Secret handling and redaction
- [x] Add centralized secret redaction utility and apply it to all logs/errors.
- [x] Ensure API keys/tokens never appear in panic/error output.
- [x] Add tests proving redaction behavior.
- Acceptance:
- [x] No secret leakage in logs under normal and error paths.

### 0.3 Tool sandbox and policy hardening
- [x] Define explicit deny-by-default tool policy.
- [x] Add argument-level validation for shell tool.
- [x] Add file read/write policy with size/type/path restrictions.
- [x] Enforce tool restrictions from global config (`agentzero.toml` `[security.*]`).
- Acceptance:
- [x] Unsafe tool calls fail closed with auditable reasons.

### 0.5 Turso and WASM security controls
- [x] Define secure secret handling for `TURSO_AUTH_TOKEN` and connection metadata.
- [x] Add threat-model section for remote memory backend risks (token theft, exfiltration, MITM).
- [x] Define encryption requirements:
- [x] data in transit must use TLS for provider/Turso/MCP connections.
- [x] secrets at rest must be encrypted for persisted secret material and sensitive local artifacts.
- [x] key management policy for local + remote backends (rotation, storage, fallback behavior).
- [x] Add configurable audit logging to retrace execution steps (`[security.audit]`).
- [x] Define WASM plugin isolation policy (capabilities, IO limits, timeout, memory limits).
- [x] Add abuse-path tests for plugin preflight and backend selection.
- [x] Add MCP server tool bridge with explicit server allowlist and fail-closed defaults.
- Acceptance:
- [x] Turso + WASM additions are covered by explicit threat model, encryption controls, and fail-closed tests.

### 0.4 Supply chain and CI security gates
- [x] Add `cargo audit` and `cargo deny` checks in CI.
- [x] Add dependency update cadence and CVE response policy.
- [x] Add security review checklist to PR template.
- Acceptance:
- [x] CI blocks on high/critical vulnerabilities by default.

## Sprint 1: Core Testing and Refactor Hygiene

### 1.1 Add core unit tests for agent orchestration
- [x] Add tests for user/assistant memory append sequence.
- [x] Add test for `tool:<name> <input>` invocation path.
- [x] Add test for unknown tool name behavior.
- Acceptance:
- [x] Deterministic tests in `crates/agentzero-core`.

### 1.2 Add infra unit tests
- [x] SQLite memory roundtrip tests (`append`, `recent`, ordering).
- [x] `read_file` allowlist path-denial tests.
- [x] `shell` allowlist denial tests.
- Acceptance:
- [x] Denied operations return clear errors.

### 1.3 Improve module boundaries
- [x] Add `lib.rs` in CLI crate to host dispatch logic, keep `main.rs` thin.
- [x] Keep command parsing isolated from command execution.
- Acceptance:
- [x] `main.rs` is runtime/bootstrap only.

### 1.4 Extract foundational crates
- [x] Create `crates/agentzero-config`.
- [x] Create `crates/agentzero-providers`.
- [x] Create memory backend crate (now consolidated as `crates/agentzero-memory`).
- [x] Move existing implementations from `agentzero-infra` into extracted crates.
- [x] Keep `agentzero-infra` only as temporary compatibility layer or remove it.
- Acceptance:
- [x] Major module responsibilities are no longer co-located in one infra crate.

## Sprint 2: Config System (Phase 3 baseline brought forward)

### 2.1 Create typed config model
- [x] Add `agentzero-config` crate.
- [x] Define structs for provider, memory, security, agent settings.
- [x] Support load from `agentzero.toml`.
- Acceptance:
- [x] `agentzero onboard` produces valid config matching the typed schema.

### 2.2 Add env overrides
- [x] Support env overrides for API key, model, base URL, db path.
- [x] Document precedence order (env > file > defaults).
- Acceptance:
- [x] Tests verify precedence behavior.

### 2.3 Validate dangerous config values
- [x] Reject empty allowlists in non-dev mode.
- [x] Reject relative traversal escape attempts in allowed root.
- [x] Reject unsupported provider URL schemes.
- Acceptance:
- [x] Validation errors are actionable and explicit.

## Sprint 3: Agent Loop Hardening (Phase 2)

### 3.1 Loop control and cancellation
- [x] Add max-tool-iterations from config.
- [x] Add overall per-request timeout.
- [x] Return typed errors for timeout vs provider failures.
- [x] Introduce step execution model so every step can trigger `before_*` and `after_*` hooks.
- Acceptance:
- [x] No unbounded loops possible.

### 3.2 Structured tracing
- [x] Add request IDs and tool execution spans.
- [x] Add redaction helpers for secrets.
- [x] Log tool start/end and duration.
- Acceptance:
- [x] Logs are structured and do not leak secrets.

### 3.3 Conversation assembly
- [x] Include recent memory in provider prompt (bounded window).
- [x] Add context window cap to avoid oversized prompts.
- Acceptance:
- [x] Prompt construction is deterministic and tested.

## Sprint 4: Provider Robustness

### 4.1 Provider abstraction hardening
- [x] Add retry policy (bounded, jittered) for transient errors.
- [x] Add status-code-specific error mapping (401/429/5xx).
- [x] Add JSON parse fallback handling.
- Acceptance:
- [x] Clear error categories surfaced to CLI.

### 4.2 Mock-driven integration tests
- [x] Add mock server tests for success, timeout, malformed response.
- [x] Add tests for auth failure and rate limits.
- Acceptance:
- [x] Integration suite runs in CI without external API dependency.

### 4.3 Turso backend verification
- [x] Add Turso integration tests in memory crate with local libsql-compatible target when feasible.
- [x] Add fallback behavior tests (backend unavailable, auth failure, invalid URL).
- Acceptance:
- [x] Turso backend behavior is predictable and does not degrade SQLite default path.

## Sprint 5: Tooling and Safety

### 5.1 Harden `shell` tool execution
- [x] Replace `sh -c` string execution with argument-safe command execution.
- [x] Add configurable command + argument policy.
- [x] Capture stdout/stderr with bounded size.
- Acceptance:
- [x] No shell injection vector via raw input concatenation.

### 5.2 Harden `read_file` tool execution
- [x] Resolve symlink and canonical path checks consistently.
- [x] Add max file size read guard.
- [x] Add binary-file detection fallback.
- Acceptance:
- [x] Tool safely rejects unsafe or oversized reads.

### 5.3 Add optional `write_file` tool (strict mode)
- [x] Create new tool disabled by default.
- [x] Restrict writes to allowed workspace root.
- [x] Add overwrite flag and dry-run mode.
- Acceptance:
- [x] Write operations are explicit and auditable.

### 5.4 Split tool modules into dedicated crates
- [x] Create `crates/agentzero-tools` and move file tools + shell tool/policy.
- [x] Add per-crate tests and docs.
- Acceptance:
- [x] Tools are isolated by risk domain and independently testable.

## Sprint 6: CLI Productization

### 6.1 Improve command UX
- [x] Add `--config` global flag.
- [x] Add global `--data-dir`/`--config-dir` for storage path resolution (`flag > env > config > default ~/.agentzero`).
- [x] Centralize default data/config/sqlite path constants and helpers in `crates/agentzero-common`.
- [x] Add global `--verbose` flag to enable debug logging output.
- [x] Extract shared tracing bootstrap into `crates/agentzero-common`.
- [x] Add `--json` output mode for `status`.
- [x] Add better error formatting and exit codes.
- [x] Make `onboard` interactive with safe overwrite confirmation.
- [x] Add typed `onboard` option overrides with env-var fallback (`flag > env > default`).
- [x] Add shared interactive CLI UX helpers (headers, colors, checkmarks, section summaries).
- [x] Apply searchable/autocomplete-first prompts for all interactive commands.
- [x] Add interactive TUI dashboard command (ratatui-style) for live status, logs, and controls.
- Acceptance:
- Commands have consistent human and machine output options.

### 6.2 Expand command docs
- [x] Add examples for each command in `docs/COMMANDS.md`.
- [x] Add troubleshooting matrix (symptom -> fix).
- [x] Add architecture diagram in `docs/ARCHITECTURE.md`.
- Acceptance:
- New user can run first query in <10 minutes from docs only.

## Sprint 7: Observability and Benchmarks (Phase 4)

### 7.1 Runtime metrics
- [x] Add counters: requests, provider errors, tool errors.
- [x] Add histograms: provider latency, tool latency.
- [x] Add lightweight exporter (log or optional endpoint).
- Acceptance:
- Metrics visible locally and testable in integration tests.

### 7.2 Benchmarks and baselines
- [x] Add `criterion` benchmark crate for core loop.
- [x] Replace temporary offline benchmark harness in `crates/agentzero-bench` with `criterion` benches.
- [x] Add CLI startup and single-message benchmark scripts.
- [x] Check in results at `docs/benchmarks.md`.
- Acceptance:
- Baseline numbers reproducible with one documented command set.

### 7.3 Introduce runtime and testkit crates
- [x] Create `crates/agentzero-runtime` and move runtime orchestration concerns from CLI.
- [x] Create `crates/agentzero-testkit` for reusable mocks/fixtures.
- [x] Update integration tests to use testkit.
- Acceptance:
- Runtime and test support are reusable and decoupled from CLI.

## Sprint 8: Release Readiness

### 8.1 Packaging
- [x] Add release profile tuning and size checks.
- [x] Add versioning and changelog process.
- [x] Add GitHub release workflow (build + artifact upload).
- Acceptance:
- Tagged release produces downloadable binaries.

### 8.2 Project quality gates
- [x] Add coverage reporting.
- [x] Keep security audits (`cargo audit`, `cargo deny`) in CI and update policy docs.
- [x] Add dependency update policy.
- Acceptance:
- CI blocks merge on critical audit failures.

## Sprint 9: Auth, Identity, and Approval

### 9.1 Auth and profile management
- [x] Add `crates/agentzero-auth` for provider auth profiles and token lifecycle.
- [x] Implement profile selection, storage, and refresh behavior.
- [x] Add CLI command surface: `agentzero auth login/logout/list/status/use`.
- [x] Expand auth subcommand parity: `paste-redirect`, `paste-token`, `setup-token`, `refresh`.
- [x] Align `auth refresh` semantics to provider-based flow (`--provider`, optional `--profile`) with OpenAI Codex/Gemini-specific behavior.
- [x] Align `auth login` + `paste-redirect` with OAuth browser flow (authorize URL + localhost callback + fallback).
- [x] Add callback port fallback: when `1455` is unavailable, auto-select next available localhost port for OAuth redirect.
- [x] Add shared encrypted persistence crate and migrate auth + gateway token storage to it.
- Acceptance:
- Auth flows are tested with mocked providers and secure token storage.

### 9.2 Identity and approval controls
- [x] Add `crates/agentzero-identity` for actor identity model.
- [x] Add `crates/agentzero-approval` for high-risk action approvals.
- Acceptance:
- High-risk actions require explicit approval and are audit logged.

## Sprint 10: Channels, Gateway, Daemon, Service

### 10.1 Channel abstraction and gateway
- [x] Add `crates/agentzero-gateway` baseline HTTP service.
- [x] Add `crates/agentzero-channels` and evolve gateway routing/authn.
- [x] Implement minimal webhook + websocket gateway.
- [x] Add CLI command surface: `agentzero gateway --host --port`.
- [x] Add `agentzero gateway --new-pairing` to clear persisted paired tokens and rotate pairing setup.
- [x] Align pairing lifecycle so one-time pairing code is shown only when no paired tokens exist; `--new-pairing` re-enables enrollment.
- Acceptance:
- One reference channel works end-to-end through gateway.

### 10.2 Daemon and service lifecycle
- [x] Add `crates/agentzero-daemon` and `crates/agentzero-service`.
- [x] Add install/start/stop/status command flow.
- Acceptance:
- Long-running runtime starts reliably and exposes health endpoints.

## Sprint 11: Reliability and Operations

### 11.1 Health subsystem
- [x] Add `crates/agentzero-health`, `crates/agentzero-heartbeat`, `crates/agentzero-doctor`.
- [x] Implement stale-task/channel detection and operator diagnostics.
- [x] Add CLI command surface: `agentzero doctor`.
- [x] Align `agentzero doctor` to subcommand parity: `doctor models` and `doctor traces`.
- [x] Add `doctor` remediation hints per failed check (clear next-action guidance).
- [x] Fix dashboard startup failure when SQLite file path is not openable from default data dir.
- Error observed:
- `error: unable to open database file: /Users/auser/.agentzero/agentzero.db: Error code 14: Unable to open the database file`
- Acceptance:
- Health checks identify and classify runtime issues correctly.

### 11.2 Scheduling and operational controls
- [x] Add `crates/agentzero-cron`, `crates/agentzero-cost`, `crates/agentzero-coordination`, `crates/agentzero-goals`, `crates/agentzero-hooks`.
- [x] Add CLI command surface: `agentzero cron list/add/update/pause/resume/remove`.
- [x] Add CLI command surface for hook controls and diagnostics (list/enable/disable/test).
- [x] Align `agentzero models` command with `models refresh/list/set/status` flow and cached catalog behavior.
- Acceptance:
- Scheduled tasks and operational controls function with auditability.

## Sprint 12: Integrations and Extensibility

### 12.1 Integrations and plugin runtime
- [x] Add `crates/agentzero-integrations`, `crates/agentzero-plugins`, `crates/agentzero-tunnel`.
- [x] Evolve WASM runtime from preflight to executable container path in `crates/agentzero-plugins`.
- [x] Add sandbox controls for WASM runtime (time, memory, host-call allowlist).
- [x] Implement plugin packaging + install pipeline with integrity checks.
- Acceptance:
- Integration discovery and plugin execution work with sandbox controls.

### 12.2 Skills and SOP
- [x] Add `crates/agentzero-skills` with embedded `skillforge` and `sop` functionality.
- [x] Add CLI command surface: `agentzero skill list/install/test/remove`.
- [x] Add CLI command surface: `agentzero tunnel ...` (secure tunnel lifecycle).
- [x] Add plugin developer commands (`plugin new/validate/test/package/install/list/remove`).
- Acceptance:
- Skills lifecycle plus `skillforge`/SOP execution are versioned and test covered.

## Sprint 13: Migration, Update, and Knowledge Expansion

### 13.1 Migration and self-update
- [x] Consolidate migration + self-update into `crates/agentzero-update` (single crate owns both flows).
- [x] Add CLI command surface in `agentzero-cli`:
- `agentzero migrate ...` (implemented via `agentzero-update`)
- `agentzero update ...`
- Acceptance:
- Data import and binary update flows are recoverable, test-covered, and implemented via `agentzero-update`.

### 13.2 RAG and multimodal
- [x] Add `crates/agentzero-rag` and `crates/agentzero-multimodal`.
- [x] Add CLI command surface: `agentzero rag ingest/query` (feature-gated).
- Acceptance:
- Optional features are behind flags and do not bloat base runtime.

### 13.3 Memory crate consolidation
- [x] Merge `crates/agentzero-memory-sqlite` and `crates/agentzero-memory-turso` into a single `crates/agentzero-memory` crate.
- [x] Preserve backend selection via features/config while keeping public memory traits stable.
- [x] Remove legacy crate references from workspace, docs, and CLI wiring.
- Acceptance:
- A single memory crate provides both SQLite and Turso backends with migration coverage and tests.

## Sprint 14: Hardware and Peripherals (Optional Profile)

### 14.1 Device support track
- [x] Add `crates/agentzero-hardware` and `crates/agentzero-peripherals`.
- [x] Keep hardware support feature-gated and off by default.
- Acceptance:
- Hardware mode can be enabled without impacting default lightweight profile.

## Backlog (Post-v1, Out of Current Scope)
- [-] Additional channel providers beyond reference implementation.
- [ ] Advanced enterprise policy packs.
- [ ] Multi-node coordination and HA mode.

## Execution Order Summary
1. Sprint 0 (security foundation)
2. Sprint 1 (tests/hygiene + crate extraction)
3. Sprint 2 (config)
4. Sprint 3 (loop hardening)
5. Sprint 4 (provider robustness)
6. Sprint 5 (tool safety)
7. Sprint 6 (CLI/docs)
8. Sprint 7 (observability/bench)
9. Sprint 8 (release readiness)
10. Sprint 9 (auth/identity/approval)
11. Sprint 10 (channels/gateway/daemon/service)
12. Sprint 11 (reliability/operations)
13. Sprint 12 (integrations/extensibility)
14. Sprint 13 (migration/update/rag/multimodal)
15. Sprint 14 (hardware/peripherals optional)

## Sprint Change Log
- 2026-02-27: Initial sprint plan created.
- 2026-02-27: Added cadence, tracking conventions, dependencies, and risk sections.
- 2026-02-27: Added target crate architecture, functional coverage checklist, and crate-extraction tasks.
- 2026-02-27: Added upstream module parity gaps/mapping and expanded roadmap to Sprints 9-14.
- 2026-02-27: Added Sprint 0 security foundation and moved security to highest priority.
- 2026-02-27: Added Turso backend and WASM plugin-container milestones with security-first controls.
- 2026-02-27: Added baseline gateway crate and CLI command (`gateway`) with HTTP health/ping routes.
- 2026-02-27: Added must-have track for `skills`, `tunnel`, `gateway`, `rag`, `auth`, `cron`, and `doctor` with explicit command deliverables.
- 2026-02-27: Refactored CLI dispatch to trait-based `AgentZeroCommand` execution in `app.rs`.
- 2026-02-27: Consolidated `AgentZeroCommand` trait + `CommandContext` into `crates/agentzero-cli`, and moved executable entrypoint to thin `bin/agentzero/src/main.rs`.
- 2026-02-27: Updated binary entrypoint to `bin/agentzero/bins/cli.rs` and configured Cargo `[[bin]]` to use that path.
- 2026-02-27: Made `onboard` explicitly interactive by default via CLI surface; added `onboard --yes` for non-interactive runs.
- 2026-02-27: Added plugin development track and explicit before/after hook lifecycle requirements for each execution step.
- 2026-02-28: Reworked `models` into parity subcommands (`refresh/list/set/status`) with cached catalog behavior and config model update flow.
- 2026-02-27: Upgraded `onboard` UX with branded header, colored/checkmark section progress, and searchable interactive prompts via `inquire`.
- 2026-02-27: Added typed `onboard` flags + env var resolution using option-spec traits and precedence `flag > env > default`.
- 2026-02-28: Added initial workspace crates for Sprint 12.1 foundation: `agentzero-integrations`, `agentzero-plugins`, and `agentzero-tunnel` with baseline validation APIs and tests.
- 2026-02-28: Merged standalone WASM plugin crate into `agentzero-plugins` and kept executable runtime path using Wasmtime (`fn() -> i32` entrypoint invocation) with success/failure tests.
- 2026-02-28: Consolidated `agentzero-memory-sqlite` + `agentzero-memory-turso` into `agentzero-memory` and forwarded top-level features (`memory-sqlite`, `memory-turso`) through `bin/agentzero`.
- 2026-02-28: Added WASM sandbox enforcement in `agentzero-plugins` with execution timeout interruption, store memory limits, and host-call import allowlist validation.
- 2026-02-28: Added plugin packaging/install pipeline in `agentzero-plugins` with manifest-based SHA-256 integrity verification and tamper-detection tests.
- 2026-02-28: Consolidated Sprint 12.2 packaging direction to a single `agentzero-skills` crate that contains skills, `skillforge`, and SOP functionality.
- 2026-02-28: Added `agentzero-skills` crate (skills + skillforge + SOP modules) and wired `agentzero skill list/install/test/remove` command surface.
- 2026-02-28: Added `agentzero tunnel start/stop/status` command surface and encrypted tunnel lifecycle state in `agentzero-tunnel`.
- 2026-02-28: Added `agentzero tunnel start/stop/status` CLI lifecycle and encrypted tunnel session state in `agentzero-tunnel`.
- 2026-02-28: Added `agentzero plugin new/validate/test/package/install` developer command surface wired to manifest validation, WASM runtime preflight/execute, and package/install integrity pipeline.
- 2026-02-28: Extended plugin developer lifecycle with `agentzero plugin list/remove` plus installed-plugin index/removal APIs and JSON-capable listing output.
- 2026-02-28: Updated Sprint 13.1 architecture to consolidate migration + update functionality into a single crate: `agentzero-update`.
- 2026-02-28: Implemented `crates/agentzero-update` and wired `agentzero migrate import` + `agentzero update check/apply/rollback/status` command surfaces with success/negative test coverage.
- 2026-02-28: Implemented Sprint 13.2 with new `agentzero-rag` + `agentzero-multimodal` crates and feature-gated `agentzero rag ingest/query` CLI surface.
- 2026-02-28: Fixed dashboard SQLite startup failure by creating missing DB parent directories in `agentzero-memory` before opening SQLite (`Error code 14` path).
- 2026-02-28: Completed Sprint 14.1 with new `agentzero-hardware` + `agentzero-peripherals` crates and feature-gated `agentzero hardware/peripheral` CLI surfaces (disabled by default).
- 2026-02-28: Completed command parity for `agentzero config`, `agentzero memory`, and `agentzero completions` with CLI wiring, docs, and tests.
- 2026-02-28: Completed command parity for `agentzero estop`, `agentzero channel`, and `agentzero integrations` with CLI wiring, docs, and tests.
- 2026-02-28: Expanded plugin manifest schema (`capabilities`, `hooks`, runtime API range), enforced runtime compatibility checks, and added `before_plugin_call`/`after_plugin_call` hook lifecycle coverage with tests.
- 2026-02-28: Reconciled functional-coverage checklist status to implemented baseline for config, agent hardening, provider/memory/tools, WASM isolation, observability, performance docs, and CI/release quality gates.
- 2026-02-28: Added `agentzero plugin dev` deterministic local dev loop (`validate/preflight/execute`) with parser + command tests and command docs.
- 2026-02-28: Added tier-aware hook error policy (`block|warn|ignore`) wired through config/runtime/core, with auditable warn/ignore paths and compatibility fallback via `fail_closed`.
- 2026-02-28: Added `agentzero approval evaluate` and `agentzero identity upsert/get/add-role` command surfaces with encrypted local persistence and parser/command tests.
- 2026-02-28: Added `agentzero coordination`, `agentzero cost`, and `agentzero goals` command groups with encrypted local state, parser coverage, and command docs.
- 2026-02-28: Added tool-parity modules in `agentzero-tools`: `url_validation`, `http_request`, and `web_fetch` with fail-closed validation tests.
- 2026-02-28: Extracted shared URL validation utility into `agentzero-common::util` and migrated `url_validation`, `http_request`, and `web_fetch` tools to consume it.
- 2026-02-28: Added `agents_ipc` tool module in `agentzero-tools` with on-disk message queue operations (`send/recv/list/clear`) and success/negative tests.
- 2026-02-28: Reopened full CLI parity closure work; aligned `auth use` to `--provider/--profile` and added `cron add-at/add-every/once` command shapes.
- 2026-02-28: Aligned `doctor` CLI shape to snapshot parity with `doctor models` and `doctor traces` subcommands.
- 2026-02-28: Aligned `estop` CLI shape to top-level engage flags (`--level`, `--domain`, `--tool`) with `status` and `resume` subcommands (`--network`, `--domain`, `--tool`, `--otp`).
- 2026-02-28: Aligned `daemon` CLI shape to direct `daemon [--host --port]` invocation (no subcommands).
- 2026-02-28: Aligned `channel` CLI shape to snapshot parity subcommands (`add`, `bind-telegram`, `doctor`, `list`, `remove`, `start`).
- 2026-02-28: Finalized parity for `integrations info/list/search` and parser/docs alignment for `migrate import`.
- 2026-02-28: Completed `service` parity shape with `--service-init` and subcommands `install/restart/start/status/stop/uninstall`, including CLI + service-state tests.
- 2026-02-28: Completed `hardware` parity shape with `discover`, `info --chip` (default `STM32F401RETx`), and `introspect`; updated parser/docs/tests.
- 2026-02-28: Completed `status` parity by removing `--json` surface and aligning parser/runner/docs to formatted status output.
- 2026-03-01: Scope update: require global `--json` support for every command with consistent structured object output.
- 2026-03-01: Completed `peripheral` parity shape with `add`, `flash`, `flash-nucleo`, `list`, and `setup-uno-q`; updated parser coverage and command docs.
- 2026-03-01: Completed `memory` parity shape updates: `get` key-prefix behavior (key optional), `clear` key-prefix delete, and `--json` support on `memory clear`.
- 2026-03-01: Closed `providers` parity tracking after confirming aligned catalog output formatting, color/no-color behavior, and JSON mode with tests.
- 2026-03-01: Completed `onboard` parity by adding flag-shape coverage for `--interactive`, `--force`, `--channels-only`, `--api-key`, `--memory`, and `--no-totp`.
- 2026-03-01: Completed `auth` parity cleanup: `paste-token` now supports optional token input + `--auth-kind`, and docs/parser coverage were aligned with `auth use --provider --profile`.
- 2026-03-01: Completed `update` parity with top-level `--check` support and default `update` behavior resolving to status when no subcommand is provided.
- 2026-03-01: Implemented global `--json` mode in CLI dispatch; all commands now emit a structured JSON object envelope when `--json` is passed.
- 2026-03-01: Created `docs/COMMANDS.md` with full CLI command inventory (33 commands, 97+ subcommands) including testability tiers and test status.
- 2026-03-01: Added CLI integration test suite (`crates/agentzero-cli/tests/cli_integration.rs`) with 52 in-process tests covering T1/T2/T3 commands via `parse_cli_from`/`execute`.
- 2026-03-01: Added unit tests to `cron.rs` (3 tests) and `hooks.rs` (2 tests) command files that previously lacked test coverage.
- 2026-03-01: Fixed `agentzero-config` watcher test (`skips_invalid_config_change`) that was flaky due to env-var override bypassing validation; changed to use syntactically invalid TOML.
- 2026-03-01: Fixed pre-existing clippy warnings across workspace (`agentzero-local`, `agentzero-tools`, `agentzero-cli`) and applied `cargo fmt --all` for consistent formatting.
- 2026-03-01: Updated `channel list` to print a full channel catalog with availability markers, feature-gate hints, and follow-up command guidance.
- 2026-03-01: Added canonical channel catalog + channel-id normalization in `agentzero-channels` and implemented persistent `channel add/remove` state so listed channels are addable/configurable.
- 2026-03-01: Migrated `channel` persistence to `agentzero-storage` (`EncryptedJsonStore`) so enabled-channel state is encrypted at rest under the shared data directory.
- 2026-03-01: Started full persistence migration by moving `models` cached catalogs to `agentzero-storage` encrypted stores (`models/<provider>.json`).
- 2026-03-01: Started channel binding generalization by routing `bind-telegram` through the same generic channel binding persistence flow used by `channel add`.
- 2026-03-01: Started template/tool/channel parity tracks with initial scaffolding: `agentzero-config` template load-order model, `agentzero-tools::apply_patch` envelope validator, and reference channel handler registry seeds.
- 2026-03-01: Migrated `agentzero-update` state persistence to `agentzero-storage` and added encrypted-at-rest regression coverage; updated `doctor models --use-cache` to read encrypted model caches.
- 2026-03-01: Migrated `agentzero-peripherals` registry persistence to `agentzero-storage` with encrypted-at-rest regression coverage.
- 2026-03-01: Migrated `agentzero-rag` index persistence from plaintext JSONL append to `agentzero-storage` with legacy JSONL migration support and regression tests.
- 2026-03-01: Refactored channel handlers into `crates/agentzero-channels/src/channels/` with one file per channel implementation (`echo`, `telegram`, `discord`, `slack`, `webhook`) to support per-channel requirements.
- 2026-03-01: Reworked channels to a macro-driven declaration in `crates/agentzero-channels/src/channels/mod.rs`; all catalog channels now have handler structs and the macro now generates `CHANNEL_CATALOG` from the same source list.

## CLI Command + Flag Parity Snapshot (2026-02-28)

Generated from local reference binary.

### CLI Parity Closure Tracker
- [x] Align `auth use` flag shape to `--provider <PROVIDER> --profile <PROFILE>`.
- [x] Add `cron add-at`, `cron add-every`, and `cron once` command shapes.
- [x] Align `doctor` shape to `doctor models` and `doctor traces` subcommands with required flags.
- [x] Align `estop` shape to optional top-level engage flags + `status`/`resume` subcommands.
- [x] Align `daemon` shape to top-level options (no subcommands).
- [x] Align `channel` shape to `add`, `bind-telegram`, `doctor`, `list`, `remove`, `start`.
- [x] Add global `--json` parity across all commands (uniform structured object output).
- [x] Migrate persisted `channel` state to `agentzero-storage` encryption instead of direct file writes.
- [-] Migrate remaining persisted command state to `agentzero-storage` (eliminate direct JSON state files in CLI commands).
- [-] Generalize channel binding flow so Telegram is configured through the same generic channel path (remove special-case UX).
- [x] Align remaining command/subcommand/flag surfaces to snapshot parity and keep tests green.

### Command + Subcommand Parity Checklist
- [x] `agent` (no subcommands)
- [x] `auth`: `login`, `paste-redirect`, `paste-token`, `setup-token`, `refresh`, `logout`, `use`, `list`, `status`
- [x] `channel`: `add`, `bind-telegram`, `doctor`, `list`, `remove`, `start`
- [x] `completions` (no subcommands)
- [x] `config`: `schema`
- [x] `cron`: `add`, `add-at`, `add-every`, `list`, `once`, `pause`, `remove`, `resume`, `update`
- [x] `daemon` (no subcommands)
- [x] `doctor`: `models`, `traces`
- [x] `estop`: `status`, `resume` (+ top-level engage flags)
- [x] `gateway` (no subcommands)
- [x] `hardware`: `discover`, `info`, `introspect`
- [x] `integrations`: `info`, `list`, `search`
- [x] `memory`: `clear`, `get`, `list`, `stats`
- [x] `migrate`: `import`
- [x] `models`: `list`, `refresh`, `set`, `status`
- [x] `onboard` (no subcommands)
- [x] `peripheral`: `add`, `flash`, `flash-nucleo`, `list`, `setup-uno-q`
- [x] `providers` (no subcommands)
- [x] `service`: `install`, `restart`, `start`, `status`, `stop`, `uninstall`
- [x] `status` (no subcommands)
- [x] `update` (top-level `--check` + subcommands)

### Upstream CLI Parity Snapshot (`--help`)

Source binary: local reference binary.

#### Global Flags
- `    --config-dir <CONFIG_DIR>  `
- `-h, --help                     Print help`
- `-V, --version                  Print version`

#### `agent`
Flags:
- `    --config-dir <CONFIG_DIR>`
- `        `
Subcommands: *(none)*

#### `auth`
Flags:
- `    --config-dir <CONFIG_DIR>  `
- `-h, --help                     Print help`
Subcommands:
- `auth list`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `auth login`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --provider <PROVIDER>      Provider (`openai-codex` or `gemini`)`
  - `    --profile <PROFILE>        Profile name (default: default) [default: default]`
  - `    --device-code              Use OAuth device-code flow`
  - `-h, --help                     Print help`
- `auth logout`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --provider <PROVIDER>      Provider`
  - `    --profile <PROFILE>        Profile name (default: default) [default: default]`
  - `-h, --help                     Print help`
- `auth paste-redirect`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --provider <PROVIDER>      Provider (`openai-codex`)`
  - `    --profile <PROFILE>        Profile name (default: default) [default: default]`
  - `    --input <INPUT>            Full redirect URL or raw OAuth code`
  - `-h, --help                     Print help`
- `auth paste-token`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --provider <PROVIDER>      Provider (`anthropic`)`
  - `    --profile <PROFILE>        Profile name (default: default) [default: default]`
  - `    --token <TOKEN>            Token value (if omitted, read interactively)`
  - `    --auth-kind <AUTH_KIND>    Auth kind override (`authorization` or `api-key`)`
  - `-h, --help                     Print help`
- `auth refresh`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --provider <PROVIDER>      Provider (`openai-codex`)`
  - `    --profile <PROFILE>        Profile name or profile id`
  - `-h, --help                     Print help`
- `auth setup-token`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --provider <PROVIDER>      Provider (`anthropic`)`
  - `    --profile <PROFILE>        Profile name (default: default) [default: default]`
  - `-h, --help                     Print help`
- `auth status`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `auth use`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --provider <PROVIDER>      Provider`
  - `    --profile <PROFILE>        Profile name or full profile id`
  - `-h, --help                     Print help`

#### `channel`
Flags:
- `    --config-dir <CONFIG_DIR>`
- `        `
Subcommands:
- `channel add`
  - `    --config-dir <CONFIG_DIR>`
  - `        `
- `channel bind-telegram`
  - `    --config-dir <CONFIG_DIR>`
  - `        `
- `channel doctor`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `channel list`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `channel remove`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `channel start`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`

#### `completions`
Flags:
- `    --config-dir <CONFIG_DIR>`
- `        `
Subcommands: *(none)*

#### `config`
Flags:
- `    --config-dir <CONFIG_DIR>`
- `        `
Subcommands:
- `config schema`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`

#### `cron`
Flags:
- `    --config-dir <CONFIG_DIR>`
- `        `
Subcommands:
- `cron add`
  - `    --config-dir <CONFIG_DIR>`
  - `        `
- `cron add-at`
  - `    --config-dir <CONFIG_DIR>`
  - `        `
- `cron add-every`
  - `    --config-dir <CONFIG_DIR>`
  - `        `
- `cron list`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `cron once`
  - `    --config-dir <CONFIG_DIR>`
  - `        `
- `cron pause`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `cron remove`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `cron resume`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `cron update`
  - `    --config-dir <CONFIG_DIR>`
  - `        `

#### `daemon`
Flags:
- `    --config-dir <CONFIG_DIR>`
- `        `
Subcommands: *(none)*

#### `doctor`
Flags:
- `    --config-dir <CONFIG_DIR>  `
- `-h, --help                     Print help`
Subcommands:
- `doctor models`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --provider <PROVIDER>      Probe a specific provider only (default: all known providers)`
  - `    --use-cache                Prefer cached catalogs when available (skip forced live refresh)`
  - `-h, --help                     Print help`
- `doctor traces`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --id <ID>                  Show a specific trace event by id`
  - `    --event <EVENT>            Filter list output by event type`
  - `    --contains <CONTAINS>      Case-insensitive text match across message/payload`
  - `    --limit <LIMIT>            Maximum number of events to display [default: 20]`
  - `-h, --help                     Print help`

#### `estop`
Flags:
- `    --config-dir <CONFIG_DIR>`
- `        `
Subcommands:
- `estop resume`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --network                  Resume only network kill`
  - `    --domain <DOMAINS>         Resume one or more blocked domain patterns`
  - `    --tool <TOOLS>             Resume one or more frozen tools`
  - `    --otp <OTP>                OTP code. If omitted and OTP is required, a prompt is shown`
  - `-h, --help                     Print help`
- `estop status`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`

#### `gateway`
Flags:
- `    --config-dir <CONFIG_DIR>`
- `        `
Subcommands: *(none)*

#### `hardware`
Flags:
- `    --config-dir <CONFIG_DIR>`
- `        `
Subcommands:
- `hardware discover`
  - `    --config-dir <CONFIG_DIR>`
  - `        `
- `hardware info`
  - `    --chip <CHIP>`
  - `        Chip name (e.g. STM32F401RETx). Default: STM32F401RETx for Nucleo-F401RE`
  - `        `
  - `        [default: STM32F401RETx]`
- `hardware introspect`
  - `    --config-dir <CONFIG_DIR>`
  - `        `

#### `integrations`
Flags:
- `    --config-dir <CONFIG_DIR>  `
- `-h, --help                     Print help`
Subcommands:
- `integrations info`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `integrations list`
  - `-c, --category <CATEGORY>      Filter by category (e.g. "chat", "ai", "productivity")`
  - `    --config-dir <CONFIG_DIR>  `
  - `-s, --status <STATUS>          Filter by status: active, available, coming-soon`
  - `-h, --help                     Print help`
- `integrations search`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`

#### `memory`
Flags:
- `    --config-dir <CONFIG_DIR>`
- `        `
Subcommands:
- `memory clear`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --key <KEY>                Delete a single entry by key (supports prefix match)`
  - `    --category <CATEGORY>      `
  - `    --yes                      Skip confirmation prompt`
  - `-h, --help                     Print help`
- `memory get`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `memory list`
  - `    --category <CATEGORY>      `
  - `    --config-dir <CONFIG_DIR>  `
  - `    --session <SESSION>        `
  - `    --limit <LIMIT>            [default: 50]`
  - `    --offset <OFFSET>          [default: 0]`
  - `-h, --help                     Print help`
- `memory stats`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`

#### `migrate`
Flags:
- `    --config-dir <CONFIG_DIR>  `
- `-h, --help                     Print help`
Subcommands:
- `migrate`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --source <SOURCE>          Optional path to source workspace`
  - `    --dry-run                  Validate and preview migration without writing any data`
  - `-h, --help                     Print help`

#### `models`
Flags:
- `    --config-dir <CONFIG_DIR>  `
- `-h, --help                     Print help`
Subcommands:
- `models list`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --provider <PROVIDER>      Provider name (defaults to configured default provider)`
  - `-h, --help                     Print help`
- `models refresh`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --provider <PROVIDER>      Provider name (defaults to configured default provider)`
  - `    --all                      Refresh all providers that support live model discovery`
  - `    --force                    Force live refresh and ignore fresh cache`
  - `-h, --help                     Print help`
- `models set`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `models status`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`

#### `onboard`
Flags:
- `    --config-dir <CONFIG_DIR>  `
- `    --interactive              Run the full interactive wizard (default is quick setup)`
- `    --force                    Overwrite existing config without confirmation`
- `    --channels-only            Reconfigure channels only (fast repair flow)`
- `    --api-key <API_KEY>        API key (used in quick mode, ignored with --interactive)`
- `    --provider <PROVIDER>      Provider name (used in quick mode, default: openrouter)`
- `    --model <MODEL>            Model ID override (used in quick mode)`
- `    --memory <MEMORY>          Memory backend (sqlite, lucid, markdown, none) - used in quick mode, default: sqlite`
- `    --no-totp                  Disable OTP in quick setup (not recommended)`
- `-h, --help                     Print help`
Subcommands: *(none)*

#### `peripheral`
Flags:
- `    --config-dir <CONFIG_DIR>`
- `        `
Subcommands:
- `peripheral add`
  - `    --config-dir <CONFIG_DIR>`
  - `        `
- `peripheral flash`
  - `    --config-dir <CONFIG_DIR>`
  - `        `
- `peripheral flash-nucleo`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `peripheral list`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `peripheral setup-uno-q`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --host <HOST>              Uno Q IP (e.g. 192.168.0.48). If omitted, assumes running ON the Uno Q`
  - `-h, --help                     Print help`

#### `providers`
Flags:
- `    --config-dir <CONFIG_DIR>  `
- `-h, --help                     Print help`
Subcommands: *(none)*

#### `service`
Flags:
- `    --config-dir <CONFIG_DIR>      `
- `    --service-init <SERVICE_INIT>  Init system to use: auto (detect), systemd, or openrc [default: auto] [possible values: auto, systemd, openrc]`
- `-h, --help                         Print help`
Subcommands:
- `service install`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `service restart`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `service start`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `service status`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `service stop`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `service uninstall`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`

#### `skill`
Flags:
- `    --config-dir <CONFIG_DIR>  `
- `-h, --help                     Print help`
Subcommands:
- `skill audit`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `skill install`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `skill list`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `skill new`
  - `    --config-dir <CONFIG_DIR>  `
  - `-t, --template <TEMPLATE>      Template language: typescript, rust, go, python [default: typescript]`
  - `-h, --help                     Print help`
- `skill remove`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `skill templates`
  - `    --config-dir <CONFIG_DIR>  `
  - `-h, --help                     Print help`
- `skill test`
  - `    --config-dir <CONFIG_DIR>  `
  - `    --tool <TOOL>              Optional tool name inside the skill (defaults to first tool found)`
  - `-a, --args <ARGS>              JSON arguments to pass to the tool, e.g. '{"city":"Hanoi"}'`
  - `-h, --help                     Print help`

#### `status`
Flags:
- `    --config-dir <CONFIG_DIR>  `
- `-h, --help                     Print help`
Subcommands: *(none)*

#### `update`
Flags:
- `    --check`
- `        Check for updates without installing`
Subcommands: *(none)*
