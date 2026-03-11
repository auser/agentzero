# AgentZero Feature Validation Plan

> Comprehensive checklist of automated and manual tests required to validate every feature.
> **Status legend**: `[A]` = automated, `[M]` = manual, `[P]` = partially automated, `[—]` = missing

---

## 1. Provider & Inference

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 1.1 | OpenRouter provider | M | `agentzero agent -m "hello" --provider openrouter` | [—] |
| 1.2 | OpenAI provider | M | `agentzero agent -m "hello" --provider openai` | [—] |
| 1.3 | Anthropic provider | M | `agentzero agent -m "hello" --provider anthropic` | [—] |
| 1.4 | Ollama (local) provider | M | `agentzero agent -m "hello" --provider ollama` (requires `ollama serve`) | [—] |
| 1.5 | Streaming output (SSE) | M | `agentzero agent -m "tell me a story" -vvv` — verify token-by-token output | [—] |
| 1.6 | Temperature/top_p tuning | A | Unit test: config round-trip for `temperature`, `top_p`, `max_tokens` | [A] |
| 1.7 | Model routing (hints) | A | Unit test: route selection by hint keywords → correct provider+model | [P] |
| 1.8 | Provider fallback chain | A | Unit test: primary fails → fallback provider used | [—] |
| 1.9 | Reasoning mode (low/med/high) | M | `agentzero agent -m "..." --reasoning high` — verify extended thinking | [—] |
| 1.10 | Request timeout | A | Mock provider with delay > timeout → verify timeout error | [—] |

### Automated test strategy

```rust
// providers/mock.rs — fake provider for offline tests
// Returns canned responses, controllable latency, error injection
// Used by: 1.7, 1.8, 1.10
```

---

## 2. Authentication & Profiles

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 2.1 | `auth setup-token` | A | Integration test: store + retrieve token | [A] |
| 2.2 | `auth login` (OAuth flow) | M | Browser-based login, verify token stored | [—] |
| 2.3 | `auth list` | A | Integration test: list profiles after setup | [A] |
| 2.4 | `auth status` | A | Integration test: active profile display | [A] |
| 2.5 | `auth use` (switch profile) | A | Integration test: switch active, verify agent uses it | [—] |
| 2.6 | `auth logout` | A | Integration test: remove profile, verify gone | [—] |
| 2.7 | `auth refresh` | P | Mock OAuth endpoint, verify refresh token exchange | [—] |
| 2.8 | Token expiry detection | A | Set expired token, verify re-auth prompt | [—] |
| 2.9 | Env var fallback (`OPENAI_API_KEY`) | A | No profile, env var set → provider resolves key | [A] |
| 2.10 | `.env` / `.env.local` loading | A | Config test: `.env` in CWD overrides `~/.agentzero/.env` | [—] |
| 2.11 | Profile-per-provider isolation | A | Two profiles (openai, anthropic), each uses correct key | [—] |

---

## 3. Configuration

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 3.1 | `config show` | A | Integration test: parse + display effective config | [A] |
| 3.2 | `config get <key>` | A | Integration test: `config get provider.model` → correct value | [A] |
| 3.3 | `config set <key> <val>` | A | Integration test: set + get round-trip | [A] |
| 3.4 | `config schema` | A | Integration test: valid JSON schema output | [A] |
| 3.5 | `config export` | A | Integration test: export → reimport yields same config | [—] |
| 3.6 | Config validation errors | A | Unit test: missing `[provider]` → clear error | [A] |
| 3.7 | TOML → dotenv → env layering | A | Unit test: env var overrides dotenv overrides TOML | [P] |
| 3.8 | `onboard --interactive` | M | Walk through wizard, verify generated TOML | [—] |
| 3.9 | `onboard --yes` (scripted) | A | Integration test: non-interactive onboard | [A] |
| 3.10 | Config hot-reload | M | Edit TOML while gateway running → verify change takes effect | [—] |

---

## 4. Security

### 4.1 File Security

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 4.1.1 | Path traversal (`../`) blocked | A | `read_file("../../etc/passwd")` → denied | [A] |
| 4.1.2 | Absolute path blocked | A | `read_file("/etc/passwd")` → denied | [A] |
| 4.1.3 | Symlink resolution | A | Symlink pointing outside workspace → denied | [A] |
| 4.1.4 | Hard-link detection (B7) | A | Hard-link to sensitive file → denied | [A] |
| 4.1.5 | Sensitive file blocking | A | `.env`, `.aws/credentials`, `.ssh/id_rsa` → denied | [A] |
| 4.1.6 | Binary file detection | A | Null bytes in file → warning/denied | [A] |
| 4.1.7 | Size cap enforcement | A | File > 64 KiB → truncated or denied | [A] |
| 4.1.8 | Write dry-run mode | A | `write_file` with dry_run → no disk write | [P] |
| 4.1.9 | `allowed_root` scoping | A | File outside allowed_root → denied | [A] |

