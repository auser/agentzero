# Sprint 74-75: Self-Evolution Engine

## Sprint Split
- **Sprint 74** (this sprint): Phases A + B + F — Telemetry, Quality Tracking, AUTO-FIX, AUTO-IMPROVE, User Feedback
- **Sprint 75** (next sprint): Phases C + D + E — AUTO-LEARN, Recipe Evolution, Real Composite Execution, Two-Stage Selection, Sharing

## Context

Sprint 73 shipped the self-growing foundation: NL goal decomposition, dynamic tool creation, NL agent definitions, and recipe-based catalog learning. The system can *create* tools and *record* what worked — but it never *improves* them, *captures* novel patterns, *shares* knowledge, or *scales* tool selection efficiently.

**Problem:** Static creation without feedback. Tools never get better or get repaired. Successful multi-tool combos are recorded as recipes but never crystallized into reusable composite tools or refined over time. Tool selection sends everything to the LLM regardless of scale. No way to share tools between instances. No human feedback signal.

**Outcome:** Seven capabilities across six phases:
1. **Quality tracking** — Per-tool and per-recipe success/failure counters, execution telemetry, quality-gated selection
2. **AUTO-FIX** — Failing tools (>60% failure rate) get LLM-based repair automatically
3. **AUTO-IMPROVE** — High-quality tools (>80% success rate) evolve into optimized variants via LLM analysis
4. **AUTO-LEARN** — Novel successful tool combinations are captured as reusable Composite dynamic tools; recipes evolve when variants outperform originals
5. **Two-stage tool selection** — Keyword/embedding pre-filter → LLM refinement, preventing prompt bloat
6. **Tool/recipe sharing** — Export bundles (tool + related recipes + lineage) and import them, with gateway API
7. **User feedback signals** — Explicit quality ratings on tools so humans can guide evolution

---

## Phase A: Execution Telemetry + Quality Tracking (Foundation)

All other phases depend on this. Quality counters on tools and recipes, plus structured per-tool execution records.

**A1. `ToolExecutionRecord` type + `tool_executions` on `ToolContext`**

File: `crates/agentzero-core/src/types.rs`

- Add `ToolExecutionRecord { tool_name: String, success: bool, error: Option<String>, latency_ms: u64, timestamp: u64 }` (after `AuditEvent`)
- Add `#[serde(skip)] pub tool_executions: Arc<std::sync::Mutex<Vec<ToolExecutionRecord>>>` to `ToolContext`
- Initialize in `ToolContext::new()`, add to Debug impl
- Re-export from `lib.rs`

**A2. Collect records in `Agent::execute_tool()`**

File: `crates/agentzero-core/src/agent.rs`

- Add `record_tool_execution(ctx, tool_name, success, error, latency_ms)` static method pushing to `ctx.tool_executions`
- Call on all 3 error paths (tool error, timeout, no-timeout error) and the success path in `execute_tool()`
- Reuse existing `tool_started.elapsed()` timing already computed

**A3. Quality fields on `DynamicToolDef`**

File: `crates/agentzero-infra/src/tools/dynamic_tool.rs`

- Add `#[serde(default)]` fields: `total_invocations: u32`, `total_successes: u32`, `total_failures: u32`, `last_error: Option<String>`, `generation: u32`, `parent_name: Option<String>`
- Add `success_rate() -> f64` method
- Add `record_outcome(name, success, error)` on `DynamicToolRegistry`
- Add `get_def(name) -> Option<DynamicToolDef>`, `is_dynamic(name) -> bool`
- Fix all construction sites (tests + tool_create.rs) — add zeroed defaults

**A4. Quality fields on `ToolRecipe`**

File: `crates/agentzero-infra/src/tool_recipes.rs`

- Add `#[serde(default)]` fields: `total_applications: u32`, `total_successes: u32`, `total_failures: u32`
- Add `success_rate() -> f64` method, `record_outcome(recipe_id, success)` on `RecipeStore`
- Quality-weighted matching: multiply TF-IDF score by `success_rate()`, exclude recipes with <15% success rate and >=3 applications

**A5. Surface in `RunAgentOutput` + persist + wire counters**

File: `crates/agentzero-infra/src/runtime.rs`

- Add `tool_executions: Vec<ToolExecutionRecord>` to `RunAgentOutput`
- Add `recipe_store: Option<Arc<Mutex<RecipeStore>>>` to `RuntimeExecution`
- After run: extract from `ctx.tool_executions`, persist to `execution-history.jsonl` (10k line cap)
- Update dynamic tool quality counters via `registry.record_outcome()`
- Record tool usage as recipe via `recipe_store.record()`
- Build `RecipeStore` in `build_runtime_execution()`

---

## Phase B: AUTO-FIX + AUTO-IMPROVE — Tool Evolution Engine (HIGH)

