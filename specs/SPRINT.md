# AgentZero Sprint Plan — Sprint 15: Reference Alignment

## Scope
Align AgentZero with reference semantics across config, security, agent loop, channels, and multi-agent coordination. Based on gap analysis (2026-02-28).

References:
- `specs/sprints/14-foundation-and-parity.md` (archived previous sprint)
- Gap analysis: `.claude/plans/glittery-churning-horizon.md`

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
1. Phase A (Config Alignment) — foundation for all other phases
2. Phase B (Security Hardening) — depends on A for config sections
3. Phase C (Agent Loop) — depends on A for config, B for security policy
4. Phase D (Channel Features) — depends on A for config, B for approval model
5. Phase E (Multi-Agent Stack) — deferred to `specs/sprints/backlog.md`

## Risks and Mitigations
- Risk: Config model breaking changes break existing onboarding flows.
  Mitigation: Keep serde aliases for old field names during transition.
- Risk: Security hardening introduces false-positive blocking.
  Mitigation: All new security features default to disabled/permissive.
- Risk: Multi-agent coordination complexity.
  Mitigation: Deferred to backlog; single-process delegation first.

---

## Phase A: Config Alignment (Foundation)

### A1. Provider Config
- [x] Rename `provider.kind` → `default_provider` (keep `kind` as serde alias)
- [x] Default provider: `openrouter` (not `openai`)
- [x] Default model: `anthropic/claude-sonnet-4-6`
- [x] Add `default_temperature: f64` (default 0.7)
- [x] Add `provider_api: Option<String>` (openai-chat-completions / openai-responses)
- [x] Add `model_support_vision: Option<bool>`

### A2. Agent Settings
- [x] `max_tool_iterations`: 4 → 20
- [x] Rename `memory_window_size` → `max_history_messages` (keep alias), default 8 → 50
- [x] Add `parallel_tools: bool` (default false)
- [x] Add `tool_dispatcher: String` (default "auto")
- [x] Add `compact_context: bool` (default true)
- [x] Add loop detection config: `loop_detection_no_progress_threshold` (3), `loop_detection_ping_pong_cycles` (2), `loop_detection_failure_streak` (3)

### A3. Missing Config Sections
- [x] `[observability]` — backend, otel_endpoint, otel_service_name, runtime_trace_mode/path/max_entries
- [x] `[research]` — enabled, trigger, keywords, min_message_length, max_iterations, show_progress
- [x] `[runtime]` — kind (native/docker/wasm), reasoning_enabled
- [x] `[runtime.wasm]` — tools_dir, fuel_limit, memory_limit_mb, max_module_size_mb, security sub-section
- [x] `[browser]` — enabled, allowed_domains, backend, session_name, computer_use sub-section
- [x] `[http_request]` — enabled, allowed_domains, max_response_size, timeout_secs, credential_profiles
- [x] `[web_fetch]` — enabled, provider, api_key, allowed/blocked_domains, max_response_size
- [x] `[web_search]` — enabled, provider, fallback_providers, retries_per_provider, api keys (brave/perplexity/exa/jina), max_results
- [x] `[composio]` — enabled, api_key, entity_id
- [x] `[cost]` — enabled, daily_limit_usd, monthly_limit_usd, warn_at_percent, allow_override
- [x] `[identity]` — format (openclaw/aieos), aieos_path, aieos_inline
- [x] `[multimodal]` — max_images, max_image_size_mb, allow_remote_fetch
- [x] `[skills]` — open_skills_enabled, open_skills_dir, prompt_injection_mode, clawhub_token
- [x] `[provider]` — reasoning_level, transport
- [x] `[gateway]` — require_pairing, allow_public_bind
- [x] `[channels_config]` — message_timeout_secs

### A4. Model Provider Profiles
- [x] `[model_providers.<profile>]` with name, base_url, wire_api, model, api_key, requires_openai_auth

### A5. Model/Embedding Routes and Query Classification
- [x] `[[model_routes]]` — hint, provider, model, max_tokens, api_key, transport
- [x] `[[embedding_routes]]` — hint, provider, model, dimensions, api_key
- [x] `[query_classification]` — enabled, rules with hint/keywords/patterns/min_length/max_length/priority

### A6. Delegate Sub-Agent Config
- [x] `[agents.<name>]` — provider, model, system_prompt, api_key, temperature, max_depth, agentic, allowed_tools, max_iterations

### A7. Config CLI Commands
- [x] `config show` — print effective config as JSON with secrets masked
- [x] `config get <key>` — dot-path query
- [x] `config set <key> <value>` — atomic update to config.toml with type inference
- [x] Add `providers-quota` top-level command