### 4.2 Shell Security

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 4.2.1 | Command allowlist | A | `rm -rf /` → denied (not in allowlist) | [A] |
| 4.2.2 | Metachar blocking (`;`, `&`, `\|`) | A | `ls; rm -rf /` → denied | [A] |
| 4.2.3 | Backtick always forbidden | A | `` `cmd` `` → denied | [A] |
| 4.2.4 | Null byte blocking | A | `ls\x00-la` → denied | [A] |
| 4.2.5 | Argument count limit (8) | A | 9+ args → denied | [A] |
| 4.2.6 | Argument length limit (128B) | A | 200-char argument → denied | [A] |
| 4.2.7 | Output truncation (8 KiB) | A | Long output → truncated | [A] |
| 4.2.8 | Quote-aware parsing | A | `echo "a;b"` → allowed (quoted) | [A] |

### 4.3 URL & Network Security

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 4.3.1 | Private IP blocking | A | `http://192.168.1.1` → denied | [A] |
| 4.3.2 | Loopback configurable | A | `http://127.0.0.1` with loopback=true → allowed | [A] |
| 4.3.3 | Domain allowlist/blocklist | A | Blocked domain → denied, allowed → permitted | [A] |
| 4.3.4 | DNS rebinding protection | A | Resolved IP in private range → denied | [—] |
| 4.3.5 | SSRF prevention | A | Internal metadata endpoints → denied | [—] |

### 4.4 OTP Gate

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 4.4.1 | TOTP generation (RFC 6238) | A | Generate + validate within window | [A] |
| 4.4.2 | Clock skew tolerance | A | Token from adjacent window → accepted | [A] |
| 4.4.3 | Expired token rejection | A | Token from 5 windows ago → denied | [A] |
| 4.4.4 | Gated tool enforcement | A | Shell without OTP → prompted; with OTP → executed | [P] |

### 4.5 Adversarial Input Detection

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 4.5.1 | Perplexity scoring | A | Normal prompt → low score; adversarial suffix → high score | [A] |
| 4.5.2 | Symbol ratio check | A | High symbol ratio → flagged | [A] |
| 4.5.3 | Short prompt bypass | A | < 32 chars → skip check | [A] |

### 4.6 Redaction

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 4.6.1 | API key redaction | A | `sk-abc123` in output → `sk-***` | [A] |
| 4.6.2 | Bearer token masking | A | `Authorization: Bearer xxx` → masked | [A] |
| 4.6.3 | JSON credential scrubbing | A | `{"api_key":"secret"}` → scrubbed | [A] |
| 4.6.4 | Error chain redaction | A | Error containing key → redacted before display | [A] |

### 4.7 Audit Trail

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 4.7.1 | Tool execution logged | A | Execute tool → AuditEvent recorded | [P] |
| 4.7.2 | Risk domain captured | A | Event includes correct RiskDomain | [—] |
| 4.7.3 | Timestamp accuracy | A | Event timestamp within 1s of wall clock | [—] |

---

## 5. Storage & Memory

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 5.1 | SQLite memory store/recall | A | Store key → recall → match | [A] |
| 5.2 | Memory forget | A | Store → forget → recall returns empty | [A] |
| 5.3 | Conversation scoping | A | Store in conv A → recall in conv B → empty | [A] |
| 5.4 | Privacy boundary columns | A | Store with boundary → query filters by boundary | [A] |
| 5.5 | TTL expiry | A | Store with TTL=1s → wait 2s → recall returns empty | [—] |
| 5.6 | Encrypted storage (SQLCipher) | A | Open with key → read; open without → fail | [A] |
| 5.7 | Plaintext → encrypted migration | A | Create plaintext DB → enable encryption → auto-migrates | [A] |
| 5.8 | Wrong-key detection | A | Encrypted DB + wrong key → error (not delete) | [A] |
| 5.9 | Schema migration (forward) | A | Old schema → open → new columns exist | [A] |
| 5.10 | Memory list / stats | A | Integration test: `memory list`, `memory stats` | [P] |
| 5.11 | Turso backend | M | Configure Turso URL → store/recall works | [—] |
| 5.12 | Encrypted JSON store | A | EncryptedJsonStore: write → read → decrypt matches | [A] |
| 5.13 | Encrypted queue | A | EncryptedQueue: enqueue → dequeue → decrypt matches | [A] |
| 5.14 | Connection pooling (R2D2) | A | Concurrent access with pool → no deadlock | [—] |

---

## 6. Tools

### 6.1 File I/O Tools

