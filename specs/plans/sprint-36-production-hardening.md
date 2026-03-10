# Sprint 36 Hardening: TTL Messages, Claim Locks, Directive Integrity

Three production hardening features that strengthen AgentZero's multi-agent coordination and security posture.

## Feature 1: Message TTL / Ephemeral Messages

**Problem:** MemoryEntry has no expiration. Sensitive intermediate results persist indefinitely.

**Changes:**
- `MemoryEntry.expires_at: Option<i64>` — unix timestamp, None = permanent
- `migrate_ttl_column()` — `ALTER TABLE memory ADD COLUMN expires_at INTEGER DEFAULT NULL`
- All query methods filter `WHERE expires_at IS NULL OR expires_at > unixepoch()`
- `MemoryStore::gc_expired()` trait method — deletes expired rows
- Mirrored in both `SqliteMemoryStore` and `PooledMemoryStore`

**Files:** `agentzero-core/src/types.rs`, `agentzero-storage/src/memory/sqlite.rs`, `agentzero-storage/src/memory/pooled.rs`, `agentzero-core/src/agent.rs`

## Feature 2: Job Claim Locks

**Problem:** In Steer mode, no atomic Pending→Running transition. Race window between routing and execution.

**Changes:**
- `JobStore::try_claim(run_id, agent_id) -> bool` — atomic compare-and-swap, Pending→Running
- `JobRecord.claimed_by: Option<String>` — audit trail for who claimed what
- Event log records Running event on successful claim

**Files:** `agentzero-orchestrator/src/job_store.rs`

## Feature 3: Directive Integrity Verification

**Problem:** System prompts flow from DelegateConfig → AgentConfig → LLM with zero validation.

**Changes:**
- `DelegateConfig.system_prompt_hash: Option<String>` — HMAC-SHA256 hex digest
- `compute_prompt_hash(prompt, key) -> String` / `verify_prompt_hash(prompt, hex, key) -> bool`
- `validate_delegation()` checks hash when present, bails on mismatch
- Constant-time comparison via `hmac::verify_slice()`
- `sha2` promoted to non-optional dependency in agentzero-core

**Files:** `agentzero-core/src/delegation.rs`, `agentzero-core/Cargo.toml`, `agentzero-infra/src/runtime.rs`

## Tests Added

- **TTL:** 4 tests — expired excluded from recent, future TTL visible, gc_expired removes/keeps, expired excluded from conversation query
- **Claim locks:** 5 tests — claim pending succeeds, already-running fails, terminal fails, double-claim second fails, nonexistent fails
- **Directive integrity:** 7 tests — roundtrip, tampered fails, wrong key fails, invalid hex, validate rejects tampered, validate accepts matching, validate skips when no hash

## Verification

```bash
cargo test -p agentzero-storage -p agentzero-orchestrator -p agentzero-core
cargo clippy --workspace -- -D warnings
```

All 57 tests pass, 0 clippy warnings.
