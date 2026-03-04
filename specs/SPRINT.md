# AgentZero Sprint Plan

## Sprint 21: Structured Tool Use

**Goal:** Wire tool schemas into provider API calls so LLMs use native tool-use/function-calling APIs instead of text-based `tool:name input` parsing. This dramatically improves tool call accuracy, enables input validation, and unlocks provider-native features like parallel tool calls, streaming tool use, and stop_reason-based control flow.

**Predecessor:** Sprint 20 (Plugin Architecture) added `description()` and `input_schema()` to all 50+ tools, but they aren't used in provider calls or the agent loop.

### Current State (Text-Based Tool Calling)

```
User message → Agent → Provider.complete(text_prompt)
                          ↓
                 LLM sees tool names in system prompt text
                          ↓
                 LLM responds with "tool:read_file /path/to/file" text
                          ↓
                 Agent parses lines starting with "tool:" via parse_tool_calls()
                          ↓
                 Agent calls tool.execute(input_text, ctx)
```

**Problems:**
- LLM must learn tool syntax from prompt text (unreliable, model-dependent)
- No input validation — LLM can send malformed input
- Tool list in system prompt wastes tokens
- No structured error feedback to LLM
- Text parsing is fragile (`tool:` prefix can appear in legitimate output)

### Target State (Structured Tool Use)

```
User message → Agent → Provider.complete_with_tools(messages, tool_defs)
                          ↓
                 LLM sees tools via native API (Anthropic tools[], OpenAI functions[])
                          ↓
                 LLM responds with tool_use content block + structured JSON input
                          ↓
                 Agent receives ToolUseRequest { id, name, input: Value }
                          ↓
                 Agent validates input against schema, calls tool.execute()
                          ↓
                 Agent sends tool_result content block back to provider
```

---

### Phase 1: Provider Tool Definitions (Days 1-3)

**Goal:** Providers accept tool definitions and include them in API requests.

**Files to modify:**

- `crates/agentzero-core/src/types.rs`
  - [ ] Add `ToolDefinition { name, description, input_schema }` struct
  - [ ] Add `ToolUseRequest { id, name, input: Value }` struct
  - [ ] Add `ToolResultMessage { tool_use_id, content: String, is_error: bool }` struct
  - [ ] Extend `Provider` trait with `complete_with_tools()` default method:
    ```rust
    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ProviderResponse>;
    ```
  - [ ] Add `ConversationMessage` enum (User, Assistant, ToolResult)
  - [ ] Add `ProviderResponse` struct with `content: Vec<ContentBlock>` (text + tool_use blocks)
  - [ ] Default impl falls back to `complete()` for backward compat

- `crates/agentzero-providers/src/anthropic.rs`
  - [ ] Add `tools: Option<Vec<ToolDef>>` field to `MessagesRequest`
  - [ ] Implement `complete_with_tools()` for `AnthropicProvider`:
    - Convert `ToolDefinition` → Anthropic `Tool { name, description, input_schema }` format
    - Handle `stop_reason: "tool_use"` in response
    - Parse `ContentBlock::ToolUse { id, name, input }` from response
    - Return structured `ProviderResponse` with tool_use blocks
  - [ ] Keep `complete()` and `complete_with_reasoning()` unchanged (backward compat)

- `crates/agentzero-providers/src/openai.rs`
  - [ ] Add `tools: Option<Vec<FunctionDef>>` field to request payload
  - [ ] Implement `complete_with_tools()` for `OpenAiCompatibleProvider`:
    - Convert `ToolDefinition` → OpenAI `{ type: "function", function: { name, description, parameters } }` format
    - Handle `finish_reason: "tool_calls"` in response
    - Parse `tool_calls[].function.{name, arguments}` from response
    - Return structured `ProviderResponse`
  - [ ] This covers all OpenAI-compatible providers (OpenRouter, Together, Groq, etc.)

