# Plan: Local LLM Ecosystem — Candle Provider + Rust LLM Libraries

## Context

AgentZero now has a Candle-based local LLM provider (implemented earlier in this session). This plan captures the research findings from evaluating the broader Rust LLM ecosystem and recommends next steps.

## What's Already Done (this session)

- `CandleProvider` with GGUF loading, streaming, tool call parsing
- `LocalModelConfig` in TOML `[local]` section
- Shared `local_tools` module (ChatML formatting, fuzzy JSON repair)
- `estimate_tokens()` on Provider trait
- Feature chain: `bin/agentzero` → `cli` → `infra` → `providers` (all have `candle` feature)
- Model catalog, doctor, onboard, all docs updated

## Ecosystem Research: Full Assessment

### Tier 1 — Adopt (high value, low risk)

#### outlines-core (dottxt-ai) — Constrained decoding
- **What:** Converts JSON schema → regex → FSA index. At each decoding step, masks logits to only allow valid tokens. Guarantees 100% valid JSON output from any model.
- **Stars:** 278 | **Version:** 0.2.14 | **License:** Apache-2.0
- **Why:** Eliminates malformed tool calls at the source — no more fuzzy JSON repair. Same approach used by HuggingFace TGI, vLLM, SGLang.
- **Integration:** Pure Rust, uses same `tokenizers` crate we already have. Build `Index` from tool call schema before generation, call `allowed_tokens(state)` during each Candle generation step to mask logits.
- **Effort:** Medium — requires modifying the Candle generation loop to apply token masks

#### text-splitter (benbrandt) — Document chunking
- **What:** Splits text into semantically meaningful chunks with configurable max size (chars or tokens). Handles plain text, Markdown, and code (tree-sitter).
- **Stars:** 585 | **Downloads:** 1.12M | **Version:** 0.29.3 | **License:** MIT
- **Why:** Essential for RAG. Token-aware sizing matches target model's tokenizer. Code-aware splitting via tree-sitter is directly useful for codebase analysis.
- **Integration:** Pure sync library, zero architectural impact. Add as dependency, use in RAG tools.
- **Effort:** Low — just add the dependency and wire into RAG pipeline

#### ~~tiktoken-rs — OpenAI token counting~~ DROPPED
- **Stars:** 376 | **Version:** 0.9.x | **License:** MIT
- **Why dropped:** Embeds 8.6MB of BPE vocabulary data unconditionally (no feature gates). Binary impact of 4.5-6.5MB alone exceeds the embedded target. No way to select only the vocabularies you need without forking.
- **Alternative:** Use `text.len() / 4` heuristic for cloud providers (sufficient for context overflow prevention). The Candle provider already has accurate token counting via the HF tokenizer.

### Tier 2 — Evaluate (medium value, some risk)

#### mistral.rs — Full inference engine on Candle
- **What:** High-performance inference engine built on Candle. PagedAttention, continuous batching, speculative decoding, 20+ model architectures, LoRA, constrained decoding via llguidance, vision/audio models. Library-embeddable (not just a server).
- **Stars:** 6,739 | **Version:** 0.7.0 | **License:** MIT
- **Why:** Eliminates months of work on scheduling, batching, quantization. Already integrates llguidance for constrained decoding. Supports structured generation via `generate_structured::<T>()`.
- **Blocker:** Enormous binary size (50-100MB+), no feature gates to exclude model architectures. Candle version pinned to specific git rev (version conflict risk). MSRV 1.88.
- **Path forward:** Could ship as a separate optional binary (`agentzero-inference`) that the main agent talks to over localhost, keeping the 4.5MB embedded target clean. Or wait for upstream to add model-architecture feature gates.
- **Key patterns worth adopting even without the dep:** Auto-detect model architecture from HF model ID, per-layer YAML topology for mixed quantization, `mistralrs tune` for auto-benchmarking hardware.

#### llguidance (Microsoft) — Advanced constrained decoding
- **What:** Full CFG + JSON schema constrained decoding. Powers OpenAI Structured Outputs. ~50us/token.
- **Stars:** 719 | **Version:** 1.0.0 | **License:** MIT
- **Why:** More powerful than outlines-core (full context-free grammars, not just regex). Already integrated in mistral.rs, llama.cpp, vLLM.
- **Trade-off:** More complex dependency than outlines-core but far more capable. Consider as upgrade path after outlines-core proves the pattern works.