| # | Tool | Type | How to validate | Status |
|---|------|------|-----------------|--------|
| 6.1.1 | `read_file` | A | Read known file → content matches | [A] |
| 6.1.2 | `write_file` | A | Write → read back → matches | [A] |
| 6.1.3 | `file_edit` (apply patch) | A | Original + patch → expected result | [A] |
| 6.1.4 | `apply_patch` | A | Unified diff → applied correctly | [A] |
| 6.1.5 | `glob_search` | A | Pattern `**/*.rs` → finds Rust files | [A] |
| 6.1.6 | `content_search` | A | Search for known string → found with line number | [A] |

### 6.2 Document Tools

| # | Tool | Type | How to validate | Status |
|---|------|------|-----------------|--------|
| 6.2.1 | `pdf_read` | A | Read test PDF → text extracted | [P] |
| 6.2.2 | `docx_read` | A | Read test DOCX → text extracted | [P] |
| 6.2.3 | `html_extract` | A | HTML input → structured text output | [P] |
| 6.2.4 | `image_info` | A | Image file → dimensions, format, size | [P] |

### 6.3 Network Tools

| # | Tool | Type | How to validate | Status |
|---|------|------|-----------------|--------|
| 6.3.1 | `web_search` (DuckDuckGo) | M | Search query → results returned | [—] |
| 6.3.2 | `web_search` (Brave) | M | Search query → results returned (needs BRAVE_API_KEY) | [—] |
| 6.3.3 | `web_fetch` | P | Fetch known URL → content returned; mock for CI | [—] |
| 6.3.4 | `http_request` | P | GET/POST to mock server → correct response | [—] |
| 6.3.5 | `url_validation` | A | Valid URL → OK; invalid → error | [—] |

### 6.4 Browser Tools

| # | Tool | Type | How to validate | Status |
|---|------|------|-----------------|--------|
| 6.4.1 | `browser` (headless navigate) | M | Navigate to URL → page content returned | [—] |
| 6.4.2 | `browser_open` (interactive) | M | Open URL in system browser | [—] |
| 6.4.3 | Screenshot capture | M | Navigate → screenshot → image file exists | [—] |

### 6.5 Git Tools

| # | Tool | Type | How to validate | Status |
|---|------|------|-----------------|--------|
| 6.5.1 | `git_operations` (status) | A | In git repo → status output | [P] |
| 6.5.2 | `git_operations` (diff) | A | Modified file → diff output | [P] |
| 6.5.3 | `git_operations` (log) | A | Commits exist → log output | [P] |
| 6.5.4 | `git_operations` (commit) | A | Stage + commit → new commit hash | [—] |

### 6.6 Memory Tools

| # | Tool | Type | How to validate | Status |
|---|------|------|-----------------|--------|
| 6.6.1 | `memory_store` | A | Store fact → success | [A] |
| 6.6.2 | `memory_recall` | A | Store → recall → fact returned | [A] |
| 6.6.3 | `memory_forget` | A | Store → forget → gone | [A] |

### 6.7 Process & Shell Tools

| # | Tool | Type | How to validate | Status |
|---|------|------|-----------------|--------|
| 6.7.1 | `shell` (allowed command) | A | `echo hello` → "hello" | [A] |
| 6.7.2 | `shell` (blocked command) | A | `rm -rf /` → denied | [A] |
| 6.7.3 | `process_tool` | A | List processes → output | [P] |
| 6.7.4 | `screenshot` | M | Capture screen → image returned | [—] |

### 6.8 Cron Tools

| # | Tool | Type | How to validate | Status |
|---|------|------|-----------------|--------|
| 6.8.1 | `cron_add` | A | Add job → listed | [A] |
| 6.8.2 | `cron_list` | A | List jobs → includes added job | [A] |
| 6.8.3 | `cron_remove` | A | Remove job → no longer listed | [A] |
| 6.8.4 | `cron_update` | A | Update schedule → new schedule reflected | [A] |
| 6.8.5 | `cron_pause` / `cron_resume` | A | Pause → status=paused; resume → status=active | [A] |
| 6.8.6 | Cron execution | P | Add job with `* * * * *` → verify it fires within 60s | [—] |

### 6.9 Sub-Agent Tools

| # | Tool | Type | How to validate | Status |
|---|------|------|-----------------|--------|
| 6.9.1 | `subagent_spawn` | A | Spawn → run_id returned | [P] |
| 6.9.2 | `subagent_list` | A | Spawn → list → includes spawned agent | [P] |
| 6.9.3 | `subagent_manage` (cancel) | A | Spawn → cancel → status=Cancelled | [P] |
| 6.9.4 | Depth limit enforcement | A | Spawn at max_depth → denied | [—] |

### 6.10 SOP Tools

