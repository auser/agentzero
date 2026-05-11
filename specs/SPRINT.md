# Sprint: Editor-Configurable Coding Agent (Pi Model)

## Goal

Make `agentzero serve` a fully functional coding agent that editors can spawn and talk to — like Pi (pi.dev), not like a passive MCP tool server. Extract the agentic loop, wire ACP Chat, add dynamic provider loading, print/JSON mode, editor config generators, and an edit tool.

## Active Plan

- `specs/plans/0003-editor-configurable-coding-agent.md`

## Current Phase

**Status: ALL 6 PHASES COMPLETE + ModelProvider trait extended**

### Phase 1: Extract AgentLoop (COMPLETE)
- [x] Create `agent_loop.rs` with `AgentLoop`, `AgentLoopConfig`, `AgentResponse`, `ApprovalHandler`, `ProgressHandler`
- [x] Export from `agentzero-session/src/lib.rs`
- [x] Refactor `cmd_chat` in CLI to use `AgentLoop`
- [x] Tests pass: `cargo test -p agentzero-session` (65 tests)

### Phase 2: Wire ACP Chat to AgentLoop (COMPLETE)
- [x] Add `AcpNotification`, `ListModels`, `SwitchModel`, `ApproveAction`, `Cancel` to ACP protocol
- [x] Wire `AcpServer::Chat` to `AgentLoop::send()` with real LLM inference + tool calls
- [x] ACP server loads policy and settings, constructs `ProviderRouter` + `AgentLoop`
- [x] Progress notifications sent over stdout during tool execution
- [x] 15 ACP tests pass (4 new)

### Phase 3: Dynamic Provider Loading + ModelProvider Trait (COMPLETE)
- [x] Extended `ModelProvider` trait with `chat_with_tools`, `chat_streaming`, `health_check`, `model_name`
- [x] Both `OllamaProvider` and `OpenAICompatProvider` implement full trait
- [x] `LocalStubProvider` implements full trait (for testing)
- [x] `ProviderRouter` uses `Box<dyn ModelProvider>` instead of hardcoded fields
- [x] `ModelsConfig` / `ProviderConfig` structs in `models_config.rs`
- [x] `ProviderRouter::from_config()` loads providers dynamically from `models.json`
- [x] `ProviderRouter::list_models()` returns all configured providers
- [x] `ProviderType::Ollama` and `ProviderType::OpenAICompatible` with serde support
- [x] 15 new tests (4 models_config, 4 router, 4 provider, 3 misc)

### Phase 4: Print/JSON Mode for CLI (COMPLETE)
- [x] `--print/-P <message>` flag for single-shot queries
- [x] `--mode json|text|jsonl` flag
- [x] JSON output: `{"content", "tool_calls", "model", "session_id", "rounds"}`
- [x] 2 parse tests for print mode

### Phase 5: Editor Configuration Generators (COMPLETE)
- [x] `az init --editor vscode|cursor|zed` flag
- [x] VS Code: `.vscode/tasks.json` with ACP server task + single-query task with input prompt
- [x] Cursor: `.cursor/rules` with MCP integration instructions
- [x] Zed: `.zed/tasks.json` with ACP server + chat tasks
- [x] Parse test for --editor flag

### Phase 6: Edit Tool (COMPLETE)
- [x] `edit_file(path, old_text, new_text)` in `ToolExecutor` with policy check + path validation
- [x] Register `edit` tool schema in providers (6th tool)
- [x] Session dispatch for `edit` tool
- [x] Returns diff summary
- [x] 2 edit tests (success + not-found)

## Tasks

### Phase 0: Documentation Gate
- [x] Define `specs/project.md`.
- [x] Add `specs/security-model.md`.
- [x] Add ADRs 0001-0010.
- [x] Add bootstrap plan and Claude Code prompts.
- [x] Run `just ci` in target repository.

### Phase 1: Rust Workspace
- [x] Create Rust workspace (7 crates: core, policy, audit, tools, skills, sandbox, cli).
- [x] Implement CLI skeleton (`doctor`, `demo`, `init`, `chat`, `run`, `policy`, `audit`, `vault`).

### Phase 2: CLI Commands
- [x] `agentzero init --private` creates `.agentzero/` with policy.yml.
- [x] `agentzero doctor` reports crate status and project config.
- [x] `agentzero policy status` shows loaded policy.
- [x] `agentzero audit tail` reads JSONL audit logs.
- [x] `agentzero vault list` reports configured handles.