#### Vector search for RAG
- **qdrant** (29.9k stars) — Production-grade but **requires a separate server** (not embeddable). The `qdrant-client` crate is just a gRPC/REST client. Breaks AgentZero's self-contained philosophy.
- **Better alternatives for AgentZero:**
  - **lancedb** — Embeddable columnar vector DB in Rust. No server needed. Supports hybrid search. Used by LangChain.
  - **SQLite FTS5 + sqlite-vss** — Vector search extension for SQLite. Keeps everything in the existing SQLite store.
  - **usearch** — Single-file embeddable vector index (C with Rust bindings). Tiny footprint.
- **Recommendation:** Evaluate lancedb or sqlite-vss for RAG, not qdrant. Self-contained > external service.

#### spider — Web crawling
- **What:** Full-site web crawling (not just single-page fetch).
- **Stars:** 2.4k | **License:** MIT
- **Why:** Powers "ingest entire docs site" for RAG knowledge base building. We only have single-page `web_fetch` today.
- **Trade-off:** Heavy dependency. Consider only if RAG ingestion becomes a priority.

### Tier 3 — Reference only (learn from, don't depend on)

#### rig (0xPlaygrounds) — LLM framework
- **Stars:** 6,675 | 20+ provider adapters, structured extraction, RAG
- **Why not adopt:** Opinionated framework that wants to own the agent loop. Conflicts with AgentZero's architecture.
- **What we can learn:**
  1. **Structured extraction pattern** — `agent.extract::<MyType>(prompt)` deserializes LLM output directly into Rust types. We could add `Provider::complete_structured::<T>()` that combines constrained decoding (outlines-core) with serde deserialization.
  2. **Tool definition via derive macro** — `#[derive(Tool)]` auto-generates tool schema from struct fields + doc comments. Cleaner than our manual `ToolDefinition` construction. Could be added to `agentzero-macros`.
  3. **Provider companion crates** — Each provider is a separate small crate (`rig-openai`, `rig-anthropic`), keeping core lean. We already do this with feature gates, but their adapter pattern for new providers is cleaner.
  4. **Preamble chaining** — `agent.preamble("system prompt").context(dynamic_context).tool(my_tool).build()` — builder ergonomics worth studying for our `AgentConfig`.

#### swiftide (bosun-ai) — RAG pipeline
- **Stars:** 680 | Streaming indexing pipeline, tree-sitter code chunking
- **Why not adopt:** Low adoption (1.8k recent downloads), API unstable, agent framework overlaps
- **Learn from:** Pipeline architecture, tree-sitter integration pattern

#### kalosm (floneum) — Local AI framework
- **Stars:** 2,164 | Candle-based, structured generation via derive macros, RAG, Whisper
- **Why not adopt:** Heavyweight, overlaps with provider architecture
- **What we can learn:**
  1. **Constrained decoding + Candle integration proof** — Kalosm proves this works in practice. Their generation loop applies grammar masks to Candle logits at each step. This validates our outlines-core integration plan.
  2. **`#[derive(Parse, Schema)]` macro** — Annotate a Rust struct and the LLM is forced to produce a valid instance. The macro generates both the JSON schema AND the grammar constraint. We should build the same pattern for tool call arguments.
  3. **Single-file GGUF loading** — Kalosm auto-detects model architecture from GGUF metadata (no separate config needed). Our `CandleProvider` currently relies on the user specifying the right model type.
  4. **RAG with BERT embeddings** — Uses `fastembed-rs` for local embedding generation (no API call). Worth considering for offline RAG — the embeddings run on the same device as the LLM.
  5. **39 tok/s on M2 with Mistral 7B** — Performance benchmark for Candle-based inference on Apple Silicon. Gives us a target to measure against.

### Tier 4 — Skip

| Project | Why skip |
|---------|----------|
| **rustformers/llm** | Archived. Candle supersedes it |
| **llm-chain** | Stale 17 months. Our LlmLayer pipeline is better |
| **smartgpt, rllama, indexify, memex** | Dead/abandoned |
| **motorhead** | Requires Redis. AgentZero is self-contained |
| **rust-bert** | Heavy libtorch dep. Only if we need NER/sentiment without LLM |
| **postgresml, pgvecto.rs** | Only if migrating to Postgres |
| **dust, aichat** | Full apps, not embeddable libraries |
| **polars** | Great library but not core to agent infrastructure |
| **whichlang** | Cute but niche (language detection) |
| **xgrammar-rs** | C++ FFI, too early/fragile |
| **KBNF** | Stale, unclear license. Used inside kalosm |

