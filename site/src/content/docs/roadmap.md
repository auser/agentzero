---
title: Roadmap
description: AgentZero development roadmap — completed milestones and future direction.
---

## Completed

### Foundation & Core (Phases 0-4)

- Workspace setup, CI, CLI shell with `onboard`, `agent`, `status` commands
- Core domain types and traits: `Provider`, `MemoryStore`, `Tool`, `Channel`
- OpenAI-compatible provider, SQLite memory, `read_file` and `shell` tools
- Agent loop hardening (max iterations, timeouts, event logging)
- TOML config, env overrides, secret redaction, security defaults

### Runtime Expansion

- Gateway HTTP server (Axum) with pairing auth, rate limiting, CORS
- WASM plugin sandbox with integrity verification
- Channel integrations (Telegram, Discord, Slack)
- FFI bindings (Swift, Kotlin, Python via UniFFI; Node.js via napi-rs)
- 35+ LLM provider support via OpenAI-compatible interface
- Autonomy levels, OTP approval, audit trails
- Hardware discovery, cron scheduling, skills/SOP engine

### Workspace Consolidation (Sprint 20)

- Workspace consolidated from 46 to 16 crates
- Encrypted SQLite with SQLCipher
- Plugin security hardening (path traversal fix, semver, debouncing, file locking)
- Replaced wasmtime with wasmi as default WASM runtime
- Build variant tooling (default, server, minimal)
- 1,400+ tests passing, 0 clippy warnings

### Structured Tool Use (Sprint 21)

- Provider tool definitions (`ToolDefinition`, `ToolUseRequest`, `ToolResultMessage`)
- Structured tool dispatch in agent loop with text-based fallback
- Conversation message history with `Vec<ConversationMessage>`
- Streaming tool use with `ToolCallDelta` and SSE parsing
- JSON Schema validation and `agentzero tools list/info/schema` CLI commands
- All 50+ tools implement `input_schema()`

### Streaming & Agent Wiring (Sprint 22)

- **Streaming agent loop** — `Agent::respond_streaming()` with `StreamSink` / `StreamChunk`
- **Runtime streaming channel** — `run_agent_streaming()` returning receiver + join handle
- **CLI `--stream` flag** — `agentzero agent --stream -m "hello"`
- **System prompt support** — `system_prompt` in AgentConfig, wired through all providers
- **Gateway agent wiring** — Real agent calls on `/api/chat`, `/v1/chat/completions`, `/ws/chat`
- **SSE streaming** — OpenAI-compatible SSE on `/v1/chat/completions?stream=true`
- **WebSocket streaming** — Bidirectional streaming on `/ws/chat`
- **MCP connection caching** — `McpSession` with cached subprocess connections and tool schemas
- **FFI Node.js parity** — `register_tool()`, `send_message_async()`, `registered_tool_names()`

### Hardening & Polish (Sprint 22H)

- JSON schema validation wired into tool dispatch (`prepare_tool_input()`)
- Config validation for `gateway.port`, `gateway.host`, `autonomy.level`, `max_cost_per_day_cents`
- Unsafe `unwrap()` calls replaced with safe alternatives
- `model_supports_tool_use` defaults to `false` (unknown models don't assume tool support)
- Full test coverage: wasm_bridge, parse_hook_mode, gateway TCP integration, full-loop agent with tool calls

### Production Readiness & Observability (Sprint 23)

- Real Prometheus metrics (counters, histograms, gauges) with request metrics middleware
- Dynamic `/v1/models` from provider catalog
- WebSocket hardening (heartbeat ping/pong, idle timeout, binary frame rejection)
- Structured error types (`GatewayError` with 8 variants, JSON error responses)
- Storage test expansion (19 → 46 tests), provider tracing spans, config audit
- Site documentation: gateway docs, architecture docs, threat model, provider guide

### Private AI Production-Readiness (Sprint 24)

- Gateway privacy wiring: NoiseSessionStore, RelayMailbox, key rotation task on startup
- Client-side Noise handshake (`NoiseClientHandshake`, `NoiseClientSession`, `NoiseHttpTransport`)
- `GET /v1/privacy/info` endpoint for capability discovery
- Security hardening: sealed envelope replay protection (nonce dedup, HTTP 409), local provider URL enforcement, network-level tool enforcement, plugin network isolation
- Per-component privacy boundaries (`PrivacyBoundary` enum with `resolve()`, agent/tool/channel boundaries)
- 6 Prometheus privacy metrics, E2E encryption integration tests
- Key rotation lifecycle (`force_rotate()`, `--force` CLI flag, persist on rotate)
- `Serialize` removed from `IdentityKeyPair` (prevent secret key leaks)

### Privacy End-to-End (Sprint 25)

- Memory privacy boundaries: `MemoryEntry` carries `privacy_boundary` and `source_channel`, `recent_for_boundary()` filters by boundary, SQLite schema migrated
- Channel privacy boundaries: `ChannelMessage.privacy_boundary`, `dispatch_with_boundary()` blocks `local_only` → non-local channels, per-channel boundary config
- Noise IK client handshake: 1 round-trip fast reconnect when server key is cached, `auto_noise_handshake()` selects IK vs XX
- `agentzero privacy test` command: 8 diagnostic checks (config, boundaries, memory, envelopes, Noise XX/IK, channels, encrypted store)
- Integration wiring: `ToolContext.privacy_boundary`, leak guard `check_boundary()`, config validation for encrypted-without-noise
- 1,724 tests passing, 0 clippy warnings

## Planned

### Near-Term

- Conversation branching and forking
- Multi-modal input (image, audio) across all providers
- Plugin registry and marketplace
- Enhanced RAG with vector search

### Medium-Term

- iOS XCFramework packaging for Swift FFI
- Android AAR packaging for Kotlin FFI
- Agent-to-agent collaboration protocols
- Cost tracking dashboard

### Long-Term

- Distributed agent coordination
- Self-hosted model fine-tuning integration
- Enterprise audit and compliance features

## Work Rules

- Add one capability per PR
- Every feature needs: tests, docs, and one explicit non-goal
- All tools must implement `input_schema()` for structured tool-use compatibility
