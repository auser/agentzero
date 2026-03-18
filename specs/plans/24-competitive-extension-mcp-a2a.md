# Competitive Extension Plan: MCP Server + A2A + Plugin Signing + Semantic Memory + Verticals

## Context

OpenFang (openfang.sh) has surfaced as a direct competitor — same design space (Rust agent OS, ~14 crates, production security focus). Gap analysis reveals five critical areas where AgentZero falls short despite having more tools (48 vs 38) and a mature gateway (42 routes). The highest-leverage gap is **MCP Server Mode**: AgentZero can consume MCP tools but can't expose its own, locking it out of the Claude Desktop / Cursor / Windsurf ecosystem. Additionally, AgentZero lacks **vector embeddings / semantic memory recall** — currently all memory retrieval is recency-based (`ORDER BY id DESC`) with no similarity search.

---

## Step 0: Housekeeping (do when starting implementation)

1. Update `specs/plans/24-competitive-extension-mcp-a2a.md` with this plan (including semantic memory additions)
2. Checkout branch `feat/competitive-extension-mcp-a2a`
3. Update `specs/SPRINT.md` with Sprint 49/50/51 entries (after current Sprint 48) — add semantic memory to Sprint 49
4. Keep `specs/SPRINT.md` up to date throughout implementation

---

## Sprint 49: MCP Server Mode + WASM Plugin Signing + Semantic Memory (parallel tracks)

### Track A: MCP Server Mode (L)

Expose AgentZero's 48 tools as an MCP server so any MCP client can discover and invoke them.

**New files:**
- `crates/agentzero-infra/src/mcp_server.rs` (~500 lines) — Core `McpServer` struct:
  - `initialize` → return server capabilities
  - `tools/list` → map `Tool::name()`, `description()`, `input_schema()` to MCP schema
  - `tools/call` → find tool, call `execute()`, return `CallToolResult`
  - JSON-RPC 2.0 framing (mirror patterns from existing `mcp.rs` client, lines 580-635)
- `crates/agentzero-cli/src/mcp_serve.rs` (~200 lines) — `agentzero mcp-serve` subcommand:
  - stdin/stdout transport (what Claude Desktop expects)
  - Builds `ToolSecurityPolicy` + `default_tools()` from config
- `crates/agentzero-gateway/src/mcp_routes.rs` (~300 lines) — HTTP transport:
  - `POST /mcp/message` — JSON-RPC over HTTP
  - `GET /mcp/sse` — SSE for server notifications (follow existing `sse_events` pattern)
  - Session management via `Sec-MCP-Session-Id` header

**Modified files:**
- `crates/agentzero-gateway/src/router.rs` — add 2 routes
- `crates/agentzero-gateway/src/state.rs` — add `mcp_server: Option<Arc<McpServer>>`
- `crates/agentzero-cli/src/lib.rs` — register `mcp-serve` subcommand
- `crates/agentzero-gateway/src/handlers.rs` — wire up the `tool_execute` stub (line ~2736) for real tool execution (benefits both MCP and REST API)

**Key reuse:** The `Tool` trait already exposes `name()`, `description()`, `input_schema()` which map 1:1 to MCP. The JSON-RPC framing in `mcp.rs` provides the wire format patterns.

### Track B: WASM Plugin Manifest Signing (S)

Ed25519 signing at package time, verification at load time.

**New files:**
- `crates/agentzero-plugins/src/signing.rs` (~200 lines) — `sign_manifest()`, `verify_manifest()`, `generate_keypair()` using `ed25519-dalek`

**Modified files:**
- `crates/agentzero-plugins/src/package.rs` — add `signature: Option<String>` and `signing_key_id: Option<String>` to `PluginManifest` (backward-compatible via `#[serde(default)]`)
- `crates/agentzero-plugins/src/wasm.rs` — check signature before executing; add `require_signed: bool` to `WasmIsolationPolicy` (default `false`)
- `crates/agentzero-plugins/Cargo.toml` — add `ed25519-dalek` dep
- CLI: add `agentzero plugin sign` and `agentzero plugin verify` subcommands