## Recommended Implementation Order

1. **outlines-core** — Constrained decoding for tool calls. Biggest pain point, highest impact. ~300KB marginal cost (tokenizers already paid for by Candle). Only behind `candle` feature.
2. **text-splitter** (markdown + code features only) — RAG chunking. ~600KB-1MB. Behind `rag` feature.
3. **llguidance** — Upgrade path if outlines-core regex isn't enough for complex schemas.
4. **mistral.rs** — Desktop/server inference upgrade (when binary size is addressed).

### Binary size budget

| Feature combination | Est. additional size |
|---|---|
| `candle` (already shipped) | ~8-10MB (Candle + tokenizers + GGUF) |
| `candle` + outlines-core | ~300KB marginal (regex automata, tokenizers shared) |
| `rag` + text-splitter (markdown+code) | ~600KB-1MB |
| Default (no local features) | 0 — all feature-gated |

## Constrained Decoding Integration (outlines-core)

This is the most impactful next step. The pattern:

1. Before generation: `json_schema::regex_from_str(&tool_call_schema)` → regex
2. Build `Index::new(&regex, &vocabulary)` from model vocabulary
3. During each Candle generation step: `index.allowed_tokens(&state)` → mask invalid logits
4. After sampling: `index.next_state(&state, token_id)`
5. Result: 100% valid JSON tool calls, even from 3B quantized models

**Files to modify:**
- `crates/agentzero-providers/Cargo.toml` — add `outlines-core` optional dep
- `crates/agentzero-providers/src/candle_provider.rs` — apply mask in generation loop
- `crates/agentzero-providers/src/local_tools.rs` — build tool call JSON schema for constraining

## Pre-Implementation Steps

1. **Create branch:** `git checkout -b feat/local-llm-ecosystem`
2. **Save plan:** Copy this file to `specs/plans/35-local-llm-ecosystem.md`
3. **Update SPRINT.md:** Add Sprint 76 entry before the Backlog section:

