# Plan: KV Cache Reuse Across Conversation Turns

## Context

The Candle provider reprocesses the entire prompt from scratch on every `generate()` call — system prompt, tool definitions, all prior messages. In a 10-turn conversation with 2-4K tokens of system+tools, that's ~30K tokens of wasted recomputation.

Candle's `quantized_llama::ModelWeights` has internal KV cache per layer. When `forward(tokens, index_pos)` is called with `index_pos > 0`, new K/V entries are concatenated with the cached ones. When `index_pos == 0`, the cache resets. The cache persists in the `ModelWeights` instance between calls.

The optimization: track what's in the KV cache (prompt + generated tokens), find the common prefix with the next prompt, and only process the new suffix tokens.

## Key Insight

After generation, the KV cache contains `[prompt_tokens + generated_tokens]`. On the next turn, the new prompt includes the previous conversation — if the generated output matches (same tokenization), the prefix extends through the assistant's response. Even if it only matches through the system prompt + tools, that's 2-4K tokens saved per turn.

Cache miss (prefix diverges) falls back to `index_pos=0` — same as current behavior. No correctness risk.

## Changes (single file: `candle_provider.rs`)

### 1. Add `cached_tokens` to `LoadedModel`

```rust
struct LoadedModel {
    weights: ModelWeights,
    tokenizer: Tokenizer,
    device: Device,
    template: local_tools::ChatTemplate,
    cached_tokens: Vec<u32>,  // ALL tokens currently in KV cache (prompt + generated)
}
```

Initialize as `Vec::new()` in `ensure_loaded()`.

### 2. Add `find_common_prefix_len` helper

```rust
fn find_common_prefix_len(cached: &[u32], new_tokens: &[u32]) -> usize {
    cached.iter().zip(new_tokens).take_while(|(a, b)| a == b).count()
}
```

### 3. Add `feed_prompt_cached` helper

Replaces the manual `forward(tokens, 0)` pattern in all three generate methods.

Logic:
- If `prefix_len == cached_tokens.len()` and new tokens extend it → cache hit, feed only suffix with `index_pos = prefix_len`
- Otherwise → cache miss, feed all tokens with `index_pos = 0`
- After feeding, set `cached_tokens = tokens.to_vec()`
- Returns `(logits, total_prompt_len)` for the autoregressive loop

### 4. Track generated tokens in the autoregressive loop

In all three loops (`generate`, `generate_streaming`, `generate_constrained`), push each accepted `next_token` to `loaded.cached_tokens`. This keeps `cached_tokens` in sync with the actual KV cache state.

EOS token is NOT pushed (we break before pushing), which is correct — EOS is not part of the next prompt.

### 5. Apply to all three generate methods

| Method | Prompt feed lines | Loop body |
|---|---|---|
| `generate()` | Replace lines ~290-293 with `feed_prompt_cached()` | Add `cached_tokens.push(next_token)` |
| `generate_streaming()` | Replace lines ~387-391 | Same |
| `generate_constrained()` | Replace lines ~560-564 | Same |

`generate_constrained` uses a completely different prompt (retry), so the prefix match will naturally fail → full reprocess. After it completes, `cached_tokens` reflects the constrained prompt, which won't match the next normal turn. This is self-correcting.

## Correctness

- **Rotary embeddings**: `forward(suffix, index_pos)` applies rotary at correct positions. Cached entries already have their rotary baked in.
- **Attention mask**: Causal mask covers only the suffix tokens. Attention to cached prefix is via KV concatenation (no mask needed — all prefix tokens visible). Correct for causal attention.
- **Tokenization consistency**: BPE tokenizers may produce different tokenizations at boundaries. If so, the prefix match fails and we fall back to full reprocess. No correctness issue.
- **Memory**: `Vec<u32>` for 8K context = 32KB. Negligible.

## Verification

1. `cargo clippy --features candle-metal` — 0 warnings
2. `cargo test --workspace` — all tests pass
3. Unit test for `find_common_prefix_len` with edge cases
4. Manual test: multi-turn conversation, check debug logs for "KV cache hit" messages
5. Benchmark: measure prompt processing time on turn 1 vs turn 5 of a conversation