| # | Tool | Type | How to validate | Status |
|---|------|------|-----------------|--------|
| 6.10.1 | `sop_list` | A | List available SOPs | [—] |
| 6.10.2 | `sop_execute` | A | Start SOP → first step active | [—] |
| 6.10.3 | `sop_advance` | A | Advance → next step active | [—] |
| 6.10.4 | `sop_approve` | A | Approve gated step → proceeds | [—] |
| 6.10.5 | `sop_status` | A | Status → current step + progress | [—] |

### 6.11 MCP Tools

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 6.11.1 | Server discovery (mcp.json) | A | Parse mcp.json → server entries loaded | [A] |
| 6.11.2 | Tool registration (`mcp__{s}__{t}`) | A | Discover → tools named correctly | [A] |
| 6.11.3 | Schema passthrough | A | Tool schema matches MCP `tools/list` response | [A] |
| 6.11.4 | Shared session (Arc) | A | Two tools from same server → same connection | [A] |
| 6.11.5 | Graceful degradation | A | Server fails to connect → others still register | [A] |
| 6.11.6 | Global + project merge | A | Global + project mcp.json → merged, project wins | [A] |
| 6.11.7 | Env var override | A | `AGENTZERO_MCP_SERVERS` overrides file-based | [A] |
| 6.11.8 | `allowed_servers` filter | A | Filter → only named servers pass through | [A] |
| 6.11.9 | Tool execution (real server) | M | `npx @anthropic/mcp-server-filesystem` → `mcp__filesystem__read_file` works | [—] |
| 6.11.10 | Reconnect on failure | A | Connection drops → next call reconnects | [—] |

### 6.12 Unchecked / Low-Coverage Tools

| # | Tool | Status | Notes |
|---|------|--------|-------|
| 6.12.1 | `cli_discovery` | [—] | No dedicated tests |
| 6.12.2 | `composio` | [—] | Requires external API |
| 6.12.3 | `delegate_coordination_status` | [—] | No dedicated tests |
| 6.12.4 | `hardware_board_info` | [—] | Platform-specific |
| 6.12.5 | `hardware_memory_map` | [—] | Platform-specific |
| 6.12.6 | `hardware_memory_read` | [—] | Platform-specific |
| 6.12.7 | `model_routing_config` | [—] | No dedicated tests |
| 6.12.8 | `proxy_config` | [—] | No dedicated tests |
| 6.12.9 | `pushover` | [—] | Requires external API |
| 6.12.10 | `schedule` | [—] | No dedicated tests |
| 6.12.11 | `task_plan` | [—] | No dedicated tests |
| 6.12.12 | `wasm_module` | [P] | Partial via plugin tests |
| 6.12.13 | `wasm_tool_exec` | [P] | Partial via plugin tests |
| 6.12.14 | `agents_ipc` | [—] | No dedicated tests |

---

## 7. Multi-Agent Orchestration

### 7.1 Delegation

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 7.1.1 | `delegate` tool appears | A | Configure `[agents.x]` → delegate tool in list | [A] |
| 7.1.2 | Sub-agent uses correct provider | A | Delegate to agent with openai → openai provider used | [P] |
| 7.1.3 | Tool allowlist enforced | A | Sub-agent can't use tools not in its `allowed_tools` | [A] |
| 7.1.4 | `max_depth` limit | A | At max_depth → delegation refused | [A] |
| 7.1.5 | Workspace root inherited | A | Sub-agent has same workspace_root as parent | [A] |
| 7.1.6 | End-to-end delegation | M | Primary delegates to coder → coder executes → result returned | [—] |

### 7.2 Swarm Coordination

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 7.2.1 | AI router classification | P | Message → router classifies → correct agent | [P] |
| 7.2.2 | Keyword fallback routing | A | AI fails → keyword match → correct agent | [A] |
| 7.2.3 | Event bus pub/sub | A | Publish event → subscriber receives | [A] |
| 7.2.4 | Topic wildcard matching | A | Subscribe `task.*.complete` → matches `task.image.complete` | [A] |
| 7.2.5 | Event bus capacity/lag | A | Overflow → lagged subscribers skip missed | [A] |
| 7.2.6 | Shutdown grace period | A | In-flight task → waits up to `shutdown_grace_ms` | [—] |

### 7.3 Pipelines

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 7.3.1 | Sequential step execution | A | Steps [A, B, C] → A.output → B.input → C.input | [P] |
| 7.3.2 | Error mode: abort | A | Step B fails → pipeline stops, no step C | [P] |
| 7.3.3 | Error mode: skip | A | Step B fails → step C gets step A's output | [—] |
| 7.3.4 | Error mode: retry | A | Step B fails → retried up to max_retries | [—] |
| 7.3.5 | Step timeout | A | Step exceeds `step_timeout_secs` → timeout error | [—] |
| 7.3.6 | Channel reply | A | Pipeline completes → result sent to channel | [—] |
| 7.3.7 | Keyword trigger matching | A | Message matches trigger keywords → pipeline starts | [P] |
| 7.3.8 | Regex trigger matching | A | Message matches regex → pipeline starts | [—] |

