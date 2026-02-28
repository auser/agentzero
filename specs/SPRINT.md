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
- [x] Optional Turso memory backend crate exists (`crates/agentzero-memory-turso`) and is wired to CLI via feature flag.
- [x] WASM plugin container crate exists (`crates/agentzero-plugins-wasm`) with security preflight validation.
- [x] Baseline gateway crate exists (`crates/agentzero-gateway`) with health and ping endpoints.
- [x] Encrypted persistence crate exists (`crates/agentzero-storage`) for secret-bearing on-disk state.
- [x] Foundational crates extracted for config/provider/sqlite memory (`agentzero-config`, `agentzero-providers`, `agentzero-memory-sqlite`).
- [x] CI exists for `fmt`, `clippy`, and `test`.

## Target Crate Architecture (Major Module = Crate)
- `crates/agentzero-cli`: CLI library (commands, parser, dispatch, command context).
- `bin/agentzero`: thin binary entrypoint (`main.rs`) that calls `agentzero_cli::cli()`.
- `crates/agentzero-core`: domain models, traits, and pure agent orchestration.
- `crates/agentzero-config`: typed config load/validation/env overrides.
- `crates/agentzero-providers`: OpenAI-compatible provider implementation.
- `crates/agentzero-memory-sqlite`: SQLite memory implementation.
- `crates/agentzero-memory-turso`: Turso/libsql memory implementation (feature-gated).
- `crates/agentzero-tools-fs`: file-system tools (`read_file`, `write_file`).
- `crates/agentzero-tools-shell`: shell tool + command policy.
- `crates/agentzero-observability`: tracing, redaction, metrics.
- `crates/agentzero-runtime`: request loop, timeout/retry policy, execution pipeline.
- `crates/agentzero-plugins-wasm`: WASM plugin container/runtime policy and execution.
- `crates/agentzero-storage`: encrypted on-disk persistence for secret-bearing state.
- `crates/agentzero-testkit`: shared mocks, fixtures, integration harness.
- Exception policy:
- Tiny glue modules may remain in parent crate when splitting adds no maintainability value.
- Every exception must be documented in `specs/SPRINT.md` change log.

## Functional Coverage Checklist
- [ ] CLI command surface complete and documented.
- [ ] Config system complete (file + env + validation).
- [ ] Agent loop hardened (timeouts, retries, bounded context).
- [ ] Provider integration stable (error mapping and mocks).
- [ ] Memory backend stable and tested.
- [ ] Turso memory backend implemented and security-reviewed.
- [ ] Tools secure by default with negative tests.
- [ ] WASM plugin container runtime implemented with isolation controls.
- [ ] Observability baseline (traces + metrics + redaction).
- [ ] Performance baseline documented.
- [ ] Release pipeline and quality gates complete.
- [ ] Required modules delivered: `skills`, `tunnel`, `gateway`, `rag`, `auth`, `cron`, `doctor`.

## Requested Must-Have Track
User-priority modules that must be included in the proper clone:

- [x] `gateway` baseline crate + CLI command exists.
- [ ] `auth` module with secure profile/token lifecycle.
- [ ] `cron` module with create/list/pause/resume/update/remove.
- [ ] `doctor` command/module for diagnostics and health checks.
- [ ] `skills` module lifecycle (install/list/test/remove).
- [ ] `tunnel` module for secure remote bridge use cases.
- [ ] `rag` module with feature-gated ingestion/query pipeline.
- [ ] Plugin development workflow (scaffold, validate, test, package, publish/install).
- [ ] Step hooks support (`before_*` / `after_*`) for every execution step.

Delivery gate:
- These must be functionally implemented and documented no later than Sprint 13 completion.

