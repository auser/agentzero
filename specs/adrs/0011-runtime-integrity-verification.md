# ADR 0011: Runtime Integrity Verification

## Status

Accepted

## Context

Skills are verified at install time via tarball checksums (SHA-256), but the original tarball is discarded after extraction. Between install and run, installed skill files could be tampered with — either by a malicious actor or by accidental modification. Without runtime verification, `agentzero run` trusts the on-disk files blindly.

ADR 0005 established the lockfile as the trust anchor for installed packages. This ADR extends that trust to execution time.

## Decision

Introduce a **directory content checksum** (`dir_checksum`) stored in the lockfile alongside the existing tarball checksum. This hash covers the extracted files, not the tarball, since tarballs are not retained.

### Checksum algorithm

Walk all files in the skill directory in **sorted order by relative path**. For each file, feed three values into a single SHA-256 hasher:

1. The relative path (UTF-8 bytes)
2. The file content length (u64, little-endian)
3. The file content (raw bytes)

The length prefix prevents ambiguity between path bytes and content bytes. The result is `sha256:<hex>`.

### Two-tier verification

1. **Fast path (mtime cache)**: A sidecar file `.agentzero/skills.lock.meta` stores the last-verified epoch seconds per skill. If the skill directory's newest file mtime is ≤ the last-verified time, skip re-hashing. This avoids SHA-256 overhead on every run.

2. **Full path (hash comparison)**: If the mtime check fails or no cache entry exists, compute the directory checksum and compare against the lockfile's `dir_checksum`. On mismatch, refuse to run.

### Security properties

- The mtime fast path is a **performance optimization**, not a security guarantee. An attacker who restores file mtimes can bypass it until the next cold run or explicit `--skip-verify`.
- The full hash comparison is the authoritative check.
- `--skip-verify` is available for development workflows but should not be used in production.

### Backward compatibility

Old lockfile entries without `dir_checksum` skip verification silently. New installs always record `dir_checksum`.

## Consequences

- `agentzero run` verifies skill integrity before execution (unless `--skip-verify` is passed).
- The lockfile gains a new optional `dir_checksum` field (backward-compatible via `#[serde(default)]`).
- A new sidecar file `.agentzero/skills.lock.meta` caches verification timestamps.
- Negligible performance impact on most runs due to mtime fast path.
