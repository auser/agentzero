# Self-Evolving Agent System — NL Definitions, Runtime Tools, Catalog Learning

## Context

AgentZero can execute agent swarms (Sprint 72), but the system requires manual wiring and doesn't grow over time. You can't say "summarize this video" and have it figure out which tools each sub-agent needs, create missing tools on the fly, or remember what worked for next time. The `GoalPlanner` has types and a prompt but no LLM call. The `PluginScaffoldTool` creates WASM plugins but they only load on restart. `agent_manage` creates agents but requires explicit field-by-field specification.

**Core principle: persistence-first growth.** Every agent created, tool invented, and recipe learned persists across sessions. The system gets smarter over weeks and months — not just within a single run.

**Outcome:** A user says "summarize this video, generate a thumbnail, and write a script" and the system:
1. Decomposes the goal into agents (fetch → transcribe → thumbnail → summarize → script) with per-node tool hints
2. If whisper/yt-dlp tools don't exist yet, creates them mid-session as persistent `DynamicTool`s (available in all future sessions)
3. Allows defining persistent agents from plain English ("an agent that monitors my PRs daily") — stored in encrypted agent store, routable by keywords
4. Remembers: "video summarization needs shell + web_fetch + image_gen + whisper_transcribe" — next time someone says "summarize this podcast", it already knows the right tools
5. Every artifact persists: dynamic tools in `.agentzero/dynamic-tools.json`, agents in `.agentzero/agents.json`, recipes in `.agentzero/tool-recipes.json` — all encrypted at rest

---

## Phase A: NL Goal Decomposition (HIGH)

Wire `GoalPlanner::plan()` so goals auto-decompose into multi-agent DAGs with per-node tool filtering.

**A1. Add `tool_hints` to `PlannedNode` + update planner prompt**

File: `crates/agentzero-orchestrator/src/goal_planner.rs`

- Add `#[serde(default)] pub tool_hints: Vec<String>` to `PlannedNode` (backward-compatible)
- Update `GOAL_PLANNER_PROMPT`: add `tool_hints` to example JSON, add rule: *"tool_hints: list of tool names this agent needs (e.g. 'shell', 'read_file', 'web_fetch'). Include only tools relevant to the task. Leave empty if unsure."*
- Update `to_workflow_json()` to pass `tool_hints` through in metadata
- Add `tool_hints: vec![]` to manually-constructed `PlannedNode` in tests

**A2. Implement `GoalPlanner` struct with `plan()`**

File: `crates/agentzero-orchestrator/src/goal_planner.rs`

- `GoalPlanner` struct with `provider: Box<dyn Provider>`
- `plan(goal, available_tools) -> Result<PlannedWorkflow>` — builds prompt from `GOAL_PLANNER_PROMPT` + tool catalog + goal, calls `provider.complete()`, parses response
- Update `lib.rs` re-exports to include `GoalPlanner`

**A3. Add `HintedToolSelector`**

File: `crates/agentzero-infra/src/tool_selection.rs`

- `HintedToolSelector { hints: Vec<String>, fallback: KeywordToolSelector }`
- Non-empty hints → exact/substring match first, then keyword fallback
- Empty hints → delegates entirely to `KeywordToolSelector`
- Always includes foundational tools: `read_file`, `shell`, `content_search`

**A4. Wire tool selection into workflow dispatcher**

Files: `crates/agentzero-cli/src/commands/workflow.rs`, `crates/agentzero-gateway/src/workflow_dispatch.rs`

- In `CliStepDispatcher::run_agent()`: call `build_runtime_execution()` instead of `run_agent_once()`, extract `tool_hints` from step metadata, set `execution.tool_selector` to `HintedToolSelector`, call `run_agent_with_runtime()`
- No changes to `RunAgentRequest` — `RuntimeExecution.tool_selector` is already `pub`

**A5. Extract `build_provider_from_config()` helper**

File: `crates/agentzero-infra/src/runtime.rs`

- Extract config-loading + API-key-resolution + provider-construction into a standalone public function
- Refactor `build_runtime_execution()` to call it internally

**A6. Wire `GoalPlanner` into swarm CLI**