### A-Acceptance
- [x] All new config sections parse from TOML correctly
- [x] Serde aliases preserve backward compatibility with old field names
- [x] Validation catches invalid values with actionable error messages
- [x] `cargo test --workspace` passes with new config model

---

## Phase B: Security Hardening

### B1. Autonomy Model
- [x] Create `crates/agentzero-autonomy/`
- [x] Add `[autonomy]` config section: level (read_only/supervised/full), workspace_only, forbidden_paths, allowed_roots
- [x] Add auto_approve, always_ask tool lists
- [x] Add allow_sensitive_file_reads/writes
- [x] Add non_cli_excluded_tools, non_cli_approval_approvers
- [x] Add non_cli_natural_language_approval_mode (direct/request_confirm/disabled)
- [x] Add non_cli_natural_language_approval_mode_by_channel

### B2. URL Access Policy
- [x] Add `[security.url_access]` config section
- [x] Implement block_private_ip, allow_cidrs, allow_domains, allow_loopback (`url_policy.rs`)
- [x] Implement domain_allowlist, domain_blocklist, approved_domains
- [x] Implement require_first_visit_approval, enforce_domain_allowlist
- [x] Add DNS rebinding protection (resolve domain → check resolved IPs for private ranges)
- [x] Wire into web_fetch, http_request, url_validation tools via shared `UrlAccessPolicy`
- [x] Add CIDR range parsing and matching (IPv4 + IPv6)
- [x] Add IPv4-mapped IPv6 private IP detection (`::ffff:192.168.x.x`)
- [x] Wire config → policy in `load_tool_security_policy()` (policy.rs)

### B3. OTP Gating
- [x] Add `[security.otp]` config section
- [x] Implement TOTP generation and validation (RFC 6238 / RFC 4226, HMAC-SHA1)
- [x] Implement gated_actions, gated_domains, gated_domain_categories (`OtpGate` engine)
- [x] Implement approval caching with TTL
- [x] Integrate with estop resume flow (--require-otp flag, encrypted OTP secret provisioning, validation on resume)

### B4. Outbound Leak Guard
- [x] Create `crates/agentzero-leak-guard/`
- [x] Add `[security.outbound_leak_guard]` config section
- [x] Implement credential leak scanning (API keys, JWTs, private keys, high-entropy tokens)
- [x] Implement action modes: redact vs block
- [x] Wire into channel response pipeline (`outbound.rs` — `process_outbound()`)

### B5. Perplexity Filter
- [x] Add `[security.perplexity_filter]` config section
- [x] Implement character-class bigram perplexity scoring (`perplexity.rs`)
- [x] Implement suffix window analysis before provider calls (`analyze_suffix()`)
- [x] Implement symbol ratio threshold detection
- [x] Add to channel message pipeline (`pipeline.rs` — `check_perplexity()` in dispatch loop)
- [x] Add to gateway handlers (api_chat, legacy_webhook, v1_chat_completions)
- [x] Shared `PerplexityFilterSettings` config struct with `PipelineConfig` and `GatewayState`

### B6. Syscall Anomaly Detection
- [x] Add `[security.syscall_anomaly]` config section
- [x] Implement `SyscallAnomalyDetector` with stateful rate-limiting windows
- [x] Implement baseline syscall profile matching (known vs unknown syscalls)
- [x] Implement anomaly detection from command output (strace, audit/seccomp log parsing)
- [x] Add alert budget and cooldown (`max_alerts_per_minute`, `alert_cooldown_secs`)
- [x] Parse strace-style (`syscall(args) = result`) and audit-style (`syscall=NAME/NUMBER`) lines
- [x] Map well-known Linux x86_64 syscall numbers to names
- [x] Denied event rate limiting (`max_denied_events_per_minute`)
- [x] 16 tests covering parsing, detection, rate limiting, budget exhaustion, and reset

### B7. File Tool Hardening
- [x] Add hard-link guard (refuse multiply-linked files) — in `agentzero-autonomy`
- [x] Add sensitive file detection (.env, .aws/credentials, private keys, etc.) — in `agentzero-autonomy`
- [x] Implement quote-aware shell separator parsing in ShellTool
- [x] Replace simple forbidden_chars with context-aware policy

### B-Acceptance
- [x] URL access policy blocks private IPs by default
- [x] Leak guard detects and redacts API key patterns in channel output
- [x] Leak guard wired into channel outbound pipeline
- [x] OTP gating validates TOTP codes with RFC 4226 test vectors
- [x] OTP integrated with estop resume (--require-otp engage, --otp code resume)
- [x] Perplexity filter flags adversarial suffixes
- [x] Perplexity filter wired into channel pipeline and gateway handlers
- [x] Syscall anomaly detector parses strace/audit output, enforces alert budget
- [x] Sensitive file reads blocked by default
- [x] All security features disabled by default (opt-in)
- [x] Negative tests for each security feature
- [x] IPC message store migrated to encrypted storage