```markdown
## Sprint 76: Local LLM Ecosystem — Constrained Decoding, Chat Templates, RAG Pipeline

**Goal:** Make local LLMs production-grade: guaranteed valid tool calls via constrained decoding (outlines-core), multi-model chat template support (Llama 3/Mistral/Gemma), semantic document chunking for RAG (text-splitter), and local embedding generation via Candle. Builds on the Candle provider shipped in Sprint 75.

**Baseline:** Sprint 75 complete. Candle provider with GGUF loading, streaming, fuzzy JSON repair, `[local]` config, `estimate_tokens()`, shared `local_tools` module. 709+ tests passing, 0 clippy warnings.

**Plan:** `specs/plans/35-local-llm-ecosystem.md`

---

### Phase A: Constrained Decoding via outlines-core (HIGH)

Guarantee valid JSON tool calls from any local model by masking invalid tokens during generation.

- [ ] **Add `outlines-core` dependency** — Feature-gated behind `candle` feature in `agentzero-providers`. Pure Rust, uses same `tokenizers` crate.
- [ ] **Build tool call JSON schema** — In `local_tools.rs`, generate a JSON schema for the `{"name": "...", "arguments": {...}}` format from `ToolDefinition` list.
- [ ] **`ConstrainedDecoder` struct** — Wraps `outlines_core::Index`. `new(schema, vocabulary)` builds the FSA. `allowed_tokens(state)` returns valid token IDs. `next_state(state, token_id)` advances.
- [ ] **Integrate into CandleProvider generation loop** — After computing logits, mask out tokens not in `allowed_tokens()`. Apply mask before sampling. Advance state after sampling. Only activate when tools are present (plain chat is unconstrained).
- [ ] **Tests** — Verify schema→regex→index pipeline. Verify masking produces valid JSON. Verify unconstrained mode still works for chat.

### Phase B: Chat Template Support (HIGH)

Support Llama 3, Mistral, Gemma, and other chat formats beyond hardcoded ChatML.

- [ ] **`ChatTemplate` enum** — In `local_tools.rs`: `ChatML` (current Qwen), `Llama3`, `Mistral`, `Gemma`, `Custom(String)`. Each variant knows its BOS/EOS tokens, role markers, and tool call format.
- [ ] **Auto-detect from GGUF metadata** — Parse `tokenizer.chat_template` from GGUF file metadata (Candle's `gguf_file::Content` exposes this). Fall back to ChatML if not found.
- [ ] **`format_prompt(template, messages, tools)` function** — Replaces the current `format_chatml_prompt`. Dispatches to the correct formatter based on detected template.
- [ ] **Config override** — Add `chat_template = "auto" | "chatml" | "llama3" | "mistral"` to `[local]` config for manual override.
- [ ] **Update EOS token resolution** — `resolve_eos_token()` in `candle_provider.rs` should use the detected template's EOS token instead of trying a hardcoded list.
- [ ] **Tests** — Format messages in each template, verify correct role markers and structure.

**Files:**
- `crates/agentzero-providers/src/local_tools.rs` — `ChatTemplate` enum + formatters
- `crates/agentzero-providers/src/candle_provider.rs` — auto-detect + use template
- `crates/agentzero-config/src/model.rs` — `chat_template` field on `LocalModelConfig`

### Phase C: RAG Document Chunking via text-splitter (MEDIUM)

Add semantic document chunking for the RAG pipeline.

- [ ] **Add `text-splitter` dependency** — Workspace dep with `markdown`, `code`, `tokenizers` features. Feature-gated behind `rag` in `agentzero-tools`.
- [ ] **`chunk_document` tool** — New tool that accepts file path + max chunk size (tokens), returns semantically split chunks. Uses `MarkdownSplitter` for .md, `CodeSplitter` for code files, `TextSplitter` for everything else.
- [ ] **Token-aware sizing** — Configure splitter with the active model's tokenizer for accurate chunk sizing.
- [ ] **Tests** — Chunk markdown file, verify semantic boundaries. Chunk code file, verify syntax-aware splits.

### Phase D: Local Embeddings via Candle (MEDIUM)

Generate vector embeddings locally for fully offline RAG — no API calls needed.

- [ ] **`CandleEmbeddingProvider`** — New struct in `crates/agentzero-providers/src/candle_embedding.rs`. Loads a small BERT/MiniLM model via Candle (same framework we already use — no ONNX Runtime, no new heavy deps). Implements the existing `EmbeddingProvider` trait.
- [ ] **Model:** Default to `sentence-transformers/all-MiniLM-L6-v2` (22MB, 384-dim). Auto-download from HF Hub via `hf-hub` (already a dependency). GGUF or safetensors format.
- [ ] **`embed(text) -> Vec<f32>`** — Tokenize input, run BERT forward pass, mean-pool token embeddings, L2-normalize.
- [ ] **Feature gate** — Behind `candle` feature (reuses Candle + tokenizers, no new deps).
- [ ] **Wire into runtime** — When `candle` feature is active and no external embedding provider is configured, use `CandleEmbeddingProvider` as default.
- [ ] **Tests** — Embed two similar sentences, verify cosine similarity > 0.8. Embed two unrelated sentences, verify < 0.5.

**Why Candle over fastembed-rs:** fastembed uses ONNX Runtime (50-150MB shared lib). We already have Candle — loading a BERT model through it adds ~22MB model weight but zero new binary dependencies. Same approach kalosm uses.

**Files:**
- New: `crates/agentzero-providers/src/candle_embedding.rs`
- `crates/agentzero-providers/src/lib.rs` — register module
- `crates/agentzero-infra/src/runtime.rs` — wire as default embedding provider

### Acceptance Criteria

- [ ] 3B quantized model produces 100% valid tool call JSON (no fuzzy repair needed)
- [ ] Non-Qwen models (Llama 3, Mistral) work with correct chat templates
- [ ] Documents chunked with semantic awareness respecting chunk size limits
- [ ] Embeddings generated locally without API calls
- [ ] 0 clippy warnings, all existing tests pass
- [ ] Default binary (no features) unaffected — all new deps behind feature gates
```

## Verification

1. Constrained decoding: generate tool calls from 3B model with schema constraint, verify 100% valid JSON
2. text-splitter: chunk a markdown file with token-aware sizing, verify chunks fit context window
3. tiktoken-rs: estimate tokens for GPT-4o prompt, compare with OpenAI's tokenizer API
