# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project follows Semantic Versioning.

## [Unreleased]


## [0.14.0] - 2026-04-20

### Added
- Cron execution loop — polls CronStore for due tasks and dispatches them to agents via the event bus. 13 unit tests.
- Trigger evaluation loop — subscribes to all bus events, evaluates against TriggerEngine rules, publishes trigger actions. 2 unit tests.
- Coordinator wiring for cron and trigger loops — `with_cron_store()`, `with_trigger_engine()` builder methods
- Cron task fire handling — routes `cron.task.fire` events to the best agent via AI router with fallback
- Trigger action handling — dispatches `trigger.*` events to named target agents
- Qwen model variants (Qwen2.5-Coder, Qwen2.5, Qwen3, QwQ-32B) with JSON-based model catalog
- GGUF registry mapping short model IDs to HuggingFace repos for auto-download
- Gateway handler module split and axum best practices audit

### Changed
- Consolidated 6 individual cron tools (cron_add, cron_list, cron_remove, cron_update, cron_pause, cron_resume) into unified `schedule` tool with natural language support
- Moved hardcoded model arrays to embedded JSON data file (`data/model_catalog.json`)
- Leaner crate builds with feature gates and lighter dependencies
- CronStore gains `mark_last_run()` for execution tracking
- Re-exported `AutopilotTriggerConfig`, `AutopilotTriggerCondition`, `AutopilotTriggerAction` from config crate

### Removed
- Duplicate WASM cron plugins (`plugins/agentzero-plugin-cron/`) — functionality consolidated into core ScheduleTool
- Cron plugin integration tests (all were `#[ignore]`, required pre-built WASM binaries)

## [0.13.0] - 2026-04-19

### Added
- Sprint 86 — capability-based security phase 1, threat model, docs — Phase 0: Close Sprint 85 — update threat model with 5 new attack surfaces
- Sprint 87 Phase A — DynamicToolDef capability bounding — - Add creator_capability_set: Option<CapabilitySet> to DynamicToolDef
- Add optional Turso/libSQL backend (memory-turso feature)
- Sprint 87 Phase E3 — MemoryConfig turso fields + build_autopilot_store factory
- Sprint 88 — MCP session scoping + A2A max_capabilities (Plan 49)
- Sprint 89 — WASM capability filtering + API key capability ceiling (Plan 50) — Phase H — WASM plugin capability integration:
- Sprint 90 — memory scope isolation + delegate max_capabilities ceiling (Plan 51) — Phase J — Memory Scope Isolation:
- Sprint 91 — file tool capability enforcement (Plan 52)
- Automatic tool-creation fallback on tool-not-found — When the LLM requests a tool that doesn't exist and enable_tool_fallback