### Track C: Vector Embeddings & Semantic Memory (M)

Add embedding-based semantic recall to the memory system. Currently all recall is recency-based (`ORDER BY id DESC`). This adds an `embedding` BLOB column, an `EmbeddingProvider` trait, and a `semantic_recall()` method to `MemoryStore`.

**Current state:**
- Schema: 9 columns across 5 migrations in `sqlite.rs` (lines 10-140)
- `MemoryEntry` struct: `crates/agentzero-core/src/types.rs` (lines 809-838) — no embedding field
- `MemoryStore` trait: `crates/agentzero-core/src/types.rs` (lines 946-1065) — `recent()` is recency-only
- Three backends: SQLite, pooled SQLite, Turso — all need the migration
- Memory tools (`memory_tools.rs`) use a separate JSON KV store, not the MemoryStore trait
- RAG system (`crates/agentzero-cli/src/rag.rs`) uses substring matching — can be upgraded

**New files:**
- `crates/agentzero-core/src/embedding.rs` (~150 lines) — `EmbeddingProvider` trait:
  ```rust
  #[async_trait]
  pub trait EmbeddingProvider: Send + Sync {
      async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;
      fn dimensions(&self) -> usize;
  }
  ```
  Plus a cosine similarity function for in-process ranking.

- `crates/agentzero-providers/src/embedding.rs` (~200 lines) — Provider-backed embeddings:
  - `ApiEmbeddingProvider` — calls LLM provider embedding endpoints (OpenAI `text-embedding-3-small`, Anthropic, etc.)
  - Reuses existing `HttpTransport` and provider config
  - Feature-gated: `embeddings`