### 7.4 Async Jobs & Lanes

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 7.4.1 | RunId uniqueness | A | Generate 1000 RunIds → all unique | [A] |
| 7.4.2 | Job lifecycle (Pending→Running→Completed) | A | Submit → start → complete → correct status | [A] |
| 7.4.3 | Job cancellation | A | Running job → cancel → status=Cancelled | [A] |
| 7.4.4 | Lane: Main (serialized) | A | Two main-lane jobs → run sequentially | [—] |
| 7.4.5 | Lane: Cron (parallel) | A | Two cron jobs → run concurrently | [—] |
| 7.4.6 | Lane: SubAgent (depth tracking) | A | SubAgent lane → parent_run_id + depth set | [P] |

### 7.5 Loop Detection

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 7.5.1 | No-progress detection (3 identical) | A | 3 identical outputs → LoopAction triggered | [A] |
| 7.5.2 | Ping-pong detection (A↔B) | A | A→B→A pattern 2x → detected | [A] |
| 7.5.3 | Failure streak (3 consecutive) | A | 3 failures → LoopAction triggered | [A] |
| 7.5.4 | Action: InjectMessage | A | Loop detected → message injected into context | [P] |
| 7.5.5 | Action: RestrictTools | A | Loop detected → tool set reduced | [—] |
| 7.5.6 | Action: ForceComplete | A | Loop detected → agent forced to stop | [—] |

### 7.6 Queue Modes

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 7.6.1 | Steer (AI router) | P | Message → classified → routed to correct agent | [P] |
| 7.6.2 | Followup (append to run) | A | Followup message → appended to existing run | [—] |
| 7.6.3 | Collect (fan-out) | A | Message → sent to all agents → results merged | [—] |
| 7.6.4 | Interrupt (preempt) | A | Running agent → interrupt → preempted | [—] |

### 7.7 Merge Strategies

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 7.7.1 | WaitAll | A | 3 agents → wait for all 3 → merge | [—] |
| 7.7.2 | WaitAny | A | 3 agents → first completes → result returned | [—] |
| 7.7.3 | WaitQuorum | A | 3 agents, quorum=2 → 2 complete → merge | [—] |

---

## 8. Event Bus

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 8.1 | InMemoryBus pub/sub | A | Publish → subscriber receives | [A] |
| 8.2 | FileBackedBus persistence | A | Publish → restart → replay from file | [A] |
| 8.3 | Topic filtering | A | Subscribe to "a.b" → only "a.b" events received | [A] |
| 8.4 | Wildcard topics | A | Subscribe "a.*" → matches "a.b", "a.c" | [A] |
| 8.5 | Correlation ID chaining | A | Event A → triggers B → B.correlation_id = A.id | [—] |
| 8.6 | Privacy boundary inheritance | A | Event from `local_only` agent → boundary preserved | [—] |
| 8.7 | Lagged subscriber recovery | A | Slow consumer → skips missed, catches up | [A] |

---

## 9. Gateway API

| # | Endpoint | Type | How to validate | Status |
|---|----------|------|-----------------|--------|
| 9.1 | `GET /health` | A | Returns 200 + `{"status":"ok"}` | [A] |
| 9.2 | `GET /ready` | A | Returns 200 when all deps ready | [—] |
| 9.3 | `POST /pair` (success) | A | Correct pairing code → 200 + bearer token | [A] |
| 9.4 | `POST /pair` (wrong code) | A | Wrong pairing code → 403 | [A] |
| 9.5 | `POST /api/chat` (authed) | A | Valid bearer + message → 200 + response | [P] |
| 9.6 | `POST /api/chat` (unauthed) | A | No bearer → 401 | [A] |
| 9.7 | `POST /v1/chat/completions` | P | OpenAI-compatible request → valid response format | [—] |
| 9.8 | `GET /v1/models` | A | Returns model list | [—] |
| 9.9 | `GET /ws/chat` | M | WebSocket connection → streaming messages | [—] |
| 9.10 | `GET /metrics` | A | Prometheus-format metrics | [—] |
| 9.11 | `POST /v1/ping` | A | Returns pong | [—] |
| 9.12 | Job submission (async) | A | Submit → job_id returned → poll → result | [—] |
| 9.13 | Job cancellation | A | Submit → cancel → status=cancelled | [—] |
| 9.14 | Webhook delivery | P | POST to webhook endpoint → dispatched to channel | [—] |
| 9.15 | OTP enforcement | A | Gateway requires OTP → request without OTP → 403 | [—] |
| 9.16 | Perplexity filter | A | Adversarial prompt → rejected by gateway | [—] |