Two evolution modes in one module: repair failing tools and optimize successful ones.

**B1. `ToolEvolver` struct**

New file: `crates/agentzero-infra/src/tool_evolver.rs`

- `ToolEvolver { provider: Arc<dyn Provider>, registry: Arc<DynamicToolRegistry>, session_fixes: Mutex<HashSet<String>> }`

**AUTO-FIX methods:**
- `maybe_fix(tool_name) -> Result<bool>` — checks if tool qualifies (failure rate >60%, invocations >=5, not already fixed this session, generation < 5)
- `fix(def, recent_errors) -> Result<DynamicToolDef>` — sends tool definition + last errors to LLM → repaired strategy
- `TOOL_FIX_PROMPT` — receives tool name, description, strategy JSON, last 5 error messages

**AUTO-IMPROVE methods:**
- `get_improvement_candidates() -> Vec<DynamicToolDef>` — tools with `total_invocations >= 10` and `success_rate() >= 0.8`
- `improve(def) -> Result<DynamicToolDef>` — LLM analyzes strategy + success patterns → produces optimized variant
- `evolve_candidates() -> Result<Vec<String>>` — finds candidates and improves them
- `TOOL_IMPROVE_PROMPT` — receives tool name, description, strategy JSON, success rate, invocation count

Both prompts use the same JSON parsing pattern as `TOOL_CREATE_PROMPT` + `parse_json_from_response()` in `tool_create.rs`.

**B2. Evolution safeguards**

- `session_fixes: Mutex<HashSet<String>>` — each tool can only be evolved once per session
- Max 5 total evolutions per session (fixes + improvements combined)
- `generation >= 5` cap for fixes, `generation >= 3` cap for improvements
- 24h cooldown for improvements: skip tools where any child has `created_at` within last 86400s
- Evolved tool gets `generation: parent.generation + 1`, `parent_name: Some(parent.name)`, zeroed counters
- Minimum 5 invocations between any evolution events

**B3. Wire into runtime**

File: `crates/agentzero-infra/src/runtime.rs`

- Add `tool_evolver: Option<Arc<ToolEvolver>>` to `RuntimeExecution`
- Construct in `build_runtime_execution()` when dynamic tools enabled (reuse provider)
- After run completes: check failed dynamic tools for auto-fix (negative feedback path)
- After run completes: run `evolve_candidates()` for auto-improve (positive feedback path)
- Both fire-and-forget with warning on failure

---

## Phase C: AUTO-LEARN — Captured Patterns + Recipe Evolution (MEDIUM)

Two sub-features: (1) capture novel multi-tool combos as reusable Composite tools, (2) evolve recipes when variants outperform originals.

**C1. `PatternCapture` struct**

New file: `crates/agentzero-infra/src/pattern_capture.rs`

- `PatternCapture { registry: Arc<DynamicToolRegistry>, recipe_store: Arc<Mutex<RecipeStore>> }`
- `capture_if_novel(goal: &str, tool_executions: &[ToolExecutionRecord]) -> Result<Option<String>>`

**C2. Novelty detection logic**

In `capture_if_novel`:
1. Extract unique successful tool names from `tool_executions`
2. If fewer than 3 unique tools → return `None`
3. Check existing recipes: `find_matching(goal, 5)` — if any recipe has Jaccard >= 0.8 on tool set → not novel
4. Create `Composite` `DynamicToolDef` with steps from execution order
5. Register + record recipe
6. Name: `auto_{first_keyword}_{short_timestamp}`

**C3. Recipe evolution**

File: `crates/agentzero-infra/src/tool_recipes.rs`

Add `evolve_recipes(&mut self) -> anyhow::Result<u32>` to `RecipeStore`:
1. Group recipes by goal similarity (Jaccard >= 0.7 on keywords)
2. Within each group, find the recipe with highest `success_rate()` (requires >=3 applications)
3. If the best variant has success_rate >= 0.2 higher than the group's next-best, **promote** it: boost its `use_count` by the delta
4. If a recipe has `total_applications >= 5` and `success_rate() < 0.15`, **retire** it: remove from store
5. Return count of promotions + retirements

Call `evolve_recipes()` periodically — after every 10th run (track via a counter on `RecipeStore`).

**C4. Real composite tool execution**

File: `crates/agentzero-infra/src/tools/dynamic_tool.rs`

Currently `DynamicToolStrategy::Composite` returns a text plan (lines 176-188). To make AUTO-LEARN captured tools useful, add a `tool_resolver` callback:

- Add `tool_resolver: Option<Arc<dyn Fn(&str) -> Option<Arc<dyn Tool>> + Send + Sync>>` to `DynamicTool`
- When resolver is set and strategy is `Composite`, actually invoke each step's tool in sequence:
  ```
  for step in steps:
    tool = resolver(step.tool_name)?
    result = tool.execute(current_input, ctx).await?
    current_input = result.output
  ```