**Modified files:**
- `crates/agentzero-core/src/types.rs`:
  - Add `embedding: Option<Vec<f32>>` to `MemoryEntry` (line ~838)
  - Add to `MemoryStore` trait:
    ```rust
    async fn semantic_recall(&self, query_embedding: &[f32], limit: usize) -> Result<Vec<MemoryEntry>>;
    ```
    Default impl: load all entries with embeddings, compute cosine similarity, return top-k.
  - Add to `MemoryStore` trait:
    ```rust
    async fn append_with_embedding(&self, entry: MemoryEntry, embedding: Vec<f32>) -> Result<()>;
    ```
    Default impl: delegates to `append()` (ignores embedding for backends that don't support it).

- `crates/agentzero-storage/src/memory/sqlite.rs`:
  - Migration v6: `ALTER TABLE memory ADD COLUMN embedding BLOB DEFAULT NULL`
  - `append_with_embedding()` — stores embedding as little-endian `f32` BLOB
  - `semantic_recall()` — loads candidates with non-NULL embeddings, computes cosine similarity in Rust, returns top-k. Respects `org_id`, `agent_id`, `privacy_boundary`, `expires_at` filters.
  - Update `row_to_entry()` (line ~295) to read embedding column

- `crates/agentzero-storage/src/memory/pooled.rs` — same migration + methods
- `crates/agentzero-storage/src/memory/turso.rs` — same migration + methods

- `crates/agentzero-tools/src/memory_tools.rs`:
  - Enhance `MemoryRecallTool` to accept optional `semantic: true` parameter
  - When semantic, use `EmbeddingProvider` to embed the query, then call `semantic_recall()`
  - Falls back to exact key match when `semantic` is false (current behavior)

- `crates/agentzero-storage/Cargo.toml` — no new deps needed (cosine similarity is trivial math on `Vec<f32>`)
- `crates/agentzero-providers/Cargo.toml` — add `embeddings` feature flag

**Design decisions:**
- Embeddings stored as `BLOB` (little-endian `f32` array) — compact, encrypted by SQLCipher
- Cosine similarity computed in Rust (no SQLite extension needed) — load candidates, rank in-process
- `EmbeddingProvider` is separate from `LlmProvider` — simpler trait, single method
- Feature-gated behind `embeddings` — no impact on binary size when disabled
- Backward-compatible: `embedding` column is `DEFAULT NULL`, old entries work fine

---

## Sprint 50: Google A2A Protocol + First Vertical Packages (parallel tracks)

### Track A: A2A Protocol Support (M-L)

Let external agent frameworks discover and invoke AgentZero agents, and let AgentZero call external A2A agents.

**New files:**
- `crates/agentzero-gateway/src/a2a.rs` (~400 lines) — A2A server:
  - `GET /.well-known/agent.json` — Agent Card (capabilities, skills, auth)
  - `POST /a2a` — JSON-RPC: `tasks/send` → `async_submit`, `tasks/get` → `job_status`, `tasks/cancel` → `job_cancel`, `tasks/sendSubscribe` → SSE stream
  - A2A `Part` ↔ AgentZero message format conversion
- `crates/agentzero-core/src/a2a_types.rs` (~200 lines) — `AgentCard`, `Task`, `TaskState`, `Message`, `Part`, `Artifact`
- `crates/agentzero-orchestrator/src/a2a_client.rs` (~300 lines) — `A2aAgentEndpoint` implementing `AgentEndpoint` trait for calling external A2A agents via `ConverseTool`

**Modified files:**
- `crates/agentzero-gateway/src/router.rs` — add 2 routes
- `crates/agentzero-orchestrator/src/swarm.rs` — register `A2aAgentEndpoint` from config
- `crates/agentzero-config/src/model.rs` — add `[a2a]` config section

**Key insight:** `AgentEndpoint::send(message, conversation_id) -> Result<String>` maps directly to A2A `tasks/send`. The existing `ConverseTool` works with any `AgentEndpoint` — external A2A agents become first-class swarm participants with zero changes to the converse tool.

### Track B: Vertical Agent Packages 1-2 (config-only, no code changes)

- **OSINT/Research Analyst** — 5 agents: source-finder, data-collector, fact-checker, analyst, report-writer. Extends existing `research-pipeline` example.
- **Social Media Manager** — 4 agents: content-strategist, copywriter, scheduler, analytics-reporter.

Each package: `agentzero.toml` + README + test script under `examples/`.

---

## Sprint 51: Remaining Verticals + Polish

- **Browser Automation / QA** — 3 agents using `browser_tool`, `screenshot`, `shell`
- **Lead Generation** — 4 agents using `web_search`, `http_request`, `memory_store`
- Integration testing across MCP + A2A + vertical packages
- Documentation updates
- Update `specs/SPRINT.md` with final acceptance criteria and mark completed items

---

## Verification Plan

1. **MCP Server:** Install AgentZero as an MCP server in Claude Desktop config, verify `tools/list` returns all 48 tools, execute a tool via Claude Desktop
2. **Plugin Signing:** Generate keypair, sign a plugin, verify load succeeds with valid sig and fails with tampered sig
3. **Semantic Memory:** Store entries with embeddings, recall by semantic similarity, verify ranking correctness. Test with and without `embeddings` feature flag. Verify migration v6 applies cleanly on existing databases.
4. **A2A:** Fetch `/.well-known/agent.json`, send a task via `POST /a2a`, verify task lifecycle through completion
5. **Verticals:** Run each example's test script end-to-end against a live gateway

## Dependency Graph

```
Step 0:     Save plan + checkout feat/competitive-extension-mcp-a2a + update SPRINT.md

Sprint 49:  MCP Server ──────────────────> done
            Plugin Signing ────> done      (parallel, no deps)
            Semantic Memory ─────────────> done  (parallel, no deps)

Sprint 50:  A2A Protocol ────────────────> done
            Verticals 1-2 ──────────────> done  (parallel, config-only)

Sprint 51:  Verticals 3-4 + polish ─────> done
```

No cross-dependencies between tracks within a sprint. All three Sprint 49 tracks are fully independent. A2A benefits from MCP being done (shared testing patterns) but doesn't depend on it. Vertical packages can leverage semantic memory but don't require it.