### Fixed
- Resolve CI, Docker, and release workflow failures — - Add explicit rng.gen::<u8>() type annotations to fix Windows clippy
- Resolve Docker and Windows CI failures — - Remove hardcoded storage-encrypted from gateway's Cargo.toml so
- Gate codegen tests behind wasm-plugins, remove stale advisory ignore — - Add #[cfg(feature = "wasm-plugins")] to check_toolchain_passes test
- Restore cargo audit ignore for RUSTSEC-2026-0049 — rustls-webpki 0.102.8 is still in the dep tree via libsql → rustls 0.22.
- Install wasm32-wasip1 in CI, clean up stale deny.toml ignores — - Add wasm32-wasip1 target to checks and coverage jobs so codegen tests
- Resolve CI failures and document binary size budget — - deny.toml: restore RUSTSEC-2026-0097 ignore for rand 0.8.5 (direct
- Three remaining CI failures — - daily_driver.rs: guard daily_driver_full_lifecycle with #[cfg(unix)] —
- Skip daily_driver test file entirely on non-unix — Replace per-function #[cfg(unix)] with a file-level #![cfg(unix)].
- Use YAML single-quoted scalars in security_policy Windows tests — Windows temp dir paths contain backslashes (C:\Users\...) which the YAML
- Normalize SDK path to forward slashes in generated Cargo.toml — On Windows, sdk.display() produces paths like D:\a\...\agentzero-plugin-sdk.
- Dispatch TursoAutopilotStore locally via SqliteAutopilotStore to avoid libsql threading conflict in test binary
- Address clippy unnecessary sort warning
- Resolve clippy warnings in providers
- Resolve clippy match guard warning
- Address clippy warnings in infra insights
- Resolve clippy sort warning in orchestrator
- Resolve clippy match guard in dashboard
- Resolve clippy match guard in config ui

### Changed
- Cut dev build time via dep opt-level and debug settings — Root cause of slow 'just test':
- Optimize pre-commit hook for fast iteration — The old hook ran clippy --all-targets + unit tests on every commit,

### Changed
- Add Plan 48 — capability enforcement wire-through (Sprint 87)
- Mark Plan 48 / Sprint 87 fully complete — all acceptance criteria met
- Mark Plan 50 / Sprint 89 fully complete — all acceptance criteria met
- Add Plan 51 / Sprint 90 — memory scope isolation + delegate ceiling (COMPLETE)

### Changed
- Fix Node.js 20 deprecation annotations — - docker/setup-buildx-action v3 → v4 (node24; also fixes the transitive
- Fix last Node.js 20 annotation — own Trivy DB cache with actions/cache@v5 — trivy-action@0.35.0 (latest) pins actions/cache@v4.2.4 (Node.js 20)
- Add memory-turso check to CI matrix
- Speed up tests and fix security audits
- Limit pre-commit tests and enable sccache
- Ignore new rustls-webpki advisories

### Wip
- Finalize sprint-87 capability & swarm wiring + gossip/test fixes

## [0.12.0] - 2026-04-14

### Added
- Add agentzero-macros crate with #[tool] and #[derive(ToolSchema)] proc macros — Introduces proc macros that eliminate boilerplate in tool definitions:
- Migrate all ~70 production tools to #[tool] and #[derive(ToolSchema)] macros — Batch migration of every production tool across three crates:
- Composable LLM pipeline with LlmLayer trait, MetricsLayer, and CostCapLayer — Adds tower-style composable middleware for LLM providers:
- Add TypedTopic<M> for compile-time type-safe event bus pub/sub — Wraps the existing string-based EventBus with generic TypedTopic<M> that
- Guardrails pipeline layer with PII redaction and prompt injection detection — Adds composable guardrails as an LlmLayer with three enforcement modes:
- 12-layer security hardening with site documentation — Comprehensive security audit and hardening across the platform:
- Self-evolution engine scaffolding and local inference support — - tool_evolver: auto-fix/improve dynamic tools via LLM feedback loops
- Local LLM ecosystem — constrained decoding, chat templates, RAG chunking, local embeddings — Sprint 76 implementation:
- Candle Metal GPU acceleration — bump candle 0.9 → 0.10, enable Apple Silicon inference — Candle's Metal backend blocker (candle-metal-kernels alpha) has cleared.
- Runtime enhancements — audit replay, instruction injection, typed IDs, plugin shims, CoW overlay — Five enhancements inspired by external agent runtime research:
- End-to-end channel messaging — Signal integration, swarm auto-wiring, routing fixes — Complete the channel messaging pipeline so messages received on external
- Web search resilience, auto-search enrichment, swarm defaults — - Enable browser, HTTP, web fetch, and web search tools by default
- Add #[tool_fn] function-level proc macro for tool authoring — New macro that generates a complete Tool trait implementation from an
- Add Codegen strategy for dynamic tools — WASM compilation pipeline — Add 5th DynamicToolStrategy variant `Codegen` that compiles LLM-generated
- Wire codegen strategy into tool_create — LLM-generated WASM tools — Complete the codegen pipeline in tool_create:
- End-to-end codegen tests — compile Rust to WASM and execute — 9 tests for the codegen pipeline:
- Convert ConversationTimerangeTool to #[tool_fn], mark Sprint 80 complete — Proof-of-concept conversion: 65 lines → 23 lines using #[tool_fn].
- Production-harden event bus — multi-axis filtering, publish metrics, Arc payloads — Consolidate to one event bus hierarchy by deleting the orchestrator's
- Add pending modules — channels e2e tests, tool middleware, insights, trajectory, credential pool, message queue — Untracked files from prior branch work, now passing `just test` (3192/3192).
- Sprint 82 retrieval upgrade + Sprint 83 device detection & feature guards — This commit bundles Sprint 82 (retrieval quality upgrade) with Sprint 83
- Sprint 84 Phase A — gateway load harness + production-readiness docs — Phase A of the production-readiness pass: a pure-Rust load harness for the
- Sprint 84 Phases B+C — codegen kill-switch, unwrap audit — Phase B: Codegen dynamic tool strategy kill-switch
- Codegen audit log + gateway admin endpoints for runtime codegen control — Closes the two Sprint 84 follow-ups that were deferred from the kill-switch
- Sprint 85 Phases A+B — privacy-first provider calls (request IDs + mandatory PII stripping) — This is a core project safety guarantee: no PII reaches a remote LLM
- Sprint 85 Phase C — extended PII patterns (credit cards, JWT, SSH keys, DB URIs, IPv4) — Extends PiiRedactionGuard with 5 new detection patterns, bringing the total
- Plan 45 — Pi-inspired architecture patterns (4 phases) — Phase 1 — Progressive Skill Loading:
- Sprint 83 — on-device inference foundations (Phases A-E) — Phase C: LocalLlm trait + shared GenerationLoop that eliminates ~300 lines
- Production-ready security hardening (5 phases) — Phase 1 — Unified PII redaction: 18 shared patterns across audit sinks,
- Add property-based testing for security invariants (Phase C) — Sprint 85 Phase C: Add proptest to the workspace and write property-based
- Structured output, CI signing, UI cleanup (Sprint 85 final) — Structured output (llama.cpp grammar):

### Fixed
- Eliminate test leaks and stub real network calls — - Coordinator: abort all internal loop tasks (ingestion, router,

### Changed
- Strip non-core features + replace Supabase with SQLite in autopilot — Sprint 85 Phases A+B: Strategic scope reduction and local-first autopilot.
- Optimize pre-commit hook and release-auto build times — Pre-commit hook:

### Changed
- Add PII Protection page + update security overview and roadmap — New /security/pii-protection/ page documenting the core project safety
- Capability-based security design doc (Phase D) — Sprint 85 Phase D: Design document for replacing ToolSecurityPolicy's

### Changed
- Reduce dependencies and add `just install` — - Replace ureq with reqwest in agentzero-plugins, converting
- Add Sprint 80 plan — #[tool_fn] macro + WASM codegen strategy — Sprint 80 plan for two-phase tool authoring enhancement:
- Update Sprint 80 progress — Phase A complete, Phase B mostly done
- Add pending untracked files — AI editor configs, daily driver test, hypergrep setup, Tauri plan — Housekeeping commit for files that accumulated across recent sessions but
- Add commit signature verification scaffold — Advisory pre-push hook that verifies commits are signed by authorized

### Plan
- Sprint 76 — local LLM ecosystem with constrained decoding, chat templates, RAG pipeline — Add Sprint 76 to SPRINT.md and save detailed plan to specs/plans/.

## [0.10.0] - 2026-03-23

### Added
- Prepare all crates for crates.io publishing, bump to v0.9.0 — Add per-crate descriptions and remove publish = false from all library
- Add workflow topology dashboard with WASM-powered graph visualization — Replace the simple canvas-based TopologyGraph with an interactive
- Redesign dashboard with modern bento grid layout — Modern dashboard redesign with:
- Add WorkflowDetails panel, fix canvas sizing and click behavior — - WorkflowDetails: sidebar listing agents, tools, channels, and
- Add DraggablePalette and wire onDrop to WorkflowTopology — - DraggablePalette: draggable list of agents, tools, and channels
- Wire Blender-style ports into agent/tool/channel nodes — Define port schemas for each node type:
- Working drag-drop from palette to canvas with ports — Move drop handling from WASM to React level to bypass RefCell borrow
- Wire port-to-port connection with onConnect callback — When a user drags from an output port to an input port, the WASM
- Add KeySelector for JSON↔text port connections — When connecting ports with different types (json→text or text→json),
- Update workflow-graph to 0.7.8, Delete key removes nodes
- Drop nodes at cursor position, port-to-port edge rendering — - Drop handler passes mouse coordinates to addNode so nodes appear
- Redesign DraggablePalette with categories, search, and collapse — - Tools organized into categories: File & Search, Memory, Agents,
- Palette items rendered as miniature node previews — Each palette item is a MiniNode matching the canvas node style:
- Palette items as cute node chips — Small rounded pills with a colored dot matching node type:
- Persist workflow graph across page refresh — Add workflowStore (Zustand + persist middleware) to save added nodes
- Cmd+K palette, /workflows page, sidebar nav, fixes — - CommandPalette: dark backdrop (bg-black/60), selected item has
- Gateway offline page, fix write_file config — Dashboard shows a "Gateway Offline" page with WifiOff icon and the
- Clear button, create agent from Cmd+K, persistence fixes — - Clear button in topology toolbar removes all persisted added nodes
- Persist node layout positions across refresh — - workflowStore gains nodePositions: Record<string, [x, y]>
- Use workflow-graph getState/loadState for persistence — Replace custom addedNodes/nodePositions/edges store with workflow-graph's
- Inline Create Agent dialog + Quick Config panel
- Channels/schedules/gates in palette, fix layout restore — Palette now always shows:
- Use workflow-graph built-in persistence, remove all manual save code — WorkflowTopology now passes persist={{ key: 'agentzero-workflow-graph' }}
- Add CLI harness tools, 429 cooldown, and upstream integration plan — Phase 1A: CLI Harness Tools
- A2A tool, streaming support, per-sender rate limiting, fallback notification — Phase 1C: A2A Tool Interface + Spec Alignment
- Add A2UI Live Canvas — rich visual agent output — CanvasStore (agentzero-core):
- Add background and parallel delegation with TaskManager — TaskManager (agentzero-tools):
- Add deterministic SOP engine with typed steps and checkpoints — SOP Types (sop/types.rs):
- Add media pipeline, Discord history, voice wake, Gmail push channels — Phase 5A: Universal Media Pipeline
- Sprint 68 — streaming drafts, rate limiter, fallback headers, canvas registration, SOP config, A2A persistence
- Wire sender_id and source_channel into RuntimeExecution — Add source_channel and sender_id fields to RuntimeExecution so
- Workflow builder polish — detail panel, edges, undo, run button — Sprint 69: Visual Workflow Builder Polish
- ReactFlow workflow builder with collapsible nodes, connection validation, and compound groups — - Replace WASM workflow-graph with ReactFlow for full-featured node editor
- Add workflow execution engine — compiler with topological sort — Sprint 70 Phase A: Workflow graph compiler
- Add workflow execution engine with step dispatch and data routing — Sprint 70 Phase B: Workflow executor
- Add workflow execution API and wire UI to real endpoint — Sprint 70 Phases C+D: Gateway API + UI integration
- Edge condition editor, agent API loading, undo/redo toolbar — Sprint 69 deferred items:
- Workflow templates gallery, live execution viz, Sprint 71 plan — Sprint 71 Phase A: Workflow Template Gallery
- Add zoom-to-fit button and save/export template as JSON — Top-left toolbar now has three buttons:
- Add Cmd+Shift+? keyboard shortcuts panel — Dark-themed modal showing all canvas keyboard shortcuts:
- Save template dialog with name+description, fix Cmd+? shortcut — Save Template:
- Template save/load with localStorage fallback and dynamic node registry — Templates now persist to localStorage when the API is unavailable,
- Add /v1/templates API — separate store for reusable workflow templates — Templates are now stored separately from active workflows via a dedicated
- Workflow execution with real-time node status, human input, and ConverseTool — Add full workflow execution pipeline:
- Parallel workflow execution with tokio::JoinSet — Replace sequential batch execution in the workflow executor with true
- Sandboxed agent execution with git worktrees — Add AgentSandbox trait and WorktreeSandbox implementation for isolated
- Cross-agent context awareness with SwarmContext — Add SwarmContext for tracking agent assignments during swarm execution.
- Dead agent recovery with RecoveryMonitor — Add RecoveryMonitor that wraps PresenceStore for automatic detection
- Goal decomposition and swarm supervisor — Add GoalPlanner for decomposing natural language goals into workflow
- Container and microVM sandbox backends — Add ContainerSandbox (Docker/Podman) and MicroVmSandbox (Firecracker)
- CLI swarm command and gateway /v1/swarm endpoint — Wire the GoalPlanner and SwarmSupervisor into user-facing entry points
- Workflow export/import endpoints + real channel dispatch — - GET /v1/workflows/:id/export — returns full workflow JSON
- Gate suspend/resume for human-in-the-loop workflows — Add real suspend/resume mechanism for gate nodes. When a gate is
- Channel triggers, --ui flag, gate timeout — Three features completing Sprint 71 backend:
- Run cancel, SSE stream, delivery confirmation, keyboard nav — Four remaining Sprint 69-71 items:
- 14 integration tests for Sprint 71 backend features — Gateway endpoint tests (11):
- Workflow builder overhaul — provider ports, constants, group collapse, execution fixes — Workflow node system:
- Self-evolving agent system — NL goals, dynamic tools, catalog learning — Sprint 73: Agents can now be defined with natural language and self-assemble

### Fixed
- Resolve CI markdown-lint and security audit failures — - Fix MD029 (ordered list prefix) in sandbox.md: use 1/1/1 style
- Resolve all markdown-lint CI errors across docs — - Add blank lines below headings (MD022) in AGENTS.md, threat-model.md,
- Use published workflow-graph ^0.5.0 and fix theme layout fields — Switch from local link: to published @auser/workflow-graph-* ^0.5.0.
- Use DraggablePalette, larger nodes, port connection dragging — - Replace WorkflowDetails with DraggablePalette in dashboard bento grid
- Update workflow-graph to 0.6.3, tighter port hit radius
- Update workflow-graph to 0.6.4 — Canvas fills parent container, nodes clamp to visible area,
- Update workflow-graph to 0.6.5, fix WASM crash
- Update workflow-graph to 0.6.6, fix crash and node layout — Node name renders at top, port labels below. Click and drag works
- Update workflow-graph to 0.6.8, fix ResizeObserver crash — Remove [&>div]:h-full CSS hack — container sizing now handled
- Update workflow-graph to 0.7.1, all borrow panics resolved — All RefCell borrow panics fixed in workflow-graph v0.7.1:
- Update workflow-graph to 0.7.4, allow any port connections — Removes strict port_type matching so response (text) can connect
- Use WASM default node renderer, add KeySelector, debug logging — - Remove custom onRenderNode (was drawing without pan/zoom transform)
- Make drop resilient to stale WASM graph instance — Added nodes are tracked in React state (addedNodes) and merged into
- Update workflow-graph to 0.7.5, nodes persist across re-renders — New nodes from drops are synced to WASM on workflow prop changes.
- Update workflow-graph to 0.7.6, free node dragging — Nodes drag freely without position clamping. Works correctly
- Update workflow-graph to 0.7.7, fix ghost drag line
- Update workflow-graph to 0.7.9, canvas fills parent fully — Nodes render without clipping — canvas always matches parent
- Update workflow-graph to 0.8.1, fix WASM lifecycle — Synchronous setWorkflow init, StrictMode-safe destroy. Nodes
- Update workflow-graph to 0.8.2 with destroyed flag — All post-destroy WASM errors silenced. Event handlers, ResizeObserver,
- Disable React StrictMode to fix WASM canvas lifecycle — StrictMode's double mount/unmount causes the workflow-graph WASM
- Show KeySelector for any cross-type port connection — Previously only triggered for json↔text. Now shows for any type
- Replace hardcoded hex colors with Tailwind classes — - Create workflow-theme.ts with NODE_TYPES config using Tailwind classes
- Update workflow-graph to 0.8.4, reliable theme on init — Theme re-applied after setWorkflow completes. Parse errors warn
- Update workflow-graph to 0.8.5, JS-level destroy guard — All WASM calls guarded at JS level — destroyed instances silently
- GET /v1/tools uses user config instead of default policy — The tools endpoint was using ToolSecurityPolicy::default_for_workspace()
- CLI tools and gateway use user config for tool security policy — - CLI `tools list`: loads policy from user config via
- Workflow editor fills full available height — WorkflowTopology accepts fullHeight prop — on /workflows page the
- Sync node deletions from WASM back to persisted store — When Delete/Backspace is pressed, compare WASM nodes with the
- Layout positions survive topology poll resets — Saved positions stored in a ref and re-applied after every topology
- Migrate old workflow store format, guard null positions — The workflowStore format changed from addedNodes/edges/nodePositions
- InitDetector uses getState to verify WASM is fully ready — The previous check (graphRef.current.instance) was truthy immediately
- Event-driven save instead of timer, debug logging — Replace 2-second auto-save interval with event-driven saves:
- Save positions directly on drag end, bypass getState — getState() was returning null because the WorkflowGraph.alive check
- Restore positions immediately, don't wait for initialized — Position restore now runs on every workflow change (50ms delay),
- Instant layout restore via initialPositions prop — Remove all timers and InitDetector. Positions now flow as a prop
- Explicit width/height style on canvas to fill container
- Simplify NodeDetailPanel — unmount when closed, click-outside to dismiss — The panel was always rendered in the DOM with transform/opacity tricks,
- Add persistent Templates button and restore workflowId prop — Templates button now always visible in top-left of canvas (not just
- Inline template name input, save to server, zoom-to-fit shortcut — Save Template:
- Escape closes keyboard shortcuts panel
- PATCH not PUT for workflow/template updates, channel fallback in Cmd+K — - workflowsApi.update and templatesApi.update now use PATCH (matching

### Changed
- Auto-register node types from definitions, clean up CommandPalette
- Data-driven canvas actions registry for shortcuts + context menu — New canvas-actions.ts registry:

### Changed
- Update Sprint 60 acceptance criteria with completed items
- Update Sprint 60 to reflect actual progress — Phase 1: all workflow-graph features shipped (v0.8.2) — ports, drag-drop,
- Add AI chat bubble for agent creation to SPRINT.md — Future sprint item: floating chat widget powered by local model
- Update SPRINT.md, make auto-save interval configurable — Sprint 60 fully updated to reflect actual progress. All completed
- Add Phase 4 (agent creation + config) and Phase 5 (AI chat bubble) — Phase 4: Inline agent creation dialog from Cmd+K, quick config
- Add workflow-graph v2 design plan (specs/plans/28) — Design targets from chaiNNer and LangChain references:
- Update SPRINT.md with known bugs and remaining work — Three critical bugs documented:
- Update SPRINT.md with ReactFlow migration status
- Add Sprint 72 — Autonomous Agent Swarms plan — Event-driven task unblocking, sandboxed agent execution
- Mark ~30 stale Sprint 60-71 checkboxes as done — Reconcile SPRINT.md with actual codebase state. Many items from
- Check off export/import and channel dispatch in Sprint 71
- Mark remaining stale checkboxes in Sprints 60, 69 — - Sprint 60 Phase 6D (Template Gallery): all 5 items done

## [0.8.1] - 2026-03-20

### Added
- MCP Server Mode: expose tools via JSON-RPC over stdio (`agentzero mcp-serve`) and HTTP (`POST /mcp/message`)
- A2A Protocol: Agent Card discovery (`GET /.well-known/agent.json`), task lifecycle (`POST /a2a`), external agent client
- Ed25519 plugin signing: `generate_keypair()`, `sign_manifest()`, `verify_manifest()` with CLI commands
- Semantic memory: `EmbeddingProvider` trait, cosine similarity, `semantic_recall()` on MemoryStore, migration v6
- API embedding provider: OpenAI-compatible `/v1/embeddings` client
- `SemanticRecallTool`: vector similarity search over memory entries
- Privacy-aware model routing: `PrivacyLevel` enum (Local/Cloud/Either) on ModelRoute
- Declarative YAML security policies: `.agentzero/security-policy.yaml` with per-tool egress/command/filesystem rules
- 4 vertical agent packages: OSINT analyst, social media manager, browser QA, lead generation
- Docker CI workflow with ghcr.io publishing
- E2E Ollama test infrastructure with CI workflow
- A2A protocol and MCP Server Mode site documentation

### Changed
- `"private"` privacy mode: blocks network tools, auto-enables Noise Protocol, allows explicit cloud providers
- agentzero-lite defaults to `--privacy-mode private` with tighter rate limits (120 req/min)
- `POST /v1/tool-execute` now executes tools for real (no longer a stub)
- Embedded binary slimmed: plain SQLite + core tools only (~6-7MB, down from 11.7MB)
- Feature-gated serde_yaml (`yaml-policy`) and ed25519-dalek (`signing`) to reduce embedded size
- Pre-commit hook: fmt + clippy auto-fix + tests in same commit
- Branch protection enabled on main (requires PR with passing CI)

### Fixed
- Parse tool calls from text output for local model compatibility (Ollama, llama.cpp)
- Exclude FFI + plugin-sdk from test runs to prevent nextest hanging
- Docker build uses plain SQLite to avoid QEMU cross-compilation failures

## [0.8.0] - 2026-03-20

## [0.6.0] - 2026-03-14

### Added
- Extract tool calls from local model text output — Local models (llama.cpp, ollama) often emit tool invocations as JSON
- Compact tool prompting for local models and streamline research pipeline — Rewrite format_tools_system_block() to use a concise markdown format instead
- Hybrid local+cloud config for research pipeline example — Configure the research pipeline to mix local and cloud models:
- Remove legacy ProcessPluginTool and fix FFI test flakiness — Remove ProcessPluginTool (MCP strictly supersedes it) across 6 files:
- Add cost estimation, per-run budgets, and daily/monthly quota enforcement — Wire up the existing cost tracking skeleton with actual pricing data,
- Production hardening II — security, TLS, observability, data integrity, E2E tests — Sprint 37 closes all CRITICAL and HIGH gaps for external deployment:
- Sprint 38 scaling & ops — per-identity rate limiting, provider fallback, OpenAPI, backup/restore, production config validation — - Per-identity rate limiting with DashMap-based tracking, GC, and X-RateLimit headers
- Sprint 39 phases A-D — SQLite event bus, typed responses, circuit breaker, liveness probe — Phase A: Embedded distributed event bus (no Redis)
- Sprint 39 phases E-F + config — Turso migrations, multi-tenancy, event bus wiring — Phase E: Turso migration versioning
- Sprint 40 Phase A — AI-based tool selection — Add ToolSelector trait and three implementations (All, Keyword, AI) to
- Sprint 40 Phase B — GossipEventBus for distributed event propagation — TCP mesh gossip layer wrapping SqliteEventBus. Length-prefixed JSON wire
- Sprint 40 Phase C — CLI API key management (create/revoke/list) — Add `auth api-key create/revoke/list` subcommands to the CLI. Create
- Sprint 40 Phase D — EventBus integration wiring — Wire distributed event bus into JobStore, PresenceStore, and Gateway:
- Sprint 40 Phase E — add Twilio SMS channel implementation — New sms.rs with send() via Twilio REST API, 1600-char chunking,
- Sprint 40 Phase F — CI/CD hardening — - Add Trivy container image scanning to CI (container-scan job)
- Sprint 41 — wire persistent API key store in gateway startup — - Wire ApiKeyStore::persistent(data_dir) into gateway run() when data_dir
- Sprint 42 staged work — config UI, fuzz targets, code interpreter, media gen, docs — Adds agentzero-config-ui crate (React + ReactFlow visual config editor),
- Sprint 43 Phases A-C — AgentStore, CRUD API, webhook proxy — Add agent-as-a-service capabilities:
- Sprint 43 Phases D-F + Coordinator wiring — webhook auto-reg, config helpers, per-agent memory — Phase A (coordinator): register_dynamic_agent() / deregister_agent() with
- Sprint 43 completion — webhook wiring, coordinator convenience, tests — Wire webhook auto-registration into gateway handlers:

### Fixed
- Prevent llama.cpp abort when prompt exceeds context window — The builtin provider had no guard checking that the tokenized prompt
- Set n_batch to match n_ctx for builtin llama.cpp provider — llama.cpp asserts `n_tokens_all <= cparams.n_batch` during decode.
- Parse tool calls from code blocks and bare JSON in builtin provider — Local models frequently emit tool calls as ```json code blocks or bare
- Add repetition detection to builtin provider generation loop — Small local models (3B-7B) frequently get stuck in degenerate repetition
- Writer agent outputs to research/brief.md instead of output/brief.md — The write_file tool requires parent directories to exist (it canonicalizes
- Isolate pipeline agent conversations and switch to Brave search — Two fixes for the research pipeline:

### Changed
- Add Sprint 38 plan — scaling, ops readiness, provider fallback — Sprint 38 targets scaling and operational readiness:
- Add Sprint 40 plan — AI tool selection, gossip bus, CLI API keys, WhatsApp/SMS — Sprint 40 phases:
- Add Sprint 41 plan — security hardening & observability
- Add Sprint 42 plan — lightweight mode, examples, Docker secrets, runbooks
- Add "Your First Hour" getting-started guide — Hands-on walkthrough covering CLI agent, HTTP gateway, and multi-agent

### Changed
- Track fuzz workspace Cargo.lock for reproducible fuzzing

## [0.5.6] - 2026-03-11

### Fixed
- Add libssl-dev to Docker builder for SQLCipher/OpenSSL headers — The `memory-sqlite` feature enables `bundled-sqlcipher` which requires

## [0.5.5] - 2026-03-11

### Fixed
- Pin Swatinem/rust-cache to node24 commit SHA — The FORCE_JAVASCRIPT_ACTIONS_TO_NODE24 env var only works at runner

## [0.5.4] - 2026-03-11

### Fixed
- Replace flaky ollama e2e tests with deterministic mock providers — Rewrite e2e_local_llm tests to use scripted mock providers instead of

## [0.5.3] - 2026-03-11

### Fixed
- Bump Dockerfile Rust to 1.86 for wasmi/wiggle MSRV — wasmi 1.0.9, wasi-common 36.0.6, and wiggle 36.0.6 all require

## [0.5.2] - 2026-03-11

### Fixed
- Force Node.js 24 for GitHub Actions and wrap unsafe env mutations — Add FORCE_JAVASCRIPT_ACTIONS_TO_NODE24 env to ci and release workflows
- Bump Dockerfile Rust version to 1.85 for edition2024 support — wiggle-macro v36.0.6 (wasmtime dependency via plugins feature) requires

## [0.5.1] - 2026-03-11

### Fixed
- Repair release container build and CI rate-limit flake — Stop excluding testkit/bench from Docker context since Cargo needs their

## [0.5.0] - 2026-03-11

### Added
- OpenClaw multi-agent patterns — lanes, depth-gated tools, announce, async jobs API — Implement the core OpenClaw-inspired multi-agent orchestration stack:
- Sprint 33 — queue modes, cascade cancel, loop detection, event log, presence, block streaming — Phase A: QueueMode enum (Steer/Followup/Collect/Interrupt) in core types,
- Sprints 34-35 — delegation security hardening + hierarchical budgeting — Sprint 34: Delegation security — AutonomyPolicy::intersect() with
- Sprint 36 — production hardening (transcript, pooling, auth, telemetry, event bus, API keys) — Phase A: Sub-agent transcript archival via GET /v1/runs/:run_id/transcript
- First-class MCP server tools, test fixes, and sqlite migration guard — - Register each MCP server tool as its own Box<dyn Tool> with namespaced
- Add gateway smoke test scripts with gz() curl helper — Shell scripts to validate gateway endpoints work end-to-end without
- Add built-in local LLM provider (llama.cpp) and enable tool use for all providers — Adds a self-contained local model provider using llama-cpp-2 behind the
- Add ConverseTool for bidirectional agent-to-agent conversations — Enable multi-turn conversations between agents (and agent-to-human via

### Fixed
- Update quinn-proto to 0.11.14 to resolve RUSTSEC-2026-0037
- Update metrics grid layout for 4 items instead of 6
- Test-pipeline.sh ignores stale PAIRING_CODE when starting its own gateway — When the script starts a new gateway with --new-pairing, it now always
- Increase request_timeout_ms to 120s in example configs — The default 30s timeout is too short for research pipelines that do
- Repair .gitignore missing newline

### Changed
- Update site documentation for first-class MCP server tools — Update all MCP references across 7 doc pages to reflect the new
- Comprehensive README rewrite with quickstart for all options — - Rewrite README with provider-specific quickstarts (OpenRouter, Anthropic,
- Add MCP, Channels, and Multi-Agent guide pages to site
- Correct landing page metrics to match actual codebase — - Minimal Binary: ~5MB → <8MB (CI budget is 8MB)
- Remove platform targets and workspace crates from landing metrics
- Add local model documentation and improve builtin provider UX — Add builtin provider docs to providers guide, installation page, and

### Changed
- Archive Sprints 25-32, plan Sprint 33 (OpenClaw queue modes, cascade stop, loop detection) — Archive completed sprints to specs/sprints/25-32-privacy-e2e-multi-agent-production.md.
- Deploy GitHub Pages after every successful CI run on main — Changed trigger from push with path filters to workflow_run, so the
- Add e2e mock provider test and feature validation plan

## [0.4.2] - 2026-03-07

### Added
- Populate changelog entries and auto-generate with git-cliff — Populate empty v0.4.0 and v0.4.1 release notes with curated content

### Fixed
- Mark agentzero-testkit as unpublishable to fix release — agentzero-testkit depends on agentzero-providers (publish = false),
## [0.4.1] - 2026-03-06

### Added
- **Per-project .env overrides** — Dotenv chain now scans both `~/.agentzero/` and CWD for `.env`, `.env.local`, and `.env.{AGENTZERO_ENV}` files; CWD files take priority over config-dir files for per-project overrides; duplicate loading avoided when CWD matches the config directory

### Fixed
- Add missing version specifiers to all workspace dependencies — 14 of 16 internal deps lacked versions, causing crates.io publish failures; `bump-versions` recipe now inserts versions into deps that lack them
- Push branch before tag in release recipe — version bump and changelog commits were stranded on local branch

## [0.4.0] - 2026-03-06

### Added
- **Orchestrator crate extraction** — Moves coordinator, agent_router, and swarm modules from `agentzero-gateway` to new `agentzero-orchestrator` crate, separating agent coordination logic from HTTP/WS transport for dependency-free reuse
- **Integration tests and e2e local LLM testing (Sprint 28)** — StaticProvider-based integration tests for agent chaining, privacy routing, pipeline execution, graceful shutdown, and correlation tracking; testkit helpers (`local_llm_provider`, `local_llm_available`, `wait_for_server`); 4 e2e tests against Ollama/tinyllama (`#[ignore]`); `e2e-tests` CI job; 1,750 tests passing
- **Conversation branching, multi-modal input, plugin registry refresh (Sprint 29)** — `MemoryEntry.conversation_id` and `ToolContext.conversation_id` fields; `MemoryStore` trait gains `recent_for_conversation`, `fork_conversation`, `list_conversations` with SQLite migration; `ContentPart` enum (`Text`/`Image`) on `ConversationMessage::User`; Anthropic `InputContentBlock::Image` and OpenAI `image_url` data URI support; `load_image_refs()` and `build_user_message()` wiring; CLI `conversation list/fork/switch` commands; `az plugin refresh --registry-url` command
- **HTTP registry fetch, plugin dependencies, audio input (Sprint 30)** — `load_registry_index()` and `refresh_registry_index()` accept `http(s)://` URLs; `install_from_url()` streams remote WASM packages; `PluginDependency { id, version_req }` with transitive resolution and cycle detection; `[AUDIO:path]` markers transcribed via Whisper-compatible API; `AudioConfig { api_url, api_key, language, model }`; graceful degradation when no API key configured; 13 unit tests for audio processing

### Fixed
- Clippy `len_zero` lint in test code; add `--all-targets` to pre-commit hook so test-only lints are caught
- Release recipe auto-fixes fmt/clippy in place instead of running read-only checks that hard-fail
- Bump plugin/fixture `Cargo.toml` versions missed by release recipe; extend recipe to handle standalone version lines

### Changed
- Pre-commit hook runs `cargo fmt --all` and `cargo clippy --fix --allow-staged` in place, re-stages with `git add -u`, then verifies with a clean clippy check
- `bump-versions` extracted as standalone recipe (`just bump-versions X.Y.Z`) from the release recipe for independent use
- Site documentation updated for Sprint 30 commands, architecture, and env vars

## [0.3.0] - 2026-03-05

### Added
- **Privacy end-to-end enforcement (Sprint 25)** — Memory entries carry `privacy_boundary` and `source_channel` fields with `recent_for_boundary()` filtering; channel messages propagate boundaries with `dispatch_with_boundary()` blocking `local_only` → non-local channels; Noise IK handshake for 1-round-trip fast reconnect; `agentzero privacy test` runs 8 diagnostic checks; integration wiring through `ToolContext.privacy_boundary` and leak guard `check_boundary()`
- **Production-ready privacy system (Sprint 24)** — Gateway initializes NoiseSessionStore, RelayMailbox, and key rotation on startup; client-side Noise handshake (`NoiseClientHandshake`, `NoiseClientSession`, `NoiseHttpTransport`); `GET /v1/privacy/info` endpoint; sealed envelope replay protection (nonce dedup, HTTP 409); local provider URL enforcement; network-level tool enforcement in `local_only` mode; plugin network isolation; per-component privacy boundaries (`PrivacyBoundary` enum with `resolve()` for agents, tools, channels); 6 Prometheus privacy metrics
- **Gateway production readiness (Sprint 23)** — Real Prometheus metrics with request instrumentation; dynamic `/v1/models` from provider catalog; WebSocket hardening (heartbeat ping/pong, idle timeout, binary frame rejection); structured `GatewayError` with 8 variants and JSON error responses; provider tracing spans on all 8 methods; storage test expansion (19 → 46 tests)
- Privacy CLI commands: `privacy status`, `privacy rotate-keys [--force]`, `privacy generate-keypair`, `privacy test [--json]`
- Noise Protocol handshake patterns: XX (mutual auth) and IK (known server key, fast reconnect)
- Per-component privacy boundaries for agents, tools, and channels with child-can't-exceed-parent enforcement
- Config validation: rejects `encrypted` mode without `noise.enabled`, boundary escalation, non-localhost URLs in `local_only`
- Responsive mobile navigation with hamburger menu for documentation site
- **Timing jitter for sealed envelope relay (Sprint 26)** — `JitterConfig` with configurable min/max delays for submit (10–100 ms) and poll (20–200 ms) to mitigate traffic analysis; wired through `SealedEnvelopeConfig` → `RelayMailbox::with_jitter()`
- **Privacy benchmarks (Sprint 26)** — Criterion 0.5 benchmarks for Noise keypair generation, XX/IK handshakes, encrypt/decrypt at 64B/1KB/64KB, sealed envelope seal+open, routing ID computation (11 functions behind `privacy` feature)
- **FFI privacy bindings (Sprint 26)** — `PrivacyBoundary`, `PrivacyInfo`, `PrivacyStatus` types exposed through UniFFI (Swift/Kotlin) and napi-rs (Node) for inspecting privacy state from mobile/Node apps

### Fixed
- Fix flaky `keyring_data_is_encrypted_on_disk` test — replace brittle 2-char substring check with longer plaintext field name assertions
- Fix flaky `set_config_value_creates_nested_keys` test — use unique temp dir to prevent parallel test collisions
- Resolve clippy `double_ended_iterator_last` lint for Rust 1.93
- Use vendored-openssl only on Windows, system OpenSSL elsewhere
- Noise middleware: empty-body requests with session header now get encrypted responses
- `IdentityKeyPair` no longer implements `Serialize` (prevents secret key leaks)

### Changed
- Privacy metrics (`record_key_rotation`, `record_encrypt_duration`) wired into actual code paths
- Pre-commit hook optimized: `cargo fmt --check` (read-only) instead of rewrite+re-stage

## [0.1.4] - 2026-03-02

## [0.1.3] - 2026-03-03

### Added
- Expand release build matrix: linux-armv7, linux-x86_64-musl, linux-aarch64-musl targets
- Installer auto-selects static musl binaries on Linux for better portability

### Fixed
- Fix stale v0.1.2 tag that pointed to a commit missing the changelog entry

## [0.1.2] - 2026-03-03

### Fixed
- Windows build: compare `HANDLE` with `.is_null()` instead of `== 0` to fix `E0308` mismatched-types errors
- Windows build: prefix unused `path` parameter in `enforce_private_permissions` with `_` to silence unused-variable warnings on non-Unix targets
- Add Windows support to `agentzero-daemon` via platform-conditional compilation (`#[cfg(unix)]` / `#[cfg(windows)]`)
- Resolve CI failures in checks, coverage, and security jobs
- Update `deny.toml` for `cargo-deny` config schema change
- Update path references from `public/` to `site/`
- Resolve CI/CD failures, upgrade wasmtime, and consolidate workflows

## [0.2.1] - 2026-03-02

### Fixed
- Eliminate flaky test failures from temp directory collisions (add PID to temp dir names)
- Skip TTY-dependent dashboard test when running in interactive terminal
- Use dynamic version in release verification benchmark test

### Added
- Channel setup module with `register_configured_channels` and `channels-standard` feature flag
- Expanded delegate tool with coordination, status tracking, and multi-agent support
- Gateway auth hardening with additional token validation and tests
- Config test coverage for policy flags (git, web_search, browser)
- `just release` now auto-bumps workspace version in Cargo.toml

## [0.2.0] - 2026-03-01

### Added
- Full tool parity: SOP tools (5), CLI discovery, proxy config, composio, pushover, hardware debug tools (3), WASM plugin tools (2)
- Full channel parity: 23 channels (Telegram, Discord, Slack, WhatsApp, Signal, iMessage, Matrix, Mattermost, IRC, Email, Nostr, Lark, Feishu, DingTalk, Nextcloud Talk, LinQ, WATI, QQ Official, Napcat, ACP, ClawdTalk, CLI)
- Template system: 8 templates with 3-tier precedence, discovery, validation, and CLI commands
- CLI completeness: skill new/audit/templates, 113 integration tests, gateway manual test script
- Persistence migration: all sensitive state uses encrypted stores
- Channel binding generalization: unified `channel add/remove` flow
- Workspace version consolidation: all crates use `version.workspace = true`
- Providers command improvements (table output, colorization, JSON mode)

### Changed
- Provider module renamed and split into `agentzero-providers` crate
- Removed `bind-telegram` special-case in favor of generic `channel add telegram`

## [0.1.0] - 2026-02-28

### Added
- Initial multi-crate workspace with CLI, runtime, config, core, tools, gateway, and security foundations.
- Interactive onboarding flow and initial command surfaces (`onboard`, `status`, `agent`, `gateway`, `doctor`, `providers`).
- Tool security policies, audit support, and baseline observability/bench harness.
