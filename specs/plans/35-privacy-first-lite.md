# Sprint 48: Privacy-First agentzero-lite

**Status:** Planned
**Priority:** HIGH
**Branch:** `feat/privacy-first-lite`

## Context

agentzero-lite is a lightweight gateway binary for edge devices. It currently defaults to privacy mode `"off"` — identical to the full binary. The goal is to rebrand it as a privacy-first product: **"Keeps private files off the cloud, runs fully offline, and adds the security layer local AI agents were missing."**

The privacy infrastructure already exists (Noise Protocol, sealed envelopes, key rotation, `local_only` mode, per-component boundaries). The work is: (1) add a new `"private"` privacy mode between `"off"` and `"local_only"`, (2) make it the lite binary's default, (3) wire it through the gateway startup, (4) update docs/messaging.

**Key constraint:** Cloud AI providers (Anthropic, OpenAI) ARE allowed but must be explicitly configured. No auto-detection from env vars in `"private"` mode.

---

## Architecture: `"private"` vs `"local_only"`

**Key insight:** Provider calls (Anthropic API, OpenAI API) go through `agentzero-providers`, NOT through the tool security policy. Agent-invoked tools (`web_search`, `http_request`, `web_fetch`) are controlled by `ToolSecurityPolicy` in `policy.rs`. So blocking network **tools** does NOT affect cloud **provider** calls — they're separate code paths.

| Behavior | `"private"` | `"local_only"` |
|----------|-------------|-----------------|
| Cloud providers | **Allowed** (explicit TOML config) | Blocked entirely |
| Network tools (`web_search`, `http_request`, etc.) | Blocked | Blocked |
| TTS / image gen / video gen / domain tools | Blocked | Blocked |
| Noise Protocol | Auto-enabled | Not enabled |
| Key rotation | Auto-enabled | Not enabled |
| Local providers | Allowed freely | Allowed freely |
| Per-agent boundary default | `encrypted_only` | `local_only` |
| URL access / domain allowlist | **Unchanged** (no restriction) | Localhost only |

---

## Phase A: New `"private"` Privacy Mode (HIGH)

### `crates/agentzero-config/src/model.rs` (validation)
- [ ] Add `"private"` to the privacy mode validation match (line 231)
- [ ] Do NOT add `"private"` to cloud-provider rejection (line 239-248) — unlike `"local_only"`, `"private"` allows cloud providers when explicitly configured
- [ ] Add `"private"` to noise requirement check (line 270) alongside `"encrypted"`
- [ ] Per-agent boundary mapping (line 291): `"private"` maps to `"encrypted_only"`

### `crates/agentzero-config/src/policy.rs` (tool security)
- [ ] Add separate `"private"` block after existing `local_only` block (line 110-129)
- [ ] Block same network tools as `local_only`: `http_request`, `web_fetch`, `web_search`, `html_extract`, `composio`, `tts`, `image_gen`, `video_gen`, `domain_tools`
- [ ] **IMPORTANT:** Do NOT set `enforce_domain_allowlist` or restrict `url_access` — leave URL access open so cloud providers work when explicitly configured

---

## Phase B: GatewayRunOptions Privacy Override (HIGH)

### `crates/agentzero-gateway/src/lib.rs`
- [ ] Add `default_privacy_mode: Option<String>` to `GatewayRunOptions` (line 50-61)
- [ ] In privacy initialization (line 250-258), use override when no config: `options.default_privacy_mode.as_deref().unwrap_or("off")`
- [ ] Add `"private"` to noise/rotation activation matches (line 264-269)

### `bin/agentzero-lite/Cargo.toml`
- [ ] Enable `privacy` feature: `agentzero-gateway = { workspace = true, features = ["privacy"] }`

---

## Phase C: Lite Binary Hardening (MEDIUM)

### `bin/agentzero-lite/src/main.rs`
- [ ] Set `default_privacy_mode: Some("private".into())`
- [ ] Add `--privacy-mode` CLI arg (default: `"private"`, values: off/private/local_only/encrypted/full)
- [ ] Tighten default rate limits: `rate_limit_max: 120` (2 req/s vs 600 default)
- [ ] Update doc comment with privacy-first positioning

---

## Phase D: Privacy Banner (MEDIUM)

### `crates/agentzero-gateway/src/banner.rs`
- [ ] Add privacy mode to banner: `Privacy: PRIVATE (Noise-encrypted, local-first)`
- [ ] Cloud provider warning: `CLOUD PROVIDER: anthropic — data WILL leave this machine`

### `crates/agentzero-gateway/src/lib.rs`
- [ ] Pass privacy mode and provider info to banner function

---

## Phase E: Documentation & Messaging (MEDIUM)

- [ ] `site/src/content/docs/guides/privacy.md` — Add `"private"` mode, "agentzero-lite: Privacy-First by Default" section
- [ ] `site/src/content/docs/config/privacy.md` — Add `"private"` mode docs
- [ ] `site/src/content/docs/guides/raspberry-pi.md` — Reference lite binary with privacy defaults
- [ ] `examples/edge-deployment/` — Example configs: `config-local.toml` (ollama) and `config-cloud.toml` (explicit anthropic)

---

## Phase F: Tests (MEDIUM)

### `crates/agentzero-config/` (model + policy tests)
- [ ] `privacy_private_mode_accepted`
- [ ] `privacy_private_mode_blocks_network_tools`
- [ ] `privacy_private_mode_allows_explicit_cloud`

### `bin/agentzero-lite/src/main.rs`
- [ ] `cli_defaults_to_private_mode`
- [ ] `cli_accepts_privacy_mode_override`

### `crates/agentzero-gateway/`
- [ ] Gateway starts in private mode with Noise auto-enabled

---

## Verification

1. `cargo build -p agentzero-lite --release` — compiles with privacy feature
2. `cargo test -p agentzero-lite` — all tests pass (existing + new)
3. `cargo test -p agentzero-config` — private mode validation tests pass
4. Run `agentzero-lite` with no config → banner shows "PRIVATE" mode, Noise enabled
5. Run with explicit cloud provider config → warning banner appears
6. Run with `--privacy-mode off` → reverts to standard behavior
7. `cargo clippy --workspace` — 0 warnings
8. `cargo test --workspace` — all pass

---

## Critical File Paths

| File | Purpose |
|------|---------|
| `crates/agentzero-config/src/model.rs` | Add `"private"` mode validation |
| `crates/agentzero-config/src/policy.rs` | Tool security policy for `"private"` mode |
| `crates/agentzero-gateway/src/lib.rs:50-61` | Add `default_privacy_mode` to `GatewayRunOptions` |
| `crates/agentzero-gateway/src/lib.rs:250-270` | Wire privacy mode override into startup |
| `crates/agentzero-gateway/src/banner.rs` | Privacy banner messaging |
| `bin/agentzero-lite/src/main.rs` | Set defaults, add CLI arg |
| `bin/agentzero-lite/Cargo.toml` | Enable `privacy` feature |
| `site/src/content/docs/guides/privacy.md` | Docs update |
