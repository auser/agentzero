# Dependency Reduction Gap Analysis

## Context

AgentZero currently pulls **606 unique crates** for a default build, producing a **26MB binary**. Many of these are transitive deps from a handful of heavy top-level crates. This analysis identifies what can be rewritten lighter, replaced, or removed — organized by impact (transitive deps eliminated) and effort.

---

## Pre-Work: Free Wins (Before Rewriting Anything)

### 0a. Deduplicate version splits

`cargo tree --duplicates` reveals **27 crates compiled twice** due to version splits:

| Crate | Versions | Cause |
|-------|----------|-------|
| rand | 0.8, 0.9, 0.10 | different transitive consumers |
| rand_core | 0.6, 0.9, 0.10 | same |
| getrandom | 0.2, 0.3, 0.4 | same |
| hashbrown | 0.14, 0.15, 0.16 | same |
| thiserror | 1.x, 2.x | workspace uses v2, transitive pulls v1 |
| indicatif | 0.17, 0.18 | workspace declares 0.17, something pulls 0.18 |
| zip | 2.x, 7.x | likely a transitive + direct conflict |
| wast | 35.x, 246.x | wasmi vs wasmtime |
| chacha20 | 0.9, 0.10 | snow vs chacha20poly1305 |
| base64 | 0.13, 0.22 | old transitive consumer |
| console | 0.15, 0.16 | inquire vs ratatui |

**Action:** Pin transitive deps in `[patch]` or upgrade direct deps to align versions. Each dedup saves a full compile of that crate + its proc-macros. Estimated savings: **~15-20 fewer compiled crate instances**, noticeable compile time improvement.

### 0b. Run `cargo udeps` for dead dependencies

After the 46→16 crate consolidation, some deps may be declared but unused. `cargo +nightly udeps` will flag them. Free removal.

### 0c. Slim tokio feature sets per crate

The workspace declares tokio with `macros, rt-multi-thread, time, fs, process, sync, io-util, io-std, signal, net` — every crate inherits all of these. Most crates only need `rt, macros, sync, time`. While tokio features don't add many *new* deps, they do increase compile time. Consider splitting:
- Workspace default: `rt, macros, sync, time`  
- Binary crates only: add `rt-multi-thread, fs, process, signal, net, io-util, io-std`

### 0d. Binary size quick wins (no code changes)

Before rewriting deps, apply `Cargo.toml` profile settings:
```toml
[profile.release]
lto = "thin"       # or "fat" for max savings
strip = true       # strip debug symbols
codegen-units = 1  # better optimization
```
This alone can drop the 26MB binary to ~18-20MB.

---

## Tier 1: High Impact, Moderate Effort

### 1. `reqwest` (170 transitive deps) → `hyper` + manual client

**What it does:** HTTP client for LLM providers, channel integrations, plugin downloads, webhooks.

**What you actually use:** `GET`/`POST` with JSON bodies, bearer auth headers, streaming responses (`stream` feature), multipart upload (voice transcription only).

**Replacement:** Write a thin HTTP client on `hyper` (which axum already pulls in). You already have `hyper` and `http` in the tree via axum. A ~300-line wrapper around `hyper::Client` with:
- JSON request/response helpers (serde_json already present)
- Header builder
- Streaming response body iterator
- One multipart encoder for the voice channel

**Savings:** ~150 unique crates removed (hyper, http, h2, tower already stay via axum). This is the single highest-impact change.

**Risk:** TLS configuration. You'd need to wire rustls (already in tree via axum-server) directly. Cookie handling if any channel needs it (currently none do).

**Middle path:** Instead of hand-rolling on hyper, consider `ureq` (~15 deps, blocking) with `tokio::task::spawn_blocking`, or `isahc` (~30 deps). These trade fewer savings for far less maintenance. `ureq` alone would cut ~155 deps to ~15 — still a massive win with almost no custom code.

---

### 2. `scraper` (55 transitive deps) → hand-rolled HTML text extractor

**What it does:** CSS selector-based HTML text extraction in `html_extract.rs` — one tool, ~80 lines of logic.

**What you actually use:** `Html::parse_document`, `Selector::parse`, iterate matches, `.text().collect()`.

