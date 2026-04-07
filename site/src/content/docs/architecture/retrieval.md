---
title: Retrieval & Memory
description: How AgentZero stores, indexes, and retrieves agent memory and RAG documents.
---

AgentZero's retrieval layer is built around three independent indexes that share a single source of truth. Each index can be enabled independently and gracefully falls back to a brute-force scan when missing or rebuilding.

## What's stored where

| Data | Source of truth | Index | Used for |
|---|---|---|---|
| **Conversation memory** | SQLite (`memory` table) — encrypted via SQLCipher | HNSW (optional) | Semantic recall in agent loop |
| **RAG documents** | Encrypted JSON store (`{data_dir}/rag/index.jsonl`) | Tantivy (when `rag` feature enabled) | BM25 keyword search via `agentzero rag query` |

The source-of-truth stores never go away. Indexes are sidecar artifacts that can be deleted and rebuilt without losing data.

## Conversation memory

Every agent interaction is appended to the `memory` table in the encrypted SQLite database. Rows carry the standard fields you'd expect — `role`, `content`, `created_at`, `conversation_id`, `org_id`, `agent_id`, `expires_at` — plus an optional `embedding` BLOB.

When you call `MemoryStore::append_with_embedding(entry, vec)`, the embedding is stored alongside the row.

### Semantic recall — brute force vs HNSW

The default `MemoryStore::semantic_recall()` implementation does a full table scan: load every row that has an embedding, compute cosine similarity in process, sort, take the top `limit`. That's `O(n)` and fine for hundreds or low thousands of rows.

For larger memory stores, opt into the [HNSW](https://github.com/jean-pierreBoth/hnswlib-rs) approximate nearest neighbor index:

```rust
use agentzero_storage::SqliteMemoryStore;

let mut store = SqliteMemoryStore::open("memory.db", Some(&key))?;
store.enable_hnsw_index("/var/lib/agentzero/hnsw", 384)?;
```

After this call:

- `append_with_embedding()` writes to both SQLite (durable) and HNSW (fast lookup)
- The HNSW index checkpoints to disk every 100 inserts
- `semantic_recall(query, k)` queries HNSW for `(k * 3)` candidate IDs, then resolves the full `MemoryEntry` rows from SQLite preserving HNSW ranking
- Expired rows (`expires_at < now`) are filtered after the HNSW lookup

If the HNSW directory is missing on startup (cold start, fresh deployment, accidental delete), `enable_hnsw_index()` rebuilds it by scanning every embedded row from SQLite. The index is never authoritative — it's always derivable from the source of truth.

The `hnsw_rs` dep is **always** linked into `agentzero-storage` regardless of feature flags, so the API surface is uniform across all builds.

### Hybrid retrieval

For combined keyword + semantic queries, use `MemoryStore::hybrid_recall(query_text, query_embedding, limit)`. The default implementation:

1. Runs `semantic_recall()` over `(limit * 4)` candidates to get the semantic ranking
2. Runs a substring match over the same `recent()` window to get a keyword ranking
3. Fuses both rankings via [reciprocal rank fusion](https://plg.uwaterloo.ca/~gvcormac/cormacksigir09-rrf.pdf) with `k = 60`
4. Deduplicates on a content fingerprint and returns the top `limit`

The `SemanticRecallTool` supports this directly:

```json
{ "query": "what did we decide about the database?", "limit": 5, "mode": "hybrid" }
```

`mode` defaults to `"semantic"` for backward compatibility; pass `"hybrid"` explicitly to opt into the fused ranking.

## RAG document index

The RAG index is a separate store for explicitly-ingested documents (notes, knowledge base articles, code snippets) that you want the agent to retrieve at query time. It is independent from conversation memory.

Documents are persisted twice:

1. **Encrypted JSON store** at `{data_dir}/rag/index.jsonl` — durable source of truth, AES-256-GCM encrypted
2. **Tantivy inverted index** in a sibling `{data_dir}/rag/index.jsonl.tantivy/` directory — fast BM25 query path

When you call `agentzero rag ingest --id <id> --text "..."`, the document is written to both. When you call `agentzero rag query "search terms"`, the Tantivy index is consulted; results are returned with a `score: f32` BM25 relevance value.

If the Tantivy directory is missing or corrupt (newer install, accidental delete, schema drift across versions), the next query rebuilds it from the encrypted store. Migration from the legacy plaintext JSONL format also happens transparently.

The Tantivy dep is gated behind the `rag` feature flag; builds without `rag` skip both Tantivy and the multimodal/document chunking surface.

## Embedding providers

Both semantic recall and hybrid retrieval need an embedding model. AgentZero ships two implementations:

| Provider | When to use |
|---|---|
| `CandleEmbeddingProvider` | Local, in-process. Uses `sentence-transformers/all-MiniLM-L6-v2` (384 dims, ~23 MB). Downloads from HuggingFace on first use, then runs entirely offline. |
| `ApiEmbeddingProvider` | Calls an OpenAI-compatible `/v1/embeddings` endpoint. Use when you want a hosted model or already have credentials configured. |

Both implement the same `EmbeddingProvider` trait. The `SemanticRecallTool` accepts whichever you wire it with.

## Why this design

We deliberately keep the source-of-truth stores separate from the indexes, even though it means writing the same data twice on ingest:

- **Recovery is simple.** If an index is corrupt, delete the directory and the next call rebuilds it from SQLite or the encrypted JSON store. There is no "split brain" recovery path.
- **Indexes are optional.** A small deployment can ignore HNSW and Tantivy entirely; the brute-force fallbacks still work and the API surface is identical.
- **Encryption stays at the storage layer.** The Tantivy index lives on disk in plaintext (Tantivy doesn't encrypt natively), but the *content* it indexes is always reachable from the encrypted store. Compromising the Tantivy directory leaks the keyword index, not the conversation history.

## See also

- [Trait System](/architecture/traits/) — `MemoryStore`, `EmbeddingProvider`, `Tool` interface definitions
- [Config Reference](/config/reference/) — Storage backend configuration
- [Provider Setup](/guides/providers/) — Embedding provider configuration