File: `crates/agentzero-cli/src/commands/swarm.rs`

- Replace single-agent fallback (lines 40-57) with `GoalPlanner::plan()` call using provider from `build_provider_from_config()`

---

## Phase B: Runtime Tool Creation + Persistent Tool Growth (HIGH)

Agents describe a missing tool in NL → system creates it mid-session, immediately available, **and persists it forever**. Next session, the tool is already there. Over time, the system accumulates a library of user-specific tools it invented.

**B1. `DynamicTool` struct + execution strategies**

New file: `crates/agentzero-infra/src/tools/dynamic_tool.rs`

- `DynamicToolDef { name, description, strategy, input_schema, created_at }`
- `DynamicToolStrategy` enum: `Llm { system_prompt }`, `Shell { command_template }`, `Http { url, method, headers }`, `Composite { steps }`
- Implement `Tool` trait using `Box::leak()` for `&'static str` (same pattern as MCP tools)
- Security: shell strategy validates against `ShellPolicy`, HTTP validates against `UrlAccessPolicy`

**B2. `DynamicToolRegistry` — persistence + runtime registration**

Same file: `crates/agentzero-infra/src/tools/dynamic_tool.rs`

- `DynamicToolRegistry { tools: Arc<RwLock<Vec<DynamicToolDef>>>, store: EncryptedJsonStore }`
- Persistence at `.agentzero/dynamic-tools.json`
- `register(def) -> Box<dyn Tool>`, `load_all() -> Vec<Box<dyn Tool>>`, `remove(name) -> bool`

**B3. `ToolSource` trait for mid-session registration**

File: `crates/agentzero-core/src/agent.rs`

- `trait ToolSource: Send + Sync { fn additional_tools(&self) -> Vec<Box<dyn Tool>>; }`
- Add `extra_tool_source: Option<Arc<dyn ToolSource>>` to `Agent`
- In `build_tool_definitions()`, merge `self.tools` with `extra_tool_source.additional_tools()`
- `DynamicToolRegistry` implements `ToolSource`

**B4. `ToolCreateTool` — LLM-callable tool for creating dynamic tools**

New file: `crates/agentzero-infra/src/tools/tool_create.rs`

- Actions: `create` (NL → LLM derives `DynamicToolDef` → register), `list`, `delete`
- Gated by `ctx.depth == 0` and `enable_dynamic_tools: bool` in `ToolSecurityPolicy`

**B5. Wire into tool registration**

- `crates/agentzero-infra/src/tools/mod.rs` — load dynamic tools at startup, register `ToolCreateTool`
- `crates/agentzero-tools/src/lib.rs` — add `enable_dynamic_tools: bool` to `ToolSecurityPolicy`
- `crates/agentzero-infra/src/runtime.rs` — add `dynamic_registry: Option<Arc<DynamicToolRegistry>>` to `RuntimeExecution`, wire into agent's `extra_tool_source`

---

## Phase C: NL Agent Definitions — Persistent Specialists (MEDIUM)

Define persistent agents from plain English descriptions. These agents live in the encrypted agent store and are available in every future session — they accumulate as the user's personal team of specialists.

**C1. Add `create_from_description` action to `AgentManageTool`**

File: `crates/agentzero-infra/src/tools/agent_manage.rs`

- New action `"create_from_description"` with `description: String` input
- LLM derives: name, description, system_prompt, keywords, allowed_tools, suggested_schedule
- Add `provider: Option<Arc<dyn Provider>>` to `AgentManageTool` via `with_provider()` builder
- Persistence: created agents persist in `.agentzero/agents.json` (encrypted)

**C2. Wire provider into `AgentManageTool`**

File: `crates/agentzero-infra/src/tools/mod.rs`

- Pass the primary provider when constructing `AgentManageTool`

**C3. Auto-routing — agents become discoverable**

- Verify `AgentRouter` reads from `AgentStoreApi` dynamically (not cached at construction)
- Future goals matching stored agent keywords auto-route to the specialist

**C4. Agent self-improvement**

- Add `version: u32` field to `AgentRecord`
- When similar NL description given again, update existing agent rather than creating a duplicate
- LLM prompt includes existing agents for dedup awareness