---

## Phase C: Agent Loop Features

### C1. Loop Detection
- [x] Implement no-progress detector (same tool+args+output N times)
- [x] Implement ping-pong detector (A→B→A→B alternation)
- [x] Implement failure streak detector (same tool consecutive failures)
- [ ] Add self-correction prompt injection before hard stop
- [x] Wire to agent config thresholds (0 = disabled)

### C2. Research Phase
- [ ] Add research phase to agent loop (runs before main turn)
- [ ] Implement trigger strategies: never, always, keywords, length, question
- [ ] Separate tool budget from main agent turn
- [ ] Add progress reporting

### C3. Parallel Tool Execution
- [ ] When `parallel_tools = true`, execute independent tool calls concurrently
- [ ] Maintain stable result ordering
- [ ] Respect approval gating (sequential for gated tools)

### C4. Sub-Agent Delegation
- [x] Create `crates/agentzero-delegation/`
- [ ] Implement `delegate` tool
- [ ] Resolve sub-agent config from `[agents.<name>]`
- [x] Enforce max_depth recursion guard
- [x] Exclude `delegate` from sub-agent tool allowlists
- [ ] Support agentic mode (multi-turn tool loop) vs single prompt→response

### C5. Model Routing
- [x] Create `crates/agentzero-routing/`
- [x] Implement hint resolution from `[[model_routes]]`
- [x] Implement query classification engine from `[query_classification]` rules
- [ ] Implement `model_routing_config` runtime tool
- [x] Implement `[[embedding_routes]]` resolution

### C6. Reasoning Control
- [ ] Wire reasoning_level through to provider calls
- [ ] Provider-specific mapping: Ollama (think field), OpenAI Codex (effort param)
- [ ] Wire reasoning_enabled as global override

### C-Acceptance
- [x] Loop detection engages on repeated tool calls (test with mock)
- [ ] Research phase runs before main turn when triggered
- [ ] Parallel tool execution produces correct results
- [x] Sub-agent delegation respects max_depth
- [x] Model routing resolves hints correctly

---

## Phase D: Channel Features

### D0. Channel Infrastructure (Completed)
Foundation for all channel features — async trait, 5 core channel implementations, macro system, crypto crate, persistent queue, and message pipeline.

- [x] Created `crates/agentzero-crypto` crate (XChaCha20Poly1305 encryption, key management, SHA-256 hashing)
- [x] Extracted encryption from `agentzero-storage` into dedicated crypto crate (MIT-compatible RustCrypto)
- [x] Created `EncryptedQueue` in `agentzero-storage` (directory-backed persistent queue, one encrypted file per item)
- [x] Replaced sync `ChannelHandler` trait with async `Channel` trait (`send`, `listen`, `health_check`, typing, drafts, reactions)
- [x] Added `ChannelMessage` (inbound) and `SendMessage` (outbound) message types
- [x] Added `ChannelRegistry` with async `dispatch()` for gateway webhook routing
- [x] Created 3-macro system for easy channel addition:
  - `channel_stub!` — one-liner for unimplemented channels (struct + descriptor + bail impl)
  - `channel_meta!` — descriptor const for implemented channels
  - `channel_catalog!` — auto-wires module tree, re-exports, and catalog array
- [x] Added shared `helpers.rs` (message IDs, timestamps, user allowlist, message splitting)
- [x] Implemented CLI channel (async stdin reader, stdout sender)
- [x] Implemented Webhook channel (passive receiver with `inject_message()` from gateway)
- [x] Implemented Telegram channel (long-polling `getUpdates`, feature-gated: `channel-telegram`)
- [x] Implemented Discord channel (WebSocket Gateway v10, heartbeat, feature-gated: `channel-discord`)
- [x] Implemented Slack channel (Socket Mode + HTTP polling fallback, feature-gated: `channel-slack`)
- [x] All 16 stub channels updated to `channel_stub!` macro invocations
- [x] Created message processing pipeline (`pipeline.rs`) with supervised listeners, semaphore-bounded concurrency
- [x] Updated gateway webhook handler to use async `dispatch().await`
- [x] Added tokio features: `sync`, `io-util`, `io-std` to workspace
- [x] Added tokio-tungstenite `connect` feature for WebSocket channels
- [x] 81 channel tests + 8 gateway tests passing

### D1. Group Reply Policy
- [x] Add `[channels_config.<channel>.group_reply]` config structure (`GroupReplyConfig` in model.rs)
- [x] Implement mode: all_messages / mention_only (`group_reply.rs` — `GroupReplyFilter`)
- [x] Implement allowed_sender_ids bypass
- [x] Wire into channel receive path (`group_reply::GroupReplyFilter::should_process()`)