**Replacement:** A simple tag-stripping + CSS-selector-subset parser. Since the tool is used by LLM agents (not browsers), you only need:
- Tag-aware text extraction (strip tags, preserve text nodes)
- Basic selector support: element name, `.class`, `#id`, nesting

A ~200-line implementation covers this. Alternatively, use `lol_html` (streaming, 5 deps) or just regex-based extraction since the LLM can handle imperfect HTML.

**Savings:** ~50 unique crates (selectors, html5ever, markup5ever, string_cache, tendril, etc.)

---

### 3. `config` crate (28 transitive deps) → direct TOML + env loading

**What it does:** Layered config: TOML file + `AGENTZERO__` env vars → deserialize into `AgentZeroConfig`.

**What you actually use:** `Config::builder().add_source(File).add_source(Environment).build().try_deserialize()` — one call site in `loader.rs`.

**Replacement:** You already have `toml` and `dotenvy`. Replace with:
1. `std::fs::read_to_string` + `toml::from_str::<AgentZeroConfig>()`
2. Walk env vars with `AGENTZERO__` prefix, split on `__`, overlay onto the struct
3. ~100 lines of code

**Savings:** ~25 unique crates (config pulls in its own serde machinery, nom parsers, etc.)

---

### 4. `snow` (45 transitive deps) → direct Noise protocol on existing crypto

**What it does:** Noise protocol handshakes for privacy-mode transport.

**What you actually use:** `snow::Builder` to create Noise_XX handshakes with x25519 + ChaCha20Poly1305.

**Replacement:** You already have `x25519-dalek` and `chacha20poly1305`. A Noise_XX handshake is a well-specified 3-message pattern. Hand-rolling it on your existing crypto primitives is ~300-400 lines and eliminates snow's entire dep tree (which duplicates crypto you already have).

**Savings:** ~40 unique crates (snow pulls its own curve25519, aes-gcm, blake2, etc.)

**Risk:** Moderate — Noise protocol is security-critical. Needs thorough testing against test vectors.

---

### 5. `tower-http` (51 transitive deps) → inline middleware

**What it does:** Compression (gzip), request size limits, timeouts on the gateway.

**What you actually use:** `CompressionLayer`, `RequestBodyLimitLayer`, `TimeoutLayer`.

**Replacement:**
- **Compression:** Use `flate2` (already pulled transitively) or `miniz_oxide` with a 30-line axum middleware
- **Body limit:** ~15-line middleware checking `Content-Length` header
- **Timeout:** `tokio::time::timeout` wrapper, ~10 lines

**Savings:** ~30 unique crates (tower-http pulls in http-body, bitflags, pin-project, etc. — though some overlap with axum)

---

## Tier 2: Moderate Impact, Low-Moderate Effort

### 6. `rust-embed` (41 transitive deps) → `include_bytes!` / build script

**What it does:** Embeds static UI files into the binary for config-ui and gateway.

**What you actually use:** Serve embedded files with content-type detection.

**Replacement:** A build script that generates `include_bytes!` calls + a static `&[(&str, &[u8], &str)]` table. Content-type detection is ~30 lines of extension matching. Total: ~80 lines of build script + ~40 lines of serving code.

**Savings:** ~35 unique crates

---

### 7. `metrics` + `metrics-exporter-prometheus` (43 transitive deps) → hand-rolled Prometheus exporter

**What it does:** Counter/gauge/histogram metrics exposed as Prometheus text format.

**What you actually use:** Basic counters and histograms on the gateway.

**Replacement:** Prometheus text format is trivial (`# TYPE foo counter\nfoo 42\n`). A ~150-line metrics registry with `AtomicU64` counters and a text serializer replaces both crates.

**Savings:** ~40 unique crates

---

### 8. `ratatui` + `crossterm` (58 transitive deps) → minimal TUI or remove

**What it does:** Terminal dashboard UI.

**Assessment:** This is already feature-gated (`tui`). If binary size is the concern, disable the feature. If you want to keep it but lighter, `crossterm` alone with raw ANSI escape codes can render a basic dashboard without ratatui's layout engine.

**Savings:** ~55 unique crates (when feature-disabled, already 0)

---

