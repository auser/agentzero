# Sprint 19: Lightweight Binary + Landing Page + Benchmarks

**Goal:** Position AgentZero competitively as a lightweight, self-contained agent framework. Feature-gate heavy dependencies for a <5MB minimal binary, add reproducible benchmarks, enhance landing page with real data.

**Status:** Complete

### Workstream 1: Feature-Gate Heavy Dependencies

- [x] Feature-gate wasmtime in `agentzero-plugins` behind `wasm-runtime` feature
- [x] Feature-gate `agentzero-plugins` in CLI behind `plugins` feature
- [x] Feature-gate `agentzero-gateway` (axum) behind `gateway` feature
- [x] Feature-gate ratatui+crossterm (dashboard) behind `tui` feature
- [x] Feature-gate `inquire`/`console` behind `interactive` feature
- [x] Slim `config` crate to TOML-only (removed json5/ron/yaml/ini)
- [x] Add `minimal` build profile (`memory-sqlite` only, no wasm/gateway/tui/interactive)
- [x] Add `release-min` Cargo profile (fat LTO + opt-level "z") for size-optimized builds
- [x] Build both default + minimal variants in release workflow
- [x] Add `--variant` flag to install script with interactive TTY prompt
- [x] Update self-updater to track installed variant

### Workstream 2: Benchmarks

- [x] Record initial baselines (binary size, cold-start)
- [x] Add minimal binary size budget to CI (target: <6MB)
- [x] Add cold-start latency benchmark to CI
- [x] Update `reference/benchmarks.md` with real data

### Workstream 3: Landing Page

- [x] Add "By The Numbers" metrics section with real benchmark data
- [x] Add "Why AgentZero" differentiators section (generic, no competitor names)
- [x] Add "Install in seconds" section with terminal mockup
- [x] Responsive styles for new sections

### Final Measurements

| Metric | Default (release) | Minimal (release-min) |
|---|---|---|
| Binary size (macOS arm64) | 18 MB | 5.2 MB (4.95 MiB) |
| Unique crate deps | ~625 | 262 |
| Cold-start (`--help`, min) | ~19ms | ~21ms |
| Cold-start (`--help`, avg) | ~43ms | ~41ms |
