# Sprint: AgentZero Bootstrap

## Goal

Establish the documentation, ADR, security, and implementation foundation for AgentZero as a local-first secure AI agent harness.

## Active Plan

- `specs/plans/0001-bootstrap-agentzero.md`

## Current Phase

**Status: PHASE 16 COMPLETE**

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

## Not Yet (deferred)

- [ ] MVM runtime integration (planned, waiting on `mvm` project maturity).
- [ ] Remote package registry (authenticated, with lockfile verification).

## Notes

This sprint intentionally prevents platform creep. The first implementation milestone is a small local secure session engine, not a hosted platform, workflow orchestrator, swarm runtime, or package marketplace.