- When resolver is not set, fall back to current plan-description behavior
- Set resolver when loading tools through `RuntimeExecution` (pass a closure over the tool registry)

**C5. Wire into runtime**

File: `crates/agentzero-infra/src/runtime.rs`

- Add `pattern_capture: Option<Arc<PatternCapture>>` to `RuntimeExecution`
- After run: call `capture_if_novel()` (fire-and-forget)
- After run: if recipe_store available, call `evolve_recipes()` every 10th run
- When building dynamic tools from registry, set `tool_resolver` closure that looks up tools by name from the full tool list

---

## Phase D: Two-Stage Tool Selection (MEDIUM)

Pre-filter tools before LLM selection to prevent prompt bloat as dynamic tools grow.

**D1. `TwoStageToolSelector`**

File: `crates/agentzero-infra/src/tool_selection.rs`

```
pub struct TwoStageToolSelector {
    keyword_selector: KeywordToolSelector,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    ai_selector: AiToolSelector,
    stage1_max: usize,  // default 30
    stage2_max: usize,  // default 15
    embedding_cache: Mutex<HashMap<String, Vec<f32>>>,
}
```

**D2. Two-stage flow**

Stage 1: `KeywordToolSelector` narrows to `stage1_max`. If embedding provider available, re-rank by cosine similarity on `name + description` embeddings. Cache embeddings per session.

Stage 2: Pass Stage 1 shortlist (name + description only) to `AiToolSelector`. Returns final `stage2_max` tools.

Graceful degradation: if no embedding provider → keyword only for Stage 1. If LLM fails → return Stage 1 results.

**D3. Config integration**

File: `crates/agentzero-core/src/types.rs`

- Add `TwoStage` variant to `ToolSelectionMode` enum (line 721)
- Add to `Display`, `FromStr` impls

File: `crates/agentzero-infra/src/runtime.rs`

- Wire `"two_stage"` in the selector construction match (around line 435)

---

## Phase E: Tool/Recipe Sharing (LOW)

Export/import tools with quality metadata and related recipes as shareable bundles.

**E1. `ToolBundle` type**

File: `crates/agentzero-infra/src/tools/dynamic_tool.rs`

```
pub struct ToolBundle {
    pub version: u32,
    pub tool: DynamicToolDef,
    pub related_recipes: Vec<ToolRecipe>,
    pub lineage: Vec<String>,
    pub exported_at: u64,
}
```

**E2. Export/import on registries**

- `DynamicToolRegistry::export_bundle(name, recipe_store) -> Result<Option<ToolBundle>>` — tool + recipes mentioning it + lineage chain
- `DynamicToolRegistry::import_bundle(bundle, recipe_store) -> Result<String>` — import tool (zeroed counters) + recipes
- `RecipeStore::export_for_tools(tool_names) -> Vec<ToolRecipe>`
- `RecipeStore::import_recipes(json) -> Result<Vec<String>>`

**E3. Gateway endpoints**

File: `crates/agentzero-gateway/src/handlers.rs`

- `GET /v1/dynamic-tools` — list dynamic tools with quality metadata
- `GET /v1/dynamic-tools/:name/bundle` — export tool bundle
- `POST /v1/dynamic-tools/import` — import tool bundle

Wire in router, add `dynamic_registry` + `recipe_store` to `GatewayState`.

**E4. CLI integration**

File: `crates/agentzero-cli/src/commands/tools.rs` (or extend `tool_create` actions)

- `agentzero tools export <name> --bundle` — export tool bundle to stdout/file
- `agentzero tools import <file>` — import tool bundle from file

---

## Phase F: User Feedback Signals (LOW)

Explicit human quality ratings so users can guide evolution beyond automated success/failure tracking.

**F1. `rate` action on `ToolCreateTool`**

File: `crates/agentzero-infra/src/tools/tool_create.rs`

- Add `"rate"` to the action enum values on `ToolCreateSchema`
- New action: `rate` with `name: String` and `rating: String` (enum: `"good"`, `"bad"`, `"reset"`)
- `good` → increment `total_successes` by 3 (strong positive signal, equivalent to 3 successful runs)
- `bad` → increment `total_failures` by 3 (strong negative signal)
- `reset` → zero all quality counters (fresh start after manual fix)
- Calls `registry.apply_user_rating(name, rating)`

**F2. `apply_user_rating()` on `DynamicToolRegistry`**

File: `crates/agentzero-infra/src/tools/dynamic_tool.rs`

- `apply_user_rating(name: &str, rating: &str) -> Result<()>`
- Applies weighted counter adjustments + persists
- Also sets a `user_rated: bool` flag on `DynamicToolDef` (prevents auto-retirement of user-endorsed tools)

**F3. `user_rated` field on `DynamicToolDef`**