**Tests:**
- [ ] Anthropic: tool definitions serialized correctly in request payload
- [ ] Anthropic: tool_use response parsed into ToolUseRequest
- [ ] Anthropic: stop_reason "tool_use" vs "end_turn" handled correctly
- [ ] OpenAI: function definitions serialized correctly
- [ ] OpenAI: tool_calls response parsed into ToolUseRequest
- [ ] Backward compat: complete() still works without tools

---

### Phase 2: Agent Loop — Structured Tool Dispatch (Days 4-7)

**Goal:** Agent loop uses provider's structured tool_use response instead of text parsing.

**Files to modify:**

- `crates/agentzero-core/src/agent.rs`
  - [ ] Add `build_tool_definitions()` method: converts `Vec<Box<dyn Tool>>` to `Vec<ToolDefinition>` using `name()`, `description()`, and `input_schema()`
  - [ ] Tools without `input_schema()` get a permissive schema: `{ "type": "object", "properties": { "input": { "type": "string" } } }`
  - [ ] Add `respond_with_tools()` path in `respond_inner()`:
    - When `config.model_supports_tool_use` is true, use `provider.complete_with_tools()`
    - Build `ConversationMessage` sequence: user → [assistant + tool_use → tool_result]*
    - Continue loop while response contains `tool_use` blocks (stop_reason == "tool_use")
    - Break when response is text-only (stop_reason == "end_turn")
  - [ ] Keep `parse_tool_calls()` text-based path as fallback for models without tool_use support
  - [ ] Input validation: if tool has `input_schema()`, validate `ToolUseRequest.input` against it before execution
  - [ ] Structured error feedback: on tool error, send `ToolResultMessage { is_error: true }` back to provider

- `crates/agentzero-core/src/agent.rs` — Parallel tool calls:
  - [ ] When response contains multiple `tool_use` blocks, execute them in parallel (existing parallel_tools logic)
  - [ ] Collect all `ToolResultMessage` responses and send as a batch
  - [ ] Gated tools still fall back to sequential execution

**Tests:**
- [ ] Agent calls tool via structured path when model_supports_tool_use is true
- [ ] Agent falls back to text parsing when model_supports_tool_use is false
- [ ] Multiple tool_use blocks → parallel execution
- [ ] Tool error → is_error tool_result → LLM can recover
- [ ] Input validation rejects invalid JSON against schema
- [ ] Multi-turn conversation: user → tool_use → tool_result → tool_use → text response
- [ ] Existing text-based tests still pass (backward compat)

---

### Phase 3: Conversation Message History (Days 8-10)

**Goal:** Multi-turn tool-use conversations maintain proper message history.

**Files to modify:**

- `crates/agentzero-core/src/agent.rs`
  - [ ] Replace `prompt: String` accumulation with `messages: Vec<ConversationMessage>` in tool loop
  - [ ] Each iteration appends: assistant response (with tool_use) → tool_result(s)
  - [ ] Memory integration: store structured messages in memory, reconstruct conversation on recall
  - [ ] Truncation: when conversation exceeds `max_prompt_chars`, summarize earlier tool interactions
  - [ ] Research phase: research findings become a system prompt prefix, not a prompt mutation

- `crates/agentzero-core/src/types.rs`
  - [ ] Add `ConversationMessage` to memory serialization format
  - [ ] Backward compat: existing `MemoryEntry` format still works for simple text

**Tests:**
- [ ] 5-turn tool conversation maintains correct message order
- [ ] Conversation truncation preserves most recent tool interactions
- [ ] Memory recall restores conversation context
- [ ] Research context injected as system prompt, not user message

---

### Phase 4: Streaming Tool Use (Days 11-13)

**Goal:** Support streaming responses that include tool_use blocks.

**Files to modify:**

- `crates/agentzero-providers/src/anthropic.rs`
  - [ ] Handle SSE events for tool_use: `content_block_start` (type: tool_use), `content_block_delta` (input_json_delta), `content_block_stop`
  - [ ] Accumulate tool input JSON across delta events
  - [ ] Emit text deltas normally via existing streaming path
  - [ ] Return both text and tool_use blocks in streaming response

