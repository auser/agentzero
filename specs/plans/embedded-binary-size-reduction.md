# Embedded Binary Size Reduction Plan

**Status**: Planned
**Priority**: High
**Created**: 2026-03-14

## Motivation

AgentZero's `embedded` profile binary (built with `release-min`, fat LTO, `opt-level = "z"`) currently weighs **10.1MB**. For deployment on resource-constrained embedded devices, we need to significantly reduce this. The CI budget was temporarily raised from 10MB to 11MB to unblock, but the trend must be reversed.

## Current State

| Profile   | Size    | Budget |
|-----------|---------|--------|
| default   | ~28MB   | 30MB   |
| minimal   | <8MB    | 8MB    |
| embedded  | 10.1MB  | 11MB   |

**Embedded features**: `memory-sqlite` + `plugins` (WASM)

**Build profile** (`release-min`):
- Inherits `release` (codegen-units=1, panic=abort, strip=symbols)
- Fat LTO
- `opt-level = "z"` (size optimization)

## Major Size Contributors

| Dependency              | Est. Size | Notes                                    |
|-------------------------|-----------|------------------------------------------|
| bundled SQLCipher       | ~3-5MB    | Largest single dep (C code + crypto)     |
| wasmi (WASM runtime)    | ~300-400KB| Interpreter-based, reasonable            |
| reqwest (HTTP client)   | ~500KB-1MB| Feature-dependent                        |
| Crypto stack            | ~200-400KB| chacha20poly1305, x25519, snow, sha2     |
| 30+ compiled-in tools   | ~500KB+   | Many unnecessary for embedded            |

## Reduction Strategies

### Phase 1: Feature-gate tools (target: -500KB to -1MB)
- Split tool registration into tiers: `core`, `extended`, `full`
- Embedded profile compiles only `core` tools (file ops, shell, memory, sub-agents)
- Move browser, web search, document, media tools behind `extended` gate
- Move composio, pushover, delegate tools behind `full` gate

### Phase 2: Plain SQLite option (target: -2MB)
- Add `memory-sqlite-plain` feature using `rusqlite` without `bundled-sqlcipher`
- Use system SQLite or `bundled` (no encryption) for embedded targets
- Keep `memory-sqlite` as the encrypted default for non-embedded builds
- Encryption can still be layered at the application level if needed

### Phase 3: Optional WASM plugins (target: -300KB)
- Make `plugins` optional within the `embedded` feature
- Create `embedded-minimal` = `["memory-sqlite-plain"]` (no WASM)
- Create `embedded` = `["memory-sqlite-plain", "plugins"]` (with WASM)

### Phase 4: Minimize HTTP client (target: -200KB)
- Audit reqwest feature flags; disable unused ones (cookies, gzip, etc.)
- Consider `ureq` as a smaller sync HTTP client for embedded
- Only pull in TLS if the embedded target needs outbound HTTPS

### Phase 5: Dependency audit with cargo-bloat (target: variable)
- Run `cargo bloat --release --crates` to identify hidden size contributors
- Audit transitive dependencies for unnecessary features
- Replace heavy deps with lighter alternatives where feasible

### Phase 6: Binary compression (deployment-time)
- Evaluate UPX for deployment compression (~60-70% reduction)
- This doesn't reduce actual memory usage but reduces storage/transfer size
- Consider `xz`-compressed images for OTA updates

## Budget Targets

| Milestone    | Target | Timeline     |
|--------------|--------|--------------|
| Short-term   | 11MB   | Current      |
| Phase 1-2    | 8MB    | Next sprint  |
| Phase 3-5    | 5MB    | Sprint after |
| Stretch      | <4MB   | Future       |

## Validation

- CI enforces size budgets via `scripts/check-binary-size.sh`
- Add `cargo bloat` report as a CI artifact for tracking trends
- Consider adding a size regression chart to the dashboard

## Key Files

- `bin/agentzero/Cargo.toml` — feature definitions
- `Cargo.toml` (workspace) — profile settings (lines 87-96)
- `crates/agentzero-infra/src/tools/mod.rs` — tool registration
- `crates/agentzero-storage/Cargo.toml` — SQLite/SQLCipher features
- `.github/workflows/ci.yml` — size budget checks