### 9. `dashmap` (24 transitive deps) → `std::sync::RwLock<HashMap>`

**What it does:** Concurrent hashmap in rate limiter, gateway state, noise handshake state.

**What you actually use:** 6 files. All are read-heavy, low-contention maps (rate limiter buckets, session state).

**Replacement:** `std::sync::RwLock<HashMap<K, V>>` or `tokio::sync::RwLock` — these are not hot-path contention points.

**Savings:** ~20 unique crates (dashmap pulls crossbeam, hashbrown separately, etc.)

---

### 10. `crypto_box` (34 transitive deps) → direct x25519 + ChaCha20Poly1305

**What it does:** NaCl-compatible public-key encryption (privacy feature).

**What you actually use:** `crypto_box::SalsaBox` for encrypting messages.

**Replacement:** `crypto_box` is literally x25519 key exchange → shared secret → ChaCha20Poly1305 AEAD. You already have both primitives. ~50 lines of glue code.

**Savings:** ~20 unique crates (after dedup with snow removal)

---

### 11. `ed25519-dalek` (28 transitive deps) → `ring` or minimal ed25519

**What it does:** Plugin/bundle signing verification.

**What you actually use:** Sign and verify with Ed25519 keys.

**Replacement:** If you keep x25519-dalek, the dalek ecosystem is already in tree. But if snow and crypto_box are removed, consider `ring` (which bundles ed25519 + x25519 + chacha20) as one dep replacing 4. Or use `ed25519-compact` (0 deps beyond core).

**Savings:** Net ~15-20 crates if consolidated

---

### 12. `inquire` (25 transitive deps) → `crossterm` raw input

**What it does:** Interactive CLI prompts (onboarding wizard).

**What you actually use:** Text input, select, confirm prompts.

**Replacement:** If crossterm is kept (for TUI), build 3 prompt types on raw terminal input. ~200 lines. If TUI is removed, use basic `stdin.read_line` with `println!` prompts.

**Savings:** ~20 unique crates

---

## Tier 3: Low Impact or Already Gated

### 13. `clap` (19 deps) — Keep
Well-justified for a complex CLI with 30+ subcommands. Writing this by hand would be worse.

### 14. `serde` + `serde_json` (9 deps) — Keep
Foundational. Used in 296 files. No realistic alternative.

### 15. `tokio` (16 deps) — Keep
Async runtime. Non-negotiable for this architecture.

### 16. `axum` (133 deps) — Keep (but see reqwest removal)
Most of axum's 133 deps overlap with hyper/tower/http which you need anyway. The unique cost of axum itself is ~10 crates.

### 17. `regex` (6 deps) — Keep
Lean, heavily used (pattern matching in tools, config, routing).

### 18. `uuid` (8 deps) — Could replace with `ulid` or inline
Simple v4 UUID generation is ~10 lines with `rand`. But savings are minimal.

### 19. `rusqlite` (25 deps) — Keep
Core storage engine. The C FFI is unavoidable for SQLite.

### 20. `anyhow` / `thiserror` — Keep
`anyhow` is used in 296 files. Removing it would be massive churn for zero user benefit.

### 21. `async-trait` — Keep (for now)
Used in 194 files. Rust's native async-in-traits (RPITIT) could replace it eventually, but the migration is large and the dep is tiny (proc-macro only).

### 22. ML/AI deps (candle, llama-cpp, tokenizers, tantivy, hnsw) — Already feature-gated
These are enormous but already behind feature flags. Default build doesn't include them.

### 23. `wasmi` (16 deps) — Keep
Lean WASM interpreter, already feature-gated. The alternative (wasmtime) is heavier.

---

## Summary: Projected Impact

| Change | Deps Removed | Effort | Risk |
|--------|-------------|--------|------|
| reqwest → hyper client | ~150 | Medium | Low (hyper already in tree) |
| scraper → hand-rolled | ~50 | Low | Low |
| snow → inline Noise | ~40 | Medium | Medium (crypto) |
| config → toml+env | ~25 | Low | Low |
| tower-http → inline middleware | ~30 | Low | Low |
| rust-embed → include_bytes | ~35 | Low | Low |
| metrics → hand-rolled prometheus | ~40 | Low | Low |
| ratatui/crossterm → disable or raw ANSI | ~55 | Low | Low |
| dashmap → RwLock<HashMap> | ~20 | Low | Low |
| crypto_box → inline | ~20 | Low | Low (existing primitives) |
| ed25519 consolidation | ~15 | Low | Low |
| inquire → raw prompts | ~20 | Low | Low |
| **Total (no double-counting)** | **~350-400** | | |