### Phase 3: Security Primitives
- [x] Data classification enum with model routing rules (ADR 0002).
- [x] Policy engine with rule-based evaluation and deny-by-default (ADR 0003).
- [x] Redaction interface with token-preserving placeholders.
- [x] Secret handles with capability-based access (ADR 0009).
- [x] Trust source labels for content provenance (ADR 0008).
- [x] Model routing decisions (local/remote/redact/deny).
- [x] Typed action kinds for audit events.
- [x] Audit event schema with JSONL sink and in-memory sink.
- [x] Approval scope model.
- [x] `agentzero demo` exercises all security primitives end-to-end.

### Phase 4: Minimal Session Engine
- [x] Local-only session mode (`SessionMode::LocalOnly`).
- [x] Model provider abstraction (`ModelProvider` trait, `LocalStubProvider`).
- [x] Supervised tool invocation (`ToolExecutor` with policy checks + path validation).
- [x] Read/list/search tools (real filesystem operations).
- [x] Proposed edit output (`propose_edit` tool — output only, no writes).
- [x] Shell approval flow (policy-based `RequiresApproval` for shell commands).
- [x] Centralized tracing crate (`agentzero-tracing` wrapping `tracing` + `tracing-subscriber`).
- [x] Session engine (`agentzero-session` — ties model/tools/policy/audit together).

### Phase 5: First Demo
- [x] Built-in `repo-security-audit` skill with pattern-based scanner.
- [x] Patterns loaded from external `patterns.toml` (extensible without code changes).
- [x] Run against this repository (`agentzero run repo-security-audit`).
- [x] Human-readable markdown audit report with severity, recommendations.
- [x] Report written to `.agentzero/audit/` when project is initialized.
- [x] Malicious fixture tests (secrets, PII, prompt injection, package scripts, sensitive files).
- [x] 12 scanner tests + 3 report tests covering all finding categories.

### Phase 6: Policy Loading, Model Provider, Chat Loop
- [x] Policy YAML/TOML loader (`load_policy_file` parses `.agentzero/policy.yml` into rules).
- [x] `init --private` writes valid TOML policy files.
- [x] `policy status` loads and displays active rules.
- [x] Ollama model provider (`OllamaProvider` — local HTTP client for Ollama REST API).
- [x] Health check, chat completion, configurable model/endpoint.
- [x] Interactive chat loop (`agentzero chat --local` with Ollama backend).
- [x] Policy loaded from project config at chat startup.
- [x] System prompt, conversation history, `/quit` command.

### Phase 7: Tool Use, Streaming, Shell Approval
- [x] Ollama tool calling (model sends tool_calls, session executes them).
- [x] Tool definitions for read/list/search/shell sent to Ollama.
- [x] Multi-round tool loop (up to 5 rounds of tool calls per user message).
- [x] Shell command execution with user approval prompt (y/n).
- [x] Streaming chat support (`chat_streaming` with token-by-token callback).
- [x] Tool results truncated to 2KB to avoid context overflow.
- [x] Session wired into chat with policy-controlled tool executor.
- [x] Audit events emitted for each tool call during chat.

### Phase 8: CLI Polish, File Write Tool, Streaming, Bin Setup
- [x] `--model` flag for `agentzero chat` (e.g. `--model codellama`).
- [x] `--stream` flag for token-by-token output in chat.
- [x] File write tool with user approval prompt (y/n before writing).
- [x] `/tools` slash command shows available tools during chat.
- [x] `/session` slash command shows session info.
- [x] `default-members` in workspace so `cargo run` defaults to CLI binary.
- [x] 5 tool definitions (read, list, search, write, shell).

### Phase 9: Persistence, History, WASM Skeleton
- [x] Audit logging to `.agentzero/audit/` during chat sessions.
- [x] Conversation persistence — saves chat history to `.agentzero/sessions/<id>.json`.
- [x] `agentzero history` command lists past sessions with metadata.
- [x] WASM sandbox engine (`wasmtime`-backed, behind `wasm` feature flag).
- [x] WASM config: memory limits, fuel-based time limits, no ambient filesystem.
- [x] Stub types when `wasm` feature is disabled (compiles without wasmtime).