---

## 10. Channels

### 10.1 Channel Middleware

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 10.1.1 | Leak guard (redact mode) | A | Sensitive content → redacted in output | [A] |
| 10.1.2 | Leak guard (block mode) | A | Sensitive content → message blocked | [A] |
| 10.1.3 | Leak guard (warn mode) | A | Sensitive content → warning appended | [A] |
| 10.1.4 | Group reply (all messages) | A | Group message → agent responds | [A] |
| 10.1.5 | Group reply (mention only) | A | Non-mention → ignored; mention → responds | [A] |
| 10.1.6 | Ack reactions (random) | A | Message → random emoji from pool | [A] |
| 10.1.7 | Ack reactions (round robin) | A | Messages → emojis cycle through pool | [A] |
| 10.1.8 | Conditional reactions | A | "urgent" message → specific emoji | [A] |
| 10.1.9 | Image markers | A | Image in message → markers extracted | [A] |
| 10.1.10 | Draft updates | A | Streaming → draft sent at interval | [A] |
| 10.1.11 | Command parsing | A | `/help` → command recognized | [A] |
| 10.1.12 | Message interruption | A | New message during processing → previous cancelled | [—] |

### 10.2 Channel Integrations

Each channel requires a live service for full validation. Automated tests cover message parsing and config loading; live tests require credentials.

| # | Channel | Auto | Manual | Notes |
|---|---------|------|--------|-------|
| 10.2.1 | Telegram | [P] | [—] | Needs bot token, test group |
| 10.2.2 | Discord | [P] | [—] | Needs bot token, test guild |
| 10.2.3 | Slack | [P] | [—] | Needs bot+app tokens, test workspace |
| 10.2.4 | Matrix | [P] | [—] | Needs homeserver, test room |
| 10.2.5 | Mattermost | [P] | [—] | Needs instance URL, token |
| 10.2.6 | Email (SMTP/IMAP) | [P] | [—] | Needs mail server or Mailtrap |
| 10.2.7 | IRC | [P] | [—] | Needs IRC server, test channel |
| 10.2.8 | Nostr | [P] | [—] | Needs relay URL |
| 10.2.9 | Webhook | [A] | — | Can test with local HTTP server |
| 10.2.10 | Signal | [P] | [—] | Needs Signal CLI daemon |
| 10.2.11 | WhatsApp | [P] | [—] | Needs business API |
| 10.2.12 | iMessage | [P] | [—] | macOS only |
| 10.2.13 | Lark / Feishu | [P] | [—] | Needs app credentials |
| 10.2.14 | DingTalk | [P] | [—] | Needs app credentials |
| 10.2.15 | MQTT | [P] | [—] | Can test with local broker |

---

## 11. CLI Commands

| # | Command | Auto | Manual | Status |
|---|---------|------|--------|--------|
| 11.1 | `agent -m "..."` | A | — | [A] |
| 11.2 | `onboard` | A | M (interactive) | [A] |
| 11.3 | `config show/get/set/schema` | A | — | [A] |
| 11.4 | `auth setup-token/list/status` | A | — | [A] |
| 11.5 | `auth login` (OAuth) | — | M | [—] |
| 11.6 | `gateway` | — | M | [P] |
| 11.7 | `daemon start/stop/status` | — | M | [P] |
| 11.8 | `service install/start/stop` | — | M | [—] |
| 11.9 | `status` | A | — | [A] |
| 11.10 | `doctor models` | — | M | [—] |
| 11.11 | `doctor traces` | — | M | [—] |
| 11.12 | `cron list/add/remove/pause/resume` | A | — | [A] |
| 11.13 | `hooks list/enable/disable/test` | A | — | [A] |
| 11.14 | `memory list/stats` | A | — | [P] |
| 11.15 | `models list/refresh` | — | M | [—] |
| 11.16 | `providers` | A | — | [P] |
| 11.17 | `tools list` | A | — | [P] |
| 11.18 | `conversation list/show` | A | — | [P] |
| 11.19 | `completions --shell zsh/bash` | A | — | [A] |
| 11.20 | `skill list/install/test/remove` | A | — | [P] |
| 11.21 | `plugin list/install/remove` | A | — | [P] |
| 11.22 | `cost` | A | — | [—] |
| 11.23 | `estop` | — | M | [—] |
| 11.24 | `dashboard` | — | M | [—] |
| 11.25 | `tunnel` | — | M | [—] |
| 11.26 | `update check` | A | — | [P] |
| 11.27 | `rag index/query` | A | — | [—] |
| 11.28 | `channel list/enable/test` | A | M (live test) | [P] |
| 11.29 | `privacy` | A | — | [—] |
| 11.30 | `identity` | A | — | [—] |
| 11.31 | `goals` | A | — | [—] |
| 11.32 | `coordination` | A | — | [—] |
| 11.33 | `template` | A | — | [—] |
| 11.34 | `hardware` | — | M | [—] |
| 11.35 | `peripheral` | — | M | [—] |