- `crates/agentzero-providers/src/openai.rs`
  - [ ] Handle SSE chunks with `tool_calls` field
  - [ ] Accumulate function arguments across chunks

- `crates/agentzero-core/src/types.rs`
  - [ ] Extend `StreamChunk` with optional `tool_use: Option<ToolUseRequest>` for tool_use events

**Tests:**
- [ ] Anthropic streaming with tool_use: text + tool_use blocks parsed correctly
- [ ] OpenAI streaming with tool_calls: function args accumulated correctly
- [ ] Mixed text + tool_use streaming works end-to-end

---

### Phase 5: Schema Validation + Auto-Documentation (Days 14-15)

**Goal:** Validate tool inputs against schemas before execution, generate tool documentation.

**Files to modify/create:**

- `crates/agentzero-core/src/validation.rs` (new)
  - [ ] `validate_tool_input(input: &Value, schema: &Value) -> Result<(), ValidationError>`
  - [ ] Validate required properties, type constraints, enum values
  - [ ] Lightweight — no jsonschema crate dependency; validate the subset we actually use:
    - `type` (string, number, boolean, object, array)
    - `required` properties
    - `enum` values
    - `properties` (recursive)
  - [ ] Return clear error messages for LLM retry

- `crates/agentzero-cli/src/commands/tools.rs` (new or modify existing)
  - [ ] `agentzero tools list` — show all tools with descriptions
  - [ ] `agentzero tools info <name>` — show tool description + input schema
  - [ ] `agentzero tools schema` — dump all tool schemas as JSON (for external tooling)

**Tests:**
- [ ] Valid input passes validation
- [ ] Missing required field → clear error
- [ ] Wrong type → clear error
- [ ] Invalid enum value → clear error
- [ ] Nested object validation works
- [ ] Tool without schema always passes

---

### Verification (End-to-End)

- [ ] `cargo build -p agentzero --release` compiles
- [ ] `cargo build -p agentzero --profile release-min --no-default-features --features minimal` stays under 6MB
- [ ] `cargo test --workspace` — all tests pass
- [ ] Anthropic provider sends `tools[]` with definitions in API request
- [ ] OpenAI-compatible provider sends `tools[]` with function definitions
- [ ] Agent uses structured tool_use when model supports it
- [ ] Agent falls back to text-based tool calling for models without tool_use
- [ ] Multi-turn tool conversations work correctly
- [ ] Parallel tool calls work via structured path
- [ ] Input validation catches malformed tool inputs
- [ ] Tool errors propagate as is_error tool_results
- [ ] Streaming with tool_use works for Anthropic
- [ ] `agentzero tools list` shows all available tools
- [ ] Binary size budgets maintained
- [ ] Existing tests remain passing (no regressions)

---

### Architecture Impact

**What changes:**
- Provider trait gains `complete_with_tools()` method (backward-compatible default)
- Agent loop gains structured tool dispatch path alongside existing text path
- Tool definitions derived from `Tool::description()` + `Tool::input_schema()` (Sprint 20)
- Conversation history becomes structured messages instead of concatenated text

**What stays the same:**
- `Tool` trait unchanged (name, description, input_schema, execute)
- Plugin system unchanged (WasmTool, FfiTool work transparently)
- Text-based fallback preserved for models without tool_use support
- Security model unchanged (ToolSecurityPolicy, autonomy checks)
- Memory backend unchanged (SQLite/Turso)

**Expected improvements:**
- Tool call accuracy: structured format eliminates parsing ambiguity
- Token efficiency: tool definitions sent once via API, not repeated in system prompt
- Error recovery: LLM can retry with corrected input after validation errors
- Parallel tool calls: native provider support instead of text-based heuristics

Previous sprint archived to `specs/sprints/20-plugin-architecture.md`.