File: `crates/agentzero-infra/src/tools/dynamic_tool.rs`

- Add `#[serde(default)] pub user_rated: bool`
- Tools with `user_rated = true` are never auto-retired by quality filters, and never auto-fixed (user takes ownership)
- AUTO-IMPROVE can still create derived variants of user-rated tools

---

## Implementation Order

**Sprint 74 (this sprint):**
```
Phase A (Telemetry + Quality)          ← foundation, start here
    ↓
Phase B (AUTO-FIX + AUTO-IMPROVE)      ← depends on A3 quality counters
    ↓
Phase F (User Feedback)                ← depends on A3 quality fields, lightweight
```

**Sprint 75 (next sprint):**
```
Phase C (AUTO-LEARN + Recipe Evolution + Composite Execution) ← depends on Sprint 74's telemetry
    ↓
Phase D (Two-Stage Selection)          ← independent, can parallel with C
Phase E (Sharing)                      ← depends on gateway state
```

## Files Modified

| File | Phase | Change |
|------|-------|--------|
| `crates/agentzero-core/src/types.rs` | A1, D3 | `ToolExecutionRecord`, `ToolSelectionMode::TwoStage` |
| `crates/agentzero-core/src/agent.rs` | A2 | Collect execution records in `execute_tool()` |
| `crates/agentzero-core/src/lib.rs` | A1 | Re-export `ToolExecutionRecord` |
| `crates/agentzero-infra/src/tools/dynamic_tool.rs` | A3, E1-E2, F2-F3 | Quality fields, `record_outcome()`, `ToolBundle`, `user_rated`, `apply_user_rating()` |
| `crates/agentzero-infra/src/tool_recipes.rs` | A4, C3, E2 | Quality fields, `record_outcome()`, `evolve_recipes()`, export/import |
| `crates/agentzero-infra/src/runtime.rs` | A5, B3, C4 | Wire telemetry, evolver, pattern capture, recipe evolution |
| `crates/agentzero-infra/src/tool_evolver.rs` | B1-B2 | **New** — AUTO-FIX + AUTO-IMPROVE engine |
| `crates/agentzero-infra/src/pattern_capture.rs` | C1-C2 | **New** — AUTO-LEARN capture engine |
| `crates/agentzero-infra/src/tool_selection.rs` | D1-D2 | `TwoStageToolSelector` |
| `crates/agentzero-gateway/src/handlers.rs` | E3 | Dynamic tool sharing endpoints |
| `crates/agentzero-infra/src/tools/tool_create.rs` | F1 | `rate` action for user feedback |
| `crates/agentzero-cli/src/commands/workflow.rs` | A5 | Quality exclusions in HintedToolSelector |

## Existing Infrastructure to Reuse

- `EmbeddingProvider` trait + `cosine_similarity()` in `crates/agentzero-core/src/embedding.rs` — for D2
- `ApiEmbeddingProvider` in `crates/agentzero-providers/src/embedding.rs` — for D2
- `AiToolSelector` with session cache in `tool_selection.rs` — Stage 2 of D
- `TOOL_CREATE_PROMPT` + `parse_json_from_response()` in `tool_create.rs` — pattern for B2
- `FileAuditSink` JSONL pattern in `audit.rs` — for A5 execution history persistence
- `EncryptedJsonStore` — all persistent store pattern
- `DynamicToolRegistry::export_tool/import_tools` — extend for E

## Verification

1. `cargo clippy --workspace` — 0 warnings
2. `cargo test -p agentzero-core` — `ToolExecutionRecord` serde, agent collects records
3. `cargo test -p agentzero-infra` — quality counters on DynamicToolDef and ToolRecipe, auto-fix with mock provider (strategy parsing), auto-improve candidate detection, pattern capture novelty detection, recipe evolution (promotion + retirement), two-stage selector (keyword → AI shortlist), bundle export/import roundtrip, user rating action
4. Manual E2E: Create dynamic shell tool with typo → run 5+ times → verify auto-fix triggers → repaired strategy
5. Manual E2E: Create dynamic tool → run 10+ times successfully → verify auto-improve triggers → optimized variant registered
6. Manual E2E: Run with 3+ tools in novel combo → verify auto-learn captures composite tool
7. Manual E2E: `tool_create rate my_tool good` → verify counters boosted, `user_rated = true`
8. Manual E2E: Export tool bundle → import on fresh instance → verify tool + recipes imported with zeroed counters

## Pre-Implementation Setup

1. Save full plan to `specs/plans/34-self-evolution-engine.md`
2. Checkout branch `feat/self-evolution-engine`
3. Add Sprint 74 entry to `specs/SPRINT.md` after Sprint 73 (before Backlog at line 2250) — Sprint 74 only (Phases A, B, F)
4. Add Sprint 75 placeholder entry to `specs/SPRINT.md` backlog (Phases C, D, E)