From 606 → estimated **~200-250 unique crates**, with the binary likely dropping to **~18-20MB**.

---

## Recommended Execution Order

**Phase 0 — Free wins (no code changes):**
1. Add `lto = "thin"`, `strip = true`, `codegen-units = 1` to release profile
2. Run `cargo +nightly udeps` and remove dead deps
3. Deduplicate version splits (align rand, getrandom, hashbrown, thiserror, chacha20, zip, etc.)
4. Slim tokio features per-crate where possible

**Phase 1 — Easy replacements (low risk, low maintenance):**
5. **config → toml+env** (easy, self-contained, good warmup)
6. **scraper → hand-rolled** (easy, isolated to one file)
7. **tower-http → inline middleware** (easy, 3 small middlewares)
8. **rust-embed → include_bytes** (easy build script)
9. **metrics → hand-rolled** (easy, isolated to gateway)
10. **dashmap → RwLock** (easy, 6 files)

**Phase 2 — Bigger swaps:**
11. **reqwest → ureq** (or hyper client if you want full control)
12. **inquire → raw prompts** (if TUI is kept, reuse crossterm)
13. **ratatui → evaluate if TUI feature is even used**

**Phase 3 — Crypto consolidation (only if supply chain security is a goal):**
14. **snow + crypto_box → inline on x25519-dalek + chacha20poly1305**
15. **ed25519-dalek → evaluate `ring` as single crypto dep**

---

## What NOT to Replace

- **serde/serde_json** — too deeply embedded, no lighter alternative
- **tokio** — the async runtime, everything depends on it
- **axum** — marginal unique cost once hyper/tower are in tree
- **clap** — CLI complexity justifies it
- **rusqlite** — SQLite FFI is irreplaceable
- **anyhow** — 296 files, zero-dep proc-macro
- **regex** — lean, 6 deps, heavily used
- **tracing** — lean, well-integrated, no lighter alternative with equivalent functionality

## Strategic Considerations

### What's the actual goal?

The right execution depends on *why* you want fewer deps:

| Goal | Best approach |
|------|--------------|
| **Binary size** | Profile settings (lto/strip) first, then remove heavy optional features. Rewriting deps is secondary. |
| **Compile time** | Dedup versions, slim feature flags, reduce proc-macro usage. The `config`, `scraper`, `tower-http` removals help most here. |
| **Supply chain security** | `cargo vet` / `cargo crev` audit is more targeted. Focus on removing deps you *don't trust*, not deps that are large. |
| **Philosophical minimalism** | The full plan makes sense, but accept the maintenance cost. |
| **Embedded/constrained targets** | Feature-gate aggressively, build `agentzero-lite` with minimal features. |

### Maintenance cost of hand-rolling

Every replaced dep becomes code you own:
- **reqwest → hyper client**: you now handle HTTP/2 negotiation, redirect chains, connection pooling, retry, TLS cert validation, proxy support. When a rustls CVE drops, you patch it yourself.
- **snow → inline Noise**: you own a cryptographic protocol implementation. Auditing burden.
- **scraper → hand-rolled**: low risk — HTML parsing for LLM tools is best-effort anyway.
- **config → toml+env**: low risk — your usage is simple and well-defined.

**Recommendation:** Do Tier 1 items #2-5 (config, scraper, tower-http, rust-embed) first — they're low-risk, low-maintenance replacements. For reqwest, go with `ureq` as the middle path. Only hand-roll crypto (snow, crypto_box) if supply chain security is a core goal.

---

## Verification

After each replacement:
1. `cargo build --release` — compiles cleanly
2. `cargo tree --prefix none | sort -u | wc -l` — track dep count
3. `ls -lh target/release/agentzero` — track binary size
4. `cargo clippy -- -D warnings` — zero warnings
5. Full test suite passes