---

## 12. Privacy & Encryption

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 12.1 | LocalOnly boundary enforcement | A | LocalOnly agent → network tools denied | [A] |
| 12.2 | EncryptedOnly boundary | A | Unencrypted transport → denied | [P] |
| 12.3 | Parent-child boundary clamping | A | Child more permissive than parent → clamped | [A] |
| 12.4 | Sealed envelope encrypt/decrypt | A | Seal → unseal → plaintext matches | [A] |
| 12.5 | Ephemeral sender keys | A | Each seal → different ephemeral key | [A] |
| 12.6 | Routing ID opacity | A | Routing ID reveals no plaintext info | [A] |
| 12.7 | TTL-based expiry | A | Seal with TTL → after TTL → unseal fails | [A] |
| 12.8 | Noise Protocol handshake | A | IK handshake → session established | [P] |
| 12.9 | Key rotation trigger | A | Staleness threshold → rotation event fired | [—] |

---

## 13. Plugins (WASM)

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 13.1 | Plugin load (wasmi) | A | Load .wasm → initialized | [A] |
| 13.2 | Plugin ABI v2 compliance | A | SDK integration tests | [A] |
| 13.3 | Plugin sandboxing | A | Plugin can't access host filesystem | [A] |
| 13.4 | Plugin hot-reload (`plugin-dev`) | M | Edit .wasm → auto-reload | [—] |
| 13.5 | Plugin integrity (hash check) | A | Tampered .wasm → load fails | [A] |
| 13.6 | WASI support | A | Plugin uses WASI calls → works | [A] |
| 13.7 | JIT (wasmtime) | A | Feature `wasm-jit` → faster execution | [—] |
| 13.8 | `plugin list/install/remove` CLI | A | CLI integration tests | [P] |

---

## 14. Autonomy Levels

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 14.1 | ReadOnly mode | A | Write tool → denied; read tool → allowed | [A] |
| 14.2 | Supervised mode | A | Write tool → prompt required; read → auto | [A] |
| 14.3 | Full autonomy | A | All tools auto-approved | [A] |
| 14.4 | Per-tool always_ask | A | Tool in always_ask list → always prompted | [A] |
| 14.5 | Forbidden paths | A | Path in forbidden_paths → access denied | [A] |
| 14.6 | Max actions per hour | A | Exceed limit → throttled | [—] |
| 14.7 | Max cost per day | A | Exceed limit → stopped | [—] |

---

## 15. Cost Tracking

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 15.1 | Token counting | A | Request → token count tracked | [—] |
| 15.2 | Daily limit enforcement | A | Exceed daily_limit_usd → requests blocked | [—] |
| 15.3 | Monthly limit enforcement | A | Exceed monthly_limit_usd → requests blocked | [—] |
| 15.4 | Warning at threshold | A | Reach warn_at_percent → warning emitted | [—] |
| 15.5 | `cost` CLI command | A | Shows accumulated costs | [—] |

---

## 16. Research Mode

| # | Feature | Type | How to validate | Status |
|---|---------|------|-----------------|--------|
| 16.1 | Trigger: Keywords | A | Message with keyword → research mode activates | [P] |
| 16.2 | Trigger: Length | A | Long message → research mode | [P] |
| 16.3 | Trigger: Question | A | Question mark → research mode | [P] |
| 16.4 | Max iterations | A | Research stops after max_iterations | [P] |
| 16.5 | End-to-end research | M | Research query → multi-step tool use → synthesized answer | [—] |

---

## 17. E2E Scenarios

These are end-to-end tests that exercise multiple subsystems together. All require a live provider (or mock provider).

| # | Scenario | Type | How to validate | Status |
|---|----------|------|-----------------|--------|
| 17.1 | Single-turn Q&A | M | `agent -m "What is 2+2?"` → "4" | [—] |
| 17.2 | Multi-turn conversation | M | Agent + gateway → multiple turns → context preserved | [—] |
| 17.3 | Tool-calling loop | M | `agent -m "find all TODO comments"` → glob + content_search + read_file | [—] |
| 17.4 | Delegation chain | M | Primary → coder → researcher → result returned to primary | [—] |
| 17.5 | Pipeline execution | M | Trigger keyword → 3-step pipeline → final output | [—] |
| 17.6 | Channel → agent → channel | M | Telegram message → agent processes → reply sent | [—] |
| 17.7 | MCP tool in agent loop | M | Agent calls `mcp__filesystem__read_file` autonomously | [—] |
| 17.8 | Emergency stop | M | `estop` during agent run → agent stops immediately | [—] |
| 17.9 | Gateway WebSocket stream | M | WS connect → send message → tokens stream back | [—] |
| 17.10 | Full office example | M | business-office config → multi-agent swarm processes query | [—] |