### D2. In-Chat Runtime Commands
- [x] Implement command interceptor before LLM inference (`commands.rs` — `intercept_command()`)
- [x] `/models`, `/models <provider>` — show/switch providers
- [x] `/model`, `/model <id>` — show/switch model (per-sender session)
- [x] `/new` — clear sender conversation history
- [x] `/approve-request <tool>`, `/approve-confirm <id>`, `/approve-pending`
- [x] `/approve <tool>`, `/unapprove <tool>`, `/approvals`
- [ ] Persist approvals to autonomy.auto_approve

### D3. Streaming and Drafts
- [x] Add stream_mode config (off/partial) — in `ChannelsGlobalConfig`
- [x] Add draft_update_interval_ms throttle — in `ChannelsGlobalConfig`
- [ ] Implement draft lifecycle: send_draft → update_draft → finalize_draft

### D4. Message Interruption
- [x] Add interrupt_on_new_message config flag — in `ChannelsGlobalConfig`
- [ ] Implement same-sender same-chat cancellation scope
- [ ] Preserve interrupted turn in conversation history

### D5. ACK Reactions
- [x] Add `[channels_config.ack_reaction.<channel>]` config (`AckReactionConfig` in model.rs)
- [x] Implement emoji pools, strategies (random/first), sample rates (`ack_reactions.rs` — `AckReactionEngine`)
- [x] Implement conditional rules (contains_any/all/none, regex, sender/chat/locale filters)

### D6. Hot Config Reload
- [ ] Watch config.toml during daemon/channel runtime
- [ ] Hot-apply: default_provider, default_model, default_temperature, api_key
- [ ] Hot-apply: reliability settings

### D7. Multimodal Image Markers
- [x] Implement `[IMAGE:<source>]` parser for user messages (`image_markers.rs`)
- [x] Support local file paths and data URIs
- [x] Support remote URL fetch when allow_remote_fetch = true
- [x] Add validation (max_images, allow_remote_fetch policy)
- [ ] Add provider vision capability check (fail with structured error)

### D8. Catalog Additions
- [x] Add Napcat (QQ via OneBot) to channel catalog
- [x] Add ACP (Agent Client Protocol) to channel catalog

### D-Acceptance
- [x] Group reply respects mention_only mode
- [x] In-chat commands execute before LLM inference
- [x] ACK reactions fire on configured channels
- [ ] Hot config reload updates provider settings without restart
- [x] Image markers parse correctly

---

## Phase E: Multi-Agent Stack — DEFERRED
See `specs/sprints/backlog.md` for full details. Deferred to avoid distributed-systems complexity until there's a concrete scaling need.

---

## New Crates to Create

| Crate | Purpose | Phase |
|---|---|---|
| `crates/agentzero-crypto/` | XChaCha20Poly1305 encryption, key management, SHA-256 | D0 (done) |
| `crates/agentzero-autonomy/` | Autonomy levels, approval state machine, tool gating | B1 (done) |
| `crates/agentzero-delegation/` | Sub-agent delegation with agentic mode | C4 (done) |
| `crates/agentzero-leak-guard/` | Outbound credential leak detection | B4 (done) |
| `crates/agentzero-routing/` | Model routing, query classification, embedding routes | C5 (done) |

## Critical Files to Modify

| File | Phase | Changes |
|---|---|---|
| `crates/agentzero-config/src/model.rs` | A | Add ~15 config sections, rename fields, update defaults |
| `crates/agentzero-core/src/types.rs` | A, C | Loop detection config, parallel_tools, research config |
| `crates/agentzero-core/src/agent.rs` | C | Loop detection, research phase, parallel tool dispatch |
| `crates/agentzero-channels/src/pipeline.rs` | D | In-chat commands, hot reload, ACK reactions |
| `crates/agentzero-channels/src/lib.rs` | D | Group reply policy, streaming config, image markers |
| `crates/agentzero-tools/src/url_validation.rs` | B | URL access policy enforcement |
| `crates/agentzero-tools/src/lib.rs` | B | File tool hardening, sensitive file detection |
| `crates/agentzero-cli/src/commands/config.rs` | A | Add show/get/set subcommands |
| `crates/agentzero-config/src/tests.rs` | A, B | Tests for all new config sections |
| `Cargo.toml` (workspace) | B, C | Add new crate members |

## Definition of Done (All Phases)
- Code compiles and tests pass locally.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` pass.
- New config sections have deserialization tests.
- Security features default to disabled (opt-in).
- Serde aliases preserve backward compatibility.
- Feature has at least one negative-path test.