### Phase 10: OpenAI-Compatible Provider + Encryption at Rest
- [x] OpenAI-compatible provider (llama.cpp, vLLM, LM Studio, LocalAI, text-gen-webui).
- [x] `--provider` flag: ollama, llama-cpp, vllm, lm-studio.
- [x] `--url` flag for custom server endpoints.
- [x] Provider dispatches to Ollama API or OpenAI `/v1/chat/completions` API.
- [x] Tool calling works with both providers.
- [x] AES-256-GCM encryption with Argon2id key derivation.
- [x] `encrypt`/`decrypt` for raw bytes, strings, and files.
- [x] Base64 encoding for string transport.
- [x] 8 crypto tests (roundtrip, wrong passphrase, uniqueness, file ops).
- [x] 6 openai-compat tests (config variants, classification routing).

### Phase 11: Encrypted Persistence, Resume, Skill Install
- [x] Encrypted audit logger (AES-256-GCM, per-line encryption, base64 transport).
- [x] `--encrypt` flag on chat for encrypted audit + session files at rest.
- [x] Passphrase prompt at session start when --encrypt is set.
- [x] Encrypted session save (`.json.enc` files).
- [x] `--resume <id>` loads message history from past session.
- [x] `agentzero install <path>` copies skill directory into `skills/`.
- [x] Validates SKILL.md presence, detects runtime, avoids duplicate installs.
- [x] 3 encrypted audit tests + 3 CLI parse tests.

### Phase 12: Pipeline Hardening
- [x] Redaction engine wired into session: `prepare_for_model()` scans content for PII/secrets.
- [x] `AllowWithRedaction` policy decisions now actually redact before sending to remote providers.
- [x] Comprehensive audit events: session start, session end, model calls (local/remote/denied/redacted), tool executions.
- [x] Every policy decision emitted as an audit event with reason and redaction list.
- [x] `session.end()` emits session_end lifecycle event.
- [x] `init --private` creates complete `.agentzero/` structure: policy.yml, settings.toml, models.json, audit/, sessions/, prompts/, skills/, vault/.
- [x] 4 new session tests: prepare_for_model (local pass-through), redact PII, redact secrets, session end.

### Phase 13: Secret Vault, Content Provenance, Skill Discovery, ACP
- [x] Encrypted secret vault (`vault add/get/remove/list`), AES-256-GCM per-secret encryption.
- [x] `agentzero vault add github token` stores encrypted in `.agentzero/vault/github/token.enc`.
- [x] `resolve_for_execution()` — only path where raw secret material is exposed.
- [x] Content provenance: tool output wrapped with `[UNTRUSTED TOOL OUTPUT]` markers per ADR 0008.
- [x] Discoverable skill runner: `agentzero run <name>` scans `skills/` for installed skills.
- [x] Installed skills with `patterns.toml` run the scanner automatically.
- [x] ACP adapter crate (`agentzero-acp`) — JSON-RPC over stdio protocol.
- [x] ACP methods: initialize, chat, tool_call, session_info, list_tools, shutdown.
- [x] ACP server reads NDJSON from stdin, dispatches, writes responses to stdout.
- [x] 7 vault tests + 7 ACP tests + 4 protocol tests.

### Phase 14: ACP Serve, Settings Loading, Improved Doctor
- [x] `agentzero serve` starts ACP server on stdio for editor integrations.
- [x] `settings.toml` loading: default_provider and default_model from project config.
- [x] Settings override CLI defaults when flags are at default values.
- [x] Improved `doctor`: shows installed skills, vault secrets count, session count, provider list.
- [x] Doctor loads and reports policy rules, settings, and ACP availability.

### Phase 15: MCP Server
- [x] MCP server crate (`agentzero-mcp`) implementing Model Context Protocol.
- [x] JSON-RPC 2.0 over stdio transport (newline-delimited).
- [x] `agentzero mcp` starts server — any MCP client can connect.
- [x] `initialize` returns server info + capabilities.
- [x] `tools/list` returns 5 tools with proper JSON schemas.
- [x] `tools/call` executes tools through AgentZero session with policy enforcement.
- [x] read_file, list_directory, search_files, write_file, run_command tools.
- [x] Policy loaded from .agentzero/policy.yml at startup.
- [x] Tool execution goes through full session audit trail.
- [x] 13 MCP tests (protocol, server handlers, tool execution).

