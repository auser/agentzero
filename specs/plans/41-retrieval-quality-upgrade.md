# Plan 41: Retrieval Quality Upgrade — Tantivy BM25 + HNSW Vector Index + Hybrid Search

## Context

AgentZero's memory and RAG retrieval layer has functional foundations (SQLite-backed conversation memory, two embedding providers, document chunking) but the actual *search* is primitive:

- **Semantic recall** (`sqlite.rs:659-686`) loads ALL embeddings from SQLite, computes cosine similarity in-process — O(n) full table scan
- **RAG query** (`rag.rs:49-83`) does case-insensitive substring matching — no tokenization, no ranking, no inverted index
- **No hybrid search** — can't combine keyword + semantic queries

These gaps mean retrieval quality degrades as data grows. Research into [memvid](https://github.com/memvid/memvid) (13.9k stars, Rust, Apache-2.0) confirmed that Tantivy + HNSW + reciprocal rank fusion is the proven pattern for this problem.

## Approach

Three-phase upgrade to the existing storage layer. No new storage backends — enhance SQLite + add sidecar indexes.

### Dependencies

```toml
# New workspace dependencies
tantivy = "0.22"           # Full-text BM25 search engine
hnsw_rs = "0.3"            # Pure Rust HNSW approximate nearest neighbor
```

Both are pure Rust, no C++ build dependencies, actively maintained.

## Phase A: Tantivy BM25 for RAG Full-Text Search (HIGH)

Replace the substring keyword search in `crates/agentzero-cli/src/rag.rs` with a proper Tantivy inverted index.

**Files to modify:**
- `crates/agentzero-cli/Cargo.toml` — add `tantivy` dependency behind `rag` feature flag
- `crates/agentzero-cli/src/rag.rs` — rewrite `query_documents()` and `ingest_document()`

**Tasks:**

- [ ] **Tantivy index schema** — Fields: `id` (stored, indexed), `text` (indexed, stored), `created_at` (fast field for sorting). Index stored at `{data_dir}/rag/tantivy/`
- [ ] **Rewrite `ingest_document()`** — Write to both encrypted JSON store (backward compat) and Tantivy index. `IndexWriter` with auto-commit
- [ ] **Rewrite `query_documents()`** — `QueryParser` with BM25 scoring. Support phrase queries, boolean operators, field-specific search. Return ranked `RagQueryMatch` with relevance score
- [ ] **Index rebuild on cold start** — If Tantivy index is missing but encrypted JSON exists, rebuild from stored documents. One-time migration
- [ ] **Feature gate** — All Tantivy code behind existing `rag` feature flag
- [ ] **Tests** — BM25 ranking returns relevant results first, phrase queries work, empty index returns empty, rebuild from encrypted store matches original, ingest + query roundtrip

## Phase B: HNSW for Semantic Recall (HIGH)

Replace the brute-force O(n) cosine scan in `crates/agentzero-storage/src/memory/sqlite.rs` with an HNSW approximate nearest neighbor index.

**Files to modify:**
- `crates/agentzero-storage/Cargo.toml` — add `hnsw_rs` dependency
- `crates/agentzero-storage/src/memory/sqlite.rs` — rewrite `semantic_recall()`
- `crates/agentzero-storage/src/memory/hnsw_index.rs` — new module for HNSW wrapper
- `crates/agentzero-core/src/types.rs` — add `rebuild_index()` to `MemoryStore` trait (default no-op)

**Tasks:**

- [ ] **`HnswMemoryIndex` wrapper** — Encapsulates `hnsw_rs::Hnsw<f32, DistCosine>`. Methods: `insert(id: i64, embedding: &[f32])`, `search(query: &[f32], limit: usize) -> Vec<(i64, f32)>`, `save(path)`, `load(path)`, `len()`
- [ ] **Index persistence** — Serialize HNSW to `{data_dir}/memory/hnsw.index` via `hnsw_rs` built-in dump/load. Checkpoint after every N inserts (configurable, default 100)
- [ ] **Wire into `SqliteMemoryStore`** — `append_with_embedding()` inserts into both SQLite (durability) and HNSW (fast search). `semantic_recall()` queries HNSW for candidate IDs, then fetches full `MemoryEntry` rows from SQLite by ID
- [ ] **Cold start rebuild** — If HNSW index file is missing, scan SQLite `WHERE embedding IS NOT NULL` and rebuild. Log progress for large datasets
- [ ] **`rebuild_index()` trait method** — Forces HNSW rebuild from SQLite. Useful after migrations or corruption
- [ ] **Multi-tenancy consideration** — Single HNSW index, post-filter by `org_id`/`agent_id` after candidate retrieval. Over-fetch by 3x to account for filtering. (Separate indexes per org/agent is a future optimization if needed)
- [ ] **Tests** — HNSW returns same top results as brute-force for small datasets, insert + search roundtrip, persistence save/load, cold start rebuild matches, over-fetch handles org filtering

## Phase C: Hybrid Search — Reciprocal Rank Fusion (MEDIUM)

Combine BM25 keyword results with HNSW semantic results using reciprocal rank fusion (RRF).

**Files to modify:**
- `crates/agentzero-core/src/types.rs` — add `hybrid_recall()` to `MemoryStore` trait
- `crates/agentzero-core/src/search.rs` — new module for rank fusion
- `crates/agentzero-storage/src/memory/sqlite.rs` — implement `hybrid_recall()`
- `crates/agentzero-tools/src/semantic_recall.rs` — update tool to support hybrid mode

**Tasks:**

- [ ] **`reciprocal_rank_fusion()`** — `fn rrf(rankings: &[Vec<(i64, f32)>], k: usize) -> Vec<(i64, f32)>`. Standard RRF formula: `score = sum(1 / (k + rank_i))` with k=60 (standard constant). Returns merged, deduplicated, re-ranked results
- [ ] **`hybrid_recall()` trait method** — `async fn hybrid_recall(&self, query_text: &str, query_embedding: &[f32], limit: usize) -> Result<Vec<MemoryEntry>>`. Default implementation: run keyword search + semantic search independently, fuse with RRF. Requires storing content in a searchable form (Tantivy index of memory content)
- [ ] **Memory content indexing** — Add Tantivy index for `memory` table content alongside HNSW. Index `content` field on `append()`. Store at `{data_dir}/memory/tantivy/`
- [ ] **`SemanticRecallTool` upgrade** — Add optional `mode` parameter: `"semantic"` (default, backward compat), `"keyword"`, `"hybrid"`. Hybrid mode calls `hybrid_recall()`
- [ ] **Feature gate** — Hybrid search requires both `rag` (Tantivy) feature. Graceful degradation: if Tantivy not available, fall back to semantic-only
- [ ] **Tests** — RRF produces correct merged ranking, hybrid outperforms either individual method on test dataset, fallback to semantic-only when Tantivy unavailable, tool mode parameter works

## Phase D: Dependency & Build Validation (LOW)

- [ ] **Workspace Cargo.toml** — Add `tantivy` and `hnsw_rs` to workspace dependencies
- [ ] **Feature propagation** — Ensure `tantivy` only pulled in when `rag` feature is active. `hnsw_rs` is always available in `agentzero-storage` (vector search is core, not optional)
- [ ] **Binary size check** — Verify `agentzero-lite` binary is not bloated by new deps (it shouldn't use `rag` feature)
- [ ] **Clippy clean** — 0 warnings across all feature combinations
- [ ] **All existing tests pass** — No regressions

## Acceptance Criteria

- [ ] `cargo clippy --all-targets` — 0 warnings
- [ ] All workspace tests pass (existing + new)
- [ ] RAG `query_documents()` returns BM25-ranked results with relevance scores
- [ ] `semantic_recall()` uses HNSW index — sub-millisecond for 100k+ entries
- [ ] `hybrid_recall()` combines keyword + semantic via RRF
- [ ] HNSW index persists to disk and survives restart
- [ ] Cold start rebuilds indexes from SQLite/encrypted store automatically
- [ ] `SemanticRecallTool` supports `mode: "hybrid"` parameter
- [ ] No changes to `EmbeddingProvider` trait or external API
- [ ] `agentzero-lite` binary unaffected (no size regression)

## Key Files Reference

| File | Current Role | Sprint Changes |
|------|-------------|----------------|
| `crates/agentzero-storage/src/memory/sqlite.rs` | Brute-force vector recall | Wire HNSW, add content indexing |
| `crates/agentzero-cli/src/rag.rs` | Substring keyword search | Replace with Tantivy BM25 |
| `crates/agentzero-core/src/embedding.rs` | Embedding trait + cosine | Keep as-is |
| `crates/agentzero-core/src/types.rs` | MemoryStore trait | Add `hybrid_recall()`, `rebuild_index()` |
| `crates/agentzero-tools/src/semantic_recall.rs` | Tool wrapper | Add hybrid mode parameter |
| `crates/agentzero-tools/src/chunk_document.rs` | Document chunking | No changes (upstream of indexing) |

## Estimated Effort

- Phase A (Tantivy BM25): 1 sprint
- Phase B (HNSW): 1 sprint
- Phase C (Hybrid RRF): 0.5 sprint
- Phase D (Validation): 0.5 sprint

Can be done as a single large sprint or split across two focused sprints.

## Inspiration

- [memvid](https://github.com/memvid/memvid) — Tantivy + HNSW + hybrid search in a single-file Rust memory system
- RRF paper: "Reciprocal Rank Fusion outperforms Condorcet and individual Rank Learning Methods" (Cormack et al., 2009)