## Command Surface Parity Checklist
- [x] `onboard`
- [x] `agent`
- [x] `gateway`
- [ ] `daemon`
- [ ] `service`
- [x] `doctor`
- [x] `status`
- [ ] `update`
- [ ] `estop`
- [ ] `cron`
- [ ] `models`
- [x] `providers`
- [ ] `channel`
- [ ] `integrations`
- [ ] `skill`
- [ ] `migrate`
- [x] `auth`
- [ ] `hardware`
- [ ] `peripheral`
- [ ] `memory`
- [ ] `config`
- [ ] `completions`
- [x] `help`

## Plugin Development Track
Goal: make plugin creation and lifecycle first-class, not just runtime execution.

- [ ] Define plugin manifest schema (id, version, permissions/capabilities, hooks, entrypoints).
- [ ] Add plugin dev CLI commands:
- `agentzero plugin new`
- `agentzero plugin validate`
- `agentzero plugin test`
- `agentzero plugin package`
- `agentzero plugin install/remove/list`
- [ ] Add local plugin dev loop (watch/build/test) with deterministic fixtures.
- [ ] Add signature + integrity verification for packaged plugins.
- [ ] Add plugin compatibility checks (runtime/API version gates).
- Acceptance:
- Developer can scaffold, test, package, and install a plugin end-to-end with docs only.

## Hook Lifecycle Track (Before/After Each Step)
Goal: support hook points around each execution phase for policy, observability, and extensibility.

- Required hook points:
- [ ] `before_run` / `after_run`
- [ ] `before_provider_call` / `after_provider_call`
- [ ] `before_tool_call` / `after_tool_call`
- [ ] `before_plugin_call` / `after_plugin_call`
- [ ] `before_memory_write` / `after_memory_write`
- [ ] `before_response_emit` / `after_response_emit`
- Hook behavior requirements:
- [ ] Hooks can read context and emit events.
- [ ] Hooks must be timeout-bounded and fail-closed configurable.
- [ ] Hook errors are auditable and policy-controlled (block/warn/ignore by tier).
- [ ] Hook chain ordering is deterministic.
- Acceptance:
- Each step emits both before and after hooks with tests for success, timeout, and failure policies.

## Upstream Module Parity Gaps (from `openclaw/src`)
The following major upstream sections are not yet explicitly covered in our sprint plan and are required for a fuller clone trajectory:

- [ ] `approval`
- [ ] `auth` + `identity`
- [ ] `channels` + `gateway` + `daemon` + `service`
- [ ] `health` + `heartbeat` + `doctor`
- [ ] `cron`
- [ ] `coordination` + `cost` + `goals` + `hooks`
- [ ] `integrations`
- [ ] `providers` catalog management
- [ ] `migration` + `update`
- [ ] `plugins` + `skills` + `skillforge` + `sop`
- [ ] `rag` + `multimodal`
- [ ] `tunnel`
- [ ] `hardware` + `peripherals`
- [ ] Shared `util` extraction strategy