### Phase 16: Tier 3 — ACP Wiring, Context Compaction, Prompts, Git Install
- [x] ACP server wired to real session engine (tool_call executes tools with policy).
- [x] ACP session_info returns real session ID.
- [x] Context compaction: auto-summarizes older messages when conversation exceeds limits.
- [x] Preserves system message + recent N messages, summarizes middle.
- [x] Prompt templates: loads system prompt from `.agentzero/prompts/system.md`.
- [x] Falls back to built-in default prompt when no custom prompt exists.
- [x] Git-based skill install: `agentzero install https://github.com/user/skill`.
- [x] Clones with `--depth 1`, removes `.git`, validates SKILL.md.
- [x] 6 context compaction tests + 4 ACP tool execution tests.

### Phase 17-18: Multi-provider Routing, Retry, Package Ecosystem, v0.1.0 Prep
- [x] Multi-provider router: tries local first, falls back to remote.
- [x] Remote providers skipped for secret/credential classifications.
- [x] Retry with exponential backoff + jitter (only for Unavailable errors).
- [x] Skill registry with lockfile (`.agentzero/skills.lock`).
- [x] Lockfile tracks name, version, source, runtime, permissions, checksum.
- [x] `scan_installed()` discovers skills from filesystem.
- [x] GitHub Actions CI workflow (check, test, clippy, fmt, docs).
- [x] README rewrite for v0.1.0 (install, quick start, MCP integration, architecture).

### Phase 19: WASM Sandbox Integration
- [x] Runtime-aware policy rule matching (`CapabilityAndRuntime` matcher, `allow_runtime`/`deny_runtime`).
- [x] `wasm_execution` policy field in loader (allow/require_approval/deny, fail closed).
- [x] `Session::execute_skill()` routing by `SkillRuntime` (InstructionOnly, WASM, unsupported).
- [x] `ToolExecutor::execute_wasm()` with cfg-gated real/stub implementations.
- [x] `AuditParams` struct for runtime-tier-aware audit events with skill IDs.
- [x] `registry::load_manifest()` builds `SkillManifest` from SKILL.md frontmatter.
- [x] `registry::find_wasm_module()` locates `.wasm` files in skill directories.
- [x] CLI `cmd_run` rewritten: manifest load → runtime routing → WASM pipeline.
- [x] CLI `cmd_run_wasm_skill`: full session pipeline (policy → sandbox → audit → output).
- [x] `wasm` feature flag wired through workspace: cli → facade → session → sandbox → wasmtime.
- [x] `agentzero init` generates `wasm_execution` field in policy.yml.
- [x] `agentzero doctor` reports WASM compilation status, policy setting, WASM skill count.
- [x] Integration tests: hand-crafted WASM module (main→42) tested through WasmEngine (4 tests) and full Session::execute_skill pipeline (2 tests).
- [x] Website docs updated: skills guide, policy guide, CLI reference, crate map, security model, architecture overview.

### Phase 20: Runtime Quick Wins
- [x] Default `wasm` feature enabled across all crates (sandbox, session, facade, CLI).
- [x] `HostSupervised` skill runtime fully wired: entrypoint resolution, shell_command execution, policy gating, audit trail.
- [x] `SkillManifest` gains `entrypoint` field (optional, parsed from SKILL.md frontmatter).
- [x] `SkillMetadata` struct replaces tuple return from `parse_skill_metadata`.
- [x] All runtime tiers working: None (InstructionOnly), HostSupervised, WasmSandbox. Only MVM deferred.

### Phase 21: Remote Registry & Publish
- [x] `package.rs` — tarball creation (tar.gz), SHA-256 checksums, extraction, round-trip verification.
- [x] `remote.rs` — `parse_skill_ref()` dispatches user input to Local, GitUrl, or GitHub owner/repo.
- [x] `github.rs` — async GitHub API client: fetch releases, create releases, upload assets, download tarballs.
- [x] `SkillPackageRef::GitHub { owner, repo, version }` variant added.
- [x] `RegistryError::ChecksumMismatch` for integrity verification.
- [x] `agentzero install owner/repo` — resolves latest GitHub release, downloads tarball, verifies SHA-256, extracts, updates lockfile.
- [x] `agentzero install https://github.com/owner/repo` — recognized as GitHub ref, uses API instead of git clone.
- [x] `agentzero publish --repo owner/repo` — packages skill, creates GitHub release with checksum, uploads tarball.
- [x] All install paths (local, git, GitHub) now update `.agentzero/skills.lock` with full metadata.
- [x] Website docs updated: skills guide (install, publish, host-supervised), CLI reference, crate map.
- [x] 21 new tests across package (5), remote (12), github (4).