---

## Validation Execution

### Automated (CI)

```bash
# Full automated suite (runs in ~3 minutes)
just ci                        # fmt-check + clippy + nextest

# Feature-specific
cargo nextest run -p agentzero-core          # core traits, security, types
cargo nextest run -p agentzero-storage       # memory, encryption
cargo nextest run -p agentzero-config        # config parsing
cargo nextest run -p agentzero-infra         # tools, MCP, runtime
cargo nextest run -p agentzero-cli           # CLI commands
cargo nextest run -p agentzero-channels      # channel middleware
cargo nextest run -p agentzero-gateway       # gateway API
cargo nextest run -p agentzero-plugins       # WASM plugins
cargo nextest run -p agentzero-auth          # auth profiles

# With specific features
cargo nextest run --features "channels-standard"    # all channels
cargo nextest run --features "wasm-plugins"         # WASM runtime
cargo nextest run --features "telemetry"            # OTEL export
cargo nextest run --features "privacy"              # Noise Protocol
```

### Manual Test Script

```bash
# 1. Provider smoke test
export OPENAI_API_KEY="sk-..."
agentzero agent -m "Say hello in one word"

# 2. Gateway + pairing
agentzero gateway --port 8080 &
CODE=$(agentzero gateway --show-pairing-code 2>/dev/null)
TOKEN=$(curl -s -X POST http://localhost:8080/pair -H "X-Pairing-Code: $CODE" | jq -r .token)
curl -s http://localhost:8080/health
curl -s -X POST http://localhost:8080/api/chat \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message":"hello"}'

# 3. MCP tool discovery
mkdir -p .agentzero
echo '{"mcpServers":{"fs":{"command":"npx","args":["-y","@anthropic/mcp-server-filesystem","."]}}}' > .agentzero/mcp.json
agentzero tools list | grep mcp__

# 4. Delegation
# (requires [agents.*] in agentzero.toml)
agentzero agent -m "delegate to researcher: what is Rust?"

# 5. Channel test
agentzero channel list
agentzero channel test webhook

# 6. Daemon lifecycle
agentzero daemon start --port 8081
agentzero daemon status
agentzero daemon stop
```

---

## Coverage Summary

| Area | Total | Automated | Partial | Manual-Only | Missing |
|------|-------|-----------|---------|-------------|---------|
| Provider & Inference | 10 | 1 | 1 | 4 | 4 |
| Auth & Profiles | 11 | 4 | 0 | 1 | 6 |
| Configuration | 10 | 7 | 1 | 1 | 1 |
| Security | 35 | 30 | 2 | 0 | 3 |
| Storage & Memory | 14 | 10 | 1 | 1 | 2 |
| Tools | 57 | 23 | 15 | 5 | 14 |
| Multi-Agent | 37 | 14 | 8 | 1 | 14 |
| Event Bus | 7 | 5 | 0 | 0 | 2 |
| Gateway API | 16 | 5 | 2 | 1 | 8 |
| Channels | 27 | 14 | 13 | 0 | 0 |
| CLI Commands | 35 | 17 | 10 | 5 | 3 |
| Privacy & Encryption | 9 | 7 | 2 | 0 | 0 |
| Plugins | 8 | 6 | 1 | 1 | 0 |
| Autonomy | 7 | 5 | 0 | 0 | 2 |
| Cost Tracking | 5 | 0 | 0 | 0 | 5 |
| Research Mode | 5 | 0 | 4 | 0 | 1 |
| E2E Scenarios | 10 | 0 | 0 | 0 | 10 |
| **Totals** | **303** | **148 (49%)** | **60 (20%)** | **20 (7%)** | **75 (25%)** |

### Priority Gaps to Close

1. **E2E scenarios (17.1–17.10)**: Add mock-provider-based integration tests for the core agent loop, delegation, and pipeline flows
2. **Gateway API (9.7–9.16)**: Expand Axum test suite for OpenAI-compat, WebSocket, metrics, job lifecycle
3. **Cost tracking (15.1–15.5)**: Implement cost accumulator + limit enforcement tests
4. **Multi-agent queue/merge (7.6–7.7)**: Test Collect, Interrupt, WaitAny, WaitQuorum modes
5. **Pipeline error modes (7.3.3–7.3.8)**: Test skip, retry, timeout, regex triggers
6. **Network tools (6.3.3–6.3.5)**: Add mock HTTP server for web_fetch and http_request
7. **SOP tools (6.10.1–6.10.5)**: Add unit tests for SOP lifecycle
