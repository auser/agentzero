# AgentZero Sprint Plan

## Sprint 19: Lightweight Binary + Landing Page + Benchmarks

**Goal:** Position AgentZero competitively as a lightweight, self-contained agent framework. Feature-gate heavy dependencies for a <5MB minimal binary, add reproducible benchmarks, enhance landing page with real data.

### Workstream 1: Feature-Gate Heavy Dependencies

- [x] Feature-gate wasmtime in `agentzero-plugins` behind `wasm-runtime` feature
- [x] Feature-gate `agentzero-plugins` in CLI behind `plugins` feature
- [x] Feature-gate `agentzero-gateway` (axum) behind `gateway` feature
- [x] Feature-gate ratatui+crossterm (dashboard) behind `tui` feature
- [x] Add `minimal` build profile (`memory-sqlite` only, no wasm/gateway/tui)
- [ ] Build both default + minimal variants in release workflow
- [ ] Add `--variant` flag to install script with interactive TTY prompt
- [ ] Update self-updater to track installed variant
- [ ] Evaluate `lto = "fat"` vs `"thin"` for further size reduction

### Workstream 2: Benchmarks

- [ ] Record initial baselines (binary size, cold-start, criterion)
- [ ] Add minimal binary size budget to CI (target: <6MB)
- [ ] Add cold-start latency benchmark to CI
- [ ] Update `reference/benchmarks.md` with real data

### Workstream 3: Landing Page

- [ ] Add "By The Numbers" metrics section with real benchmark data
- [ ] Add "Why AgentZero" differentiators section (generic, no competitor names)
- [ ] Add "Install in seconds" section with terminal mockup
- [ ] Responsive styles for new sections

### Current Measurements

| Metric | Default | Minimal |
|---|---|---|
| Binary size (macOS arm64) | 12 MB | TBD (building) |
| Cold-start (`--help`, min) | ~19ms | TBD |
| Cold-start (`--help`, avg) | ~52ms | TBD |
| Transitive deps | 625 | TBD |

Previous sprint archived to `specs/sprints/18-delegation-hardening-channels.md`.