---

## Phase D: Tool Catalog Learning — Compounding Knowledge (MEDIUM)

Record successful tool combos, boost them on matching future goals. After a month of use, the system has a rich catalog of "for X kind of task, use these tools" — like institutional memory.

**D1. `ToolRecipe` and `RecipeStore`**

New file: `crates/agentzero-infra/src/tool_recipes.rs`

- `ToolRecipe { id, goal_summary, goal_keywords, tools_used, success, timestamp, use_count }`
- `RecipeStore` backed by `EncryptedJsonStore` at `.agentzero/tool-recipes.json`
- `record(goal, tools, success)`, `find_matching(goal, top_k) -> Vec<ToolRecipe>` (TF-IDF)

**D2. Record recipes after swarm execution**

File: `crates/agentzero-orchestrator/src/swarm_supervisor.rs`

- Add `recipe_store: Option<Arc<Mutex<RecipeStore>>>` to `SwarmSupervisor`
- After `execute()`, record recipe with goal title and tool_hints from completed nodes

**D3. Record recipes after single-agent runs**

File: `crates/agentzero-infra/src/runtime.rs`

- Add `tools_invoked: Vec<String>` to `RunAgentOutput`
- Record recipe if `RecipeStore` is available

**D4. Integrate recipes into `HintedToolSelector`**

File: `crates/agentzero-infra/src/tool_selection.rs`

- Extend `HintedToolSelector` with `recipes: Option<Arc<Mutex<RecipeStore>>>`
- Selection priority: explicit hints → recipe-matched tools → keyword fallback

---

## Growth Lifecycle — How the System Compounds

**Week 1:** User says "summarize this video." System decomposes goal, notices it needs whisper → creates `whisper_transcribe` shell tool → succeeds → records recipe.

**Week 2:** User says "transcribe this podcast." Recipe store matches "transcribe" → boosts `whisper_transcribe` tool (already exists from Week 1). No tool creation needed. Faster, zero setup.

**Week 3:** User says "I need an agent that reviews my PRs daily." System creates `pr_reviewer` agent with keywords `["pr", "review", "github"]`, system prompt derived from NL, scheduled via cron. Agent persists in encrypted store.

**Week 4:** User says "review the latest PRs." System's `AgentRouter` matches keywords → routes to the persistent `pr_reviewer` agent rather than creating a generic one. The agent already has the right tools and prompt.

**Month 2:** The system has accumulated: 12 dynamic tools, 5 specialist agents, 40 recipes. New goals resolve faster because the right tools and agents already exist. The encrypted stores at `.agentzero/` are the system's "brain" — portable, backupable, growing.

**Persistence files (all encrypted at rest):**
- `.agentzero/dynamic-tools.json` — invented tools (shell, http, llm, composite strategies)
- `.agentzero/agents.json` — NL-defined persistent agents
- `.agentzero/tool-recipes.json` — successful tool combos indexed by goal keywords
- `.agentzero/skills-state.json` — behavioral templates (existing)
- `.agentzero/plugins/` — WASM plugins (existing, for heavy-duty tools)

---

## Implementation Order

```
Phase A (Goal Decomposition)     ← start here
    ↓
Phase B (Runtime Tools)          ← can start after A3
Phase C (NL Agents)              ← can start after A5
    ↓
Phase D (Catalog Learning)       ← after B+C complete
```

## Verification

1. `cargo clippy --workspace` — 0 warnings
2. `cargo test -p agentzero-orchestrator` — GoalPlanner unit tests with mock provider
3. `cargo test -p agentzero-infra` — HintedToolSelector, DynamicTool, RecipeStore tests
4. Manual E2E: `agentzero swarm "summarize this video"` → multi-node plan generated → each agent gets filtered tools → execution completes
5. Manual E2E: Agent calls `tool_create` with "a tool that downloads YouTube videos using yt-dlp" → shell-strategy dynamic tool registered → available in same session
6. Manual E2E: `agent_manage create_from_description "an agent that reviews my PRs daily"` → full AgentRecord created with derived fields