### Phase 23: Security Hardening (IN PROGRESS)

Prerequisite for self-improving agent work. Fixes security gaps found during gap analysis.

- [x] Block `.agentzero/` in tool path blocklist (prevents policy/vault info disclosure)
- [x] TOCTOU fix: canonicalize path before policy check, use resolved path for operations
- [x] Redact tool arguments in ToolCallRecord and approval flow (prevents secret leakage)
- [x] Scan tool output for secrets before audit logging (redaction labels in audit events)
- [x] WASM import verification: reject modules with imports not currently provided
- [x] Verify wasmtime version not affected by CVE-2026-34971 (v29.0.1 — not affected)
- [x] Implement approval scope tracking (Once/Session via ApprovedForSession)
- [x] Extract redaction scanning into agentzero-core as shared public utility

### Phase 24: Self-Improving Agent Foundation (IN PROGRESS)

WASM host imports, codegen, and dynamic tool registration per ADR 0012.

- [x] ADRs 0012 (Self-Improving via WASM), 0013 (WIT), 0014 (MCP Deprecation)
- [x] `az:host` WIT interface definition (`crates/agentzero-sandbox/wit/az-host.wit`)
- [x] WASM host imports via wasmtime Linker (`az::log`, `az::read_file`, `az::write_file`)
- [x] `WasmHostCallbacks` trait for injectable host functions
- [x] `wasm-encoder` template-based tool generation (PureComputation, Logger, FileReader)
- [x] `DynamicToolRegistry` — per-project tool storage with directory-based versioning
- [x] `AgentLoop::generate_and_register_tool()` — end-to-end codegen → registration
- [x] `generate_tool` built-in tool callable by the LLM during the agent loop
- [x] Wire `WasmHostCallbacks` to `ToolExecutor` + `PolicyEngine` in session
- [ ] Extended filesystem host imports: `list-dir`, `create-dir`, `file-exists`, `append-file`
- [ ] Clock host import: `now` (ISO 8601)
- [ ] WIT spec bumped to `az:host@0.2.0`
- [ ] ADR 0015: Personal Brain as WASM Plugin

### Phase 25: Provider & Onboarding (COMPLETE)

- [x] Anthropic Claude provider (Messages API, tool calling, system prompt extraction)
- [x] MCP moved to `--features mcp` optional flag (ADR 0014)
- [x] `az bootstrap` command (platform detection, backend probing, install orchestration)
- [x] `ProviderType::Anthropic` in models_config with router wiring
- [x] OpenAI-compat verified against: Groq, Together, DeepSeek (all use /v1/chat/completions)

### Phase 26: Marketplace & Catalog (COMPLETE)

- [x] TrustTier enum (Verified, Community, Generated) in SkillIndexEntry
- [x] Catalog search: SkillIndex::search() with name/description/tag matching
- [x] `az search` command with JSON output support
- [x] `az link` for cross-project tool sharing via symlinks
- [x] Extended SkillIndexEntry with author, tags, trust fields
- [ ] Javy embedding (Tier 2 JS→WASM compilation, deferred — significant dependency)
- [ ] `az publish --catalog` workflow (deferred — needs catalog repo infrastructure)

### Phase 27: Polish (COMPLETE)

- [x] Random redaction placeholders (hex suffix instead of sequential indices)
- [x] `az audit summary` with human-readable and JSON output
- [x] Multi-model routing config (`tool_generation_model`, `max_tools_in_context` in AgentLoopConfig)
- [x] `az vault-import` for migrating secrets from .env files

## Not Yet (deferred)

- [ ] MVM runtime integration (planned, waiting on `mvm` project maturity).
- [ ] Lockfile checksum re-verification on `agentzero run`.
- [ ] Javy embedding for Tier 2 tool generation
- [ ] `az publish --catalog` with PR-based submission
- [ ] Brain plugin (personal LLM wiki) — WASM plugin per ADR 0015
  - Spec: `specs/prompts/0006-agentzero-brain-production-plugin-prompt.md`
  - Plan: `specs/plans/0004-brain-plugin.md`
  - MVP: `brain init`, `brain today`, `brain capture`, `brain query`
  - Blocked on: Phase 24 extended host imports (`list-dir`, `create-dir`, `file-exists`, `append-file`, `now`)

## Notes

This sprint intentionally prevents platform creep. The first implementation milestone is a small local secure session engine, not a hosted platform, workflow orchestrator, swarm runtime, or package marketplace.
