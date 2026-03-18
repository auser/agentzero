# Sprint 37 — MiniMax-Inspired Feature Parity: Code Interpreter, Browser, Context Summarization, Media Gen

## Context

Comparing agentzero against [MiniMax Agent](https://agent.minimax.io/) revealed 4 capability gaps worth closing. AgentZero already leads on orchestration (swarm, lanes, fanout, pipelines), async jobs, E2E encryption, and channel integrations. But MiniMax agents ship with built-in code execution, browser, intelligent context management, and media generation — tools that expand what agents can autonomously accomplish.

---

## Feature 1: Sandboxed Code Interpreter

**Goal:** Execute Python/JS in an isolated subprocess with output capture (stdout, stderr, files/charts).

### Files to create
- `crates/agentzero-tools/src/code_interpreter.rs` — `CodeInterpreterTool` impl

### Files to modify
- `crates/agentzero-tools/src/lib.rs` — add module, re-export, add `enable_code_interpreter: bool` to `ToolSecurityPolicy`
- `crates/agentzero-infra/src/tools/mod.rs` — register behind policy gate in `default_tools()`
- `crates/agentzero-config/src/model.rs` — add `CodeInterpreterConfig` struct
- `crates/agentzero-config/src/policy.rs` — wire config → policy
- `examples/config-full.toml` — add `[code_interpreter]` section

### Design
```rust
pub struct CodeInterpreterTool {
    timeout_ms: u64,          // default 30_000
    max_output_bytes: usize,  // default 65536
    allowed_languages: Vec<String>,
}

// Input schema
{ "language": "python"|"javascript", "code": "..." }
```

**Execution:** Write code to temp file in `{workspace}/.agentzero/sandbox/`, spawn `python3 -u` or `node`, apply `tokio::time::timeout`, capture stdout/stderr with same `read_limited` pattern as `ShellTool`. Scan output dir for generated files. Return stdout + stderr + exit code + file paths.

**Security:** Own policy gate (not shell allowlist). Subprocess timeout + output size cap. Workspace-confined output dir. On Linux, optional `setrlimit` via `Command::pre_exec`.

### Config
```toml
[code_interpreter]
enabled = false
timeout_ms = 30000
max_output_bytes = 65536
allowed_languages = ["python", "javascript"]
```

---

## Feature 2: Web Browsing Tool Enhancement

**Goal:** The existing `BrowserTool` (in `crates/agentzero-tools/src/browser.rs`) delegates to an `agent-browser` external process and already supports navigate, snapshot, click, fill, type, get_text, screenshot, etc. The `input_schema()` advertises actions like `execute_js` and `content` that don't exist in the `BrowserAction` enum.

### Files to modify
- `crates/agentzero-tools/src/browser.rs` — add `ExecuteJs { script }` and `Content` variants to `BrowserAction` enum, sync `input_schema()` with actual capabilities

### Design
- `ExecuteJs`: forwards `{"action": "execute_js", "script": "..."}` to `agent-browser` subprocess
- `Content`: forwards `{"action": "content"}` to get clean page text
- Both use existing subprocess dispatch pattern — minimal code change

---

## Feature 3: Context Summarization

**Goal:** When conversation history exceeds a threshold, summarize older entries using the LLM instead of hard-truncating. Opt-in via config.

### Files to modify
- `crates/agentzero-core/src/agent.rs` — modify context building in `call_provider_with_context()` / `build_provider_prompt()`
- `crates/agentzero-core/src/types.rs` — add `SummarizationConfig` to `AgentConfig`
- `crates/agentzero-config/src/model.rs` — add `SummarizationSettings`

### Design

**Current flow:** Fetch `memory_window_size` entries → format as text → hard-truncate at `max_prompt_chars`.

**New flow (when enabled):**
1. Fetch `memory_window_size` entries as before
2. Split: **older** entries (to summarize) vs **recent** entries (last `keep_recent`, kept verbatim)
3. If older count >= `min_entries_for_summarization`, call provider with a summarization prompt (non-streaming, 5s timeout, no tools)
4. Cache summary keyed by hash of entry contents (avoid re-summarizing on every turn)
5. Build prompt: `"Context summary:\n{summary}\n\nRecent conversation:\n{recent}\n\nCurrent input:\n{prompt}"`
6. Fallback: if summarization call fails/times out, fall back to hard-truncation (existing behavior)

```rust
pub struct SummarizationConfig {
    pub enabled: bool,                       // default: false
    pub keep_recent: usize,                  // default: 10
    pub min_entries_for_summarization: usize, // default: 20
    pub max_summary_chars: usize,            // default: 2000
}
```

**Key reuse:** Uses existing `Provider::complete()` for the summarization call. The `Agent` struct already holds the provider.

### Config
```toml
[agent.summarization]
enabled = false
keep_recent = 10
min_entries_for_summarization = 20
max_summary_chars = 2000
```

---

## Feature 4: Media Generation Tools (TTS, Image, Video)

**Goal:** Provider-agnostic tools that call external APIs, save output to workspace, return file paths.

### Files to create
- `crates/agentzero-tools/src/media_gen.rs` — `TtsTool`, `ImageGenTool`, `VideoGenTool`

### Files to modify
- `crates/agentzero-tools/src/lib.rs` — module, re-exports, add `enable_tts`, `enable_image_gen`, `enable_video_gen` to `ToolSecurityPolicy`
- `crates/agentzero-infra/src/tools/mod.rs` — register behind policy gates
- `crates/agentzero-config/src/model.rs` — add `MediaGenConfig` with sub-configs
- `crates/agentzero-config/src/policy.rs` — wire config → policy
- `crates/agentzero-core/src/types.rs` — add `Audio` variant to `ContentPart` enum
- `examples/config-full.toml` — add `[media_gen]` section

### Design

Each tool follows the same pattern: parse JSON input → resolve API key from env → HTTP POST to provider → save response bytes to `{workspace}/.agentzero/media/{timestamp}_{hash}.{ext}` → return path.

```rust
pub struct TtsTool { client: reqwest::Client, config: MediaEndpointConfig, model: String, default_voice: String }
// Input: { "text": "...", "voice?": "alloy", "format?": "mp3" }
// Default: OpenAI TTS API

pub struct ImageGenTool { client: reqwest::Client, config: MediaEndpointConfig, model: String }
// Input: { "prompt": "...", "size?": "1024x1024", "style?": "natural" }
// Default: OpenAI DALL-E 3

pub struct VideoGenTool { client: reqwest::Client, config: MediaEndpointConfig, model: String }
// Input: { "prompt": "...", "image_path?": "...", "duration_secs?": 5 }
// Default: MiniMax Hailuo. Async poll pattern with timeout.
```

Tools use `from_env()` constructors reading API URLs/keys from env vars, matching patterns of existing tools like `ComposioTool`.

### Config
```toml
[media_gen.tts]
enabled = false
api_url = "https://api.openai.com/v1/audio/speech"
api_key_env = "OPENAI_API_KEY"
model = "tts-1"
default_voice = "alloy"

[media_gen.image_gen]
enabled = false
api_url = "https://api.openai.com/v1/images/generations"
api_key_env = "OPENAI_API_KEY"
model = "dall-e-3"

[media_gen.video_gen]
enabled = false
api_url = "https://api.minimax.chat/v1/video_generation"
api_key_env = "MINIMAX_API_KEY"
model = "MiniMax-Hailuo-2.3"
```

### ContentPart addition
```rust
pub enum ContentPart {
    Text { text: String },
    Image { media_type: String, data: String },
    Audio { media_type: String, data: String },  // NEW
}
```

---

## Implementation Order

| # | Feature | Scope | Dependencies |
|---|---------|-------|-------------|
| 1 | Code Interpreter | New tool file + config + policy wiring | None |
| 2 | Browser Enhancement | Small enum/schema fix in existing file | None |
| 3 | Media Generation | New tool file + config + ContentPart addition | None |
| 4 | Context Summarization | Modify agent.rs core loop | None (but most impactful — do last) |

Features 1-3 can be parallelized. Feature 4 touches the agent hot path and should be done last with care.

---

## Verification

### Code Interpreter
- `cargo test -p agentzero-tools` — unit tests for input parsing, language validation, timeout
- Manual: enable in config, ask agent to "write a Python script that prints fibonacci numbers"

### Browser Enhancement
- `cargo test -p agentzero-tools` — verify new BrowserAction variants parse correctly
- Manual: verify `input_schema()` matches enum variants

### Media Generation
- `cargo test -p agentzero-tools` — unit tests for input parsing, missing API key error
- Manual with mock server or real API: enable TTS, ask agent to "read this text aloud"

### Context Summarization
- `cargo test -p agentzero-core` — test build_provider_prompt with/without summarization
- Manual: set `keep_recent = 3`, `min_entries = 5`, run long conversation, verify "Context summary:" appears in prompt

### Full suite
```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

---

## Sources
- [MiniMax Agent Platform](https://agent.minimax.io/)
- [MiniMax API Overview](https://platform.minimax.io/docs/api-reference/api-overview)
- [Mini-Agent GitHub](https://github.com/MiniMax-AI/Mini-Agent)
- [MiniMax MCP Server](https://github.com/MiniMax-AI/MiniMax-MCP)