## OpenClaw Tool Parity Checklist (from `openclaw/src/tools`)
- [ ] `agents_ipc.rs`
- [ ] `apply_patch.rs`
- [ ] `browser.rs`
- [ ] `browser_open.rs`
- [ ] `cli_discovery.rs`
- [ ] `composio.rs`
- [ ] `content_search.rs`
- [ ] `cron_add.rs`
- [ ] `cron_list.rs`
- [ ] `cron_remove.rs`
- [ ] `cron_run.rs`
- [ ] `cron_runs.rs`
- [ ] `cron_update.rs`
- [ ] `delegate.rs`
- [ ] `delegate_coordination_status.rs`
- [ ] `docx_read.rs`
- [ ] `file_edit.rs`
- [ ] `file_read.rs`
- [ ] `file_write.rs`
- [ ] `git_operations.rs`
- [ ] `glob_search.rs`
- [ ] `hardware_board_info.rs`
- [ ] `hardware_memory_map.rs`
- [ ] `hardware_memory_read.rs`
- [ ] `http_request.rs`
- [ ] `image_info.rs`
- [ ] `mcp_client.rs`
- [ ] `mcp_protocol.rs`
- [ ] `mcp_tool.rs`
- [ ] `mcp_transport.rs`
- [ ] `memory_forget.rs`
- [ ] `memory_recall.rs`
- [ ] `memory_store.rs`
- [ ] `mod.rs`
- [ ] `model_routing_config.rs`
- [ ] `pdf_read.rs`
- [ ] `process.rs`
- [ ] `proxy_config.rs`
- [ ] `pushover.rs`
- [ ] `schedule.rs`
- [ ] `schema.rs`
- [ ] `screenshot.rs`
- [ ] `shell.rs`
- [ ] `sop_advance.rs`
- [ ] `sop_approve.rs`
- [ ] `sop_execute.rs`
- [ ] `sop_list.rs`
- [ ] `sop_status.rs`
- [ ] `subagent_list.rs`
- [ ] `subagent_manage.rs`
- [ ] `subagent_registry.rs`
- [ ] `subagent_spawn.rs`
- [ ] `task_plan.rs`
- [ ] `traits.rs`
- [ ] `url_validation.rs`
- [ ] `wasm_module.rs`
- [ ] `wasm_tool.rs`
- [ ] `web_fetch.rs`
- [ ] `web_search_tool.rs`

## Channel Parity Checklist (from upstream `src/channels`)
- [ ] `clawdtalk.rs`
- [ ] `cli.rs`
- [ ] `dingtalk.rs`
- [ ] `discord.rs`
- [ ] `email_channel.rs`
- [ ] `imessage.rs`
- [ ] `irc.rs`
- [ ] `lark.rs`
- [ ] `linq.rs`
- [ ] `matrix.rs`
- [ ] `mattermost.rs`
- [ ] `mqtt.rs`
- [ ] `nextcloud_talk.rs`
- [ ] `nostr.rs`
- [ ] `qq.rs`
- [ ] `signal.rs`
- [ ] `slack.rs`
- [ ] `telegram.rs`
- [ ] `transcription.rs`
- [ ] `wati.rs`
- [ ] `whatsapp.rs`
- [ ] `whatsapp_storage.rs`
- [ ] `whatsapp_web.rs`
- [ ] `traits.rs` (channel trait surface)

## Template Usage Parity
Goal: support the same workspace template model and usage flow.

- [ ] `AGENTS.md` template support.
- [ ] `BOOT.md` template support.
- [ ] `BOOTSTRAP.md` template support.
- [ ] `HEARTBEAT.md` template support.
- [ ] `IDENTITY` template support.
- [ ] `SOUL.md` template support.
- [ ] `TOOLS.md` template support.
- [ ] `USER` template support.
- [ ] Define template load order and session behavior in runtime.
- [ ] Add CLI/config support to scaffold and validate template files.
- [ ] Add docs for template responsibilities and safe usage boundaries.
- [ ] Add tests for template discovery, precedence, and missing-file behavior.
- Acceptance:
- [ ] Template loading is deterministic and documented.
- [ ] Missing templates fail safely with actionable guidance.
- [ ] Main-session vs shared-session template behavior is test-covered.

## Module Parity Mapping (Planned)
- Core runtime (`agent`, `memory`, `providers`, `tools`, `runtime`, `config`, `observability`, `security`): Sprints 0-8.
- Auth/identity/approval: Sprint 9.
- Channel runtime (`channels`, `gateway`, `daemon`, `service`): Sprint 10.
- Reliability/runtime ops (`health`, `heartbeat`, `doctor`, `cron`, `cost`, `coordination`): Sprint 11.
- Ecosystem and extensibility (`integrations`, `plugins`, `skills`, `skillforge`, `sop`, `tunnel`): Sprint 12.
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
- [x] Create `crates/agentzero-memory-sqlite`.
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
- [x] Add `crates/agentzero-memory-turso` integration tests with local libsql-compatible target when feasible.
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
- [ ] Add interactive TUI dashboard command (ratatui-style) for live status, logs, and controls.
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
- [-] Add `criterion` benchmark crate for core loop.
- [x] Temporary offline benchmark harness added in `crates/agentzero-bench`; swap to `criterion` when registry access is available.
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
- [-] Add `crates/agentzero-daemon` and `crates/agentzero-service`.
- [ ] Add install/start/stop/status command flow.
- Acceptance:
- Long-running runtime starts reliably and exposes health endpoints.

## Sprint 11: Reliability and Operations

### 11.1 Health subsystem
- [ ] Add `crates/agentzero-health`, `crates/agentzero-heartbeat`, `crates/agentzero-doctor`.
- [ ] Implement stale-task/channel detection and operator diagnostics.
- [x] Add CLI command surface: `agentzero doctor`.
- [ ] Add `agentzero doctor --json` with stable machine-readable schema.
- [ ] Add `doctor` remediation hints per failed check (clear next-action guidance).
- Acceptance:
- Health checks identify and classify runtime issues correctly.

### 11.2 Scheduling and operational controls
- [ ] Add `crates/agentzero-cron`, `crates/agentzero-cost`, `crates/agentzero-coordination`, `crates/agentzero-goals`, `crates/agentzero-hooks`.
- [ ] Add CLI command surface: `agentzero cron list/add/update/pause/resume/remove`.
- [ ] Add CLI command surface for hook controls and diagnostics (list/enable/disable/test).
- Acceptance:
- Scheduled tasks and operational controls function with auditability.

## Sprint 12: Integrations and Extensibility

### 12.1 Integrations and plugin runtime
- [ ] Add `crates/agentzero-integrations`, `crates/agentzero-plugins`, `crates/agentzero-tunnel`.
- [ ] Evolve `crates/agentzero-plugins-wasm` from preflight to executable runtime container.
- [ ] Add sandbox controls for WASM runtime (time, memory, host-call allowlist).
- [ ] Implement plugin packaging + install pipeline with integrity checks.
- Acceptance:
- Integration discovery and plugin execution work with sandbox controls.

### 12.2 Skills and SOP
- [ ] Add `crates/agentzero-skills`, `crates/agentzero-skillforge`, `crates/agentzero-sop`.
- [ ] Add CLI command surface: `agentzero skill list/install/test/remove`.
- [ ] Add CLI command surface: `agentzero tunnel ...` (secure tunnel lifecycle).
- [ ] Add plugin developer commands (`plugin new/validate/test/package/install`).
- Acceptance:
- Skills lifecycle and SOP execution are versioned and test covered.

## Sprint 13: Migration, Update, and Knowledge Expansion

### 13.1 Migration and self-update
- [ ] Add `crates/agentzero-migration` and `crates/agentzero-update`.
- Acceptance:
- Data import and binary update flows are recoverable and tested.

### 13.2 RAG and multimodal
- [ ] Add `crates/agentzero-rag` and `crates/agentzero-multimodal`.
- [ ] Add CLI command surface: `agentzero rag ingest/query` (feature-gated).
- Acceptance:
- Optional features are behind flags and do not bloat base runtime.

## Sprint 14: Hardware and Peripherals (Optional Profile)

### 14.1 Device support track
- [ ] Add `crates/agentzero-hardware` and `crates/agentzero-peripherals`.
- [ ] Keep hardware support feature-gated and off by default.
- Acceptance:
- Hardware mode can be enabled without impacting default lightweight profile.

## Backlog (Post-v1, Out of Current Scope)
- [ ] Additional channel providers beyond reference implementation.
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
- 2026-02-27: Upgraded `onboard` UX with branded header, colored/checkmark section progress, and searchable interactive prompts via `inquire`.
- 2026-02-27: Added typed `onboard` flags + env var resolution using option-spec traits and precedence `flag > env > default`.
