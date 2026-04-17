# Plan 52: File Capability Enforcement

## Status: COMPLETE (Sprint 91)

## Context

Plans 48–51 established the capability model and enforced it across dynamic tools,
delegation, MCP, A2A, WASM plugins, API key ceilings, and memory namespaces.
`Capability::FileRead { glob }` and `Capability::FileWrite { glob }` already
exist in `CapabilitySet`, but the file tools (`read_file`, `write_file`,
`apply_patch`, `file_edit`) were not consulting them at runtime. This left file
I/O as the only remaining tool category without capability enforcement.

Sprint 91 closes that gap by applying `CapabilitySet::allows_file_read` and
`CapabilitySet::allows_file_write` inside each file tool, using the same
pattern as the memory tooling added in Plan 51.

---

## Decisions

### 1. Use `allows_file_read` / `allows_file_write` as the enforcement predicate

File tooling should respect the existing capability predicates:

- `CapabilitySet::allows_file_read(&Path)` for read access
- `CapabilitySet::allows_file_write(&Path)` for write access

These mirror the semantics of other capability checks and keep enforcement
centralized in the capability model.

### 2. Preserve backward compatibility via `CapabilitySet::is_empty()`

An empty capability set represents legacy config. In that case, the tools must
skip enforcement to preserve historical behavior.

### 3. Evaluate globs against the user-provided relative path

Capability globs (e.g., `src/**`) should match the same path users specify in
tool calls. Therefore, enforcement checks the **relative** input path (before
canonicalization), rather than the absolute resolved path.

---

## Phase L: File Tool Capability Enforcement

**Estimated effort:** 0.5 days

### L1: `read_file` capability guard

**File:** `crates/agentzero-tools/src/read_file.rs`

Add the FileRead guard immediately after safe path resolution:

    // Phase L: FileRead capability enforcement.
    if !ctx.capability_set.is_empty()
        && !ctx.capability_set.allows_file_read(Path::new(input.trim()))
    {
        return Err(anyhow!(
            "capability denied: FileRead does not permit '{}'",
            input.trim()
        ));
    }

### L2: `write_file` capability guard

**File:** `crates/agentzero-tools/src/write_file.rs`

Add the FileWrite guard after destination resolution:

    // Phase L: FileWrite capability enforcement.
    if !ctx.capability_set.is_empty()
        && !ctx.capability_set.allows_file_write(Path::new(&request.path))
    {
        return Err(anyhow!(
            "capability denied: FileWrite does not permit '{}'",
            request.path
        ));
    }

### L3: `apply_patch` capability guard

**File:** `crates/agentzero-tools/src/apply_patch.rs`

Add the FileWrite guard inside the per-file loop:

    // Phase L: FileWrite capability enforcement.
    if !ctx.capability_set.is_empty()
        && !ctx.capability_set.allows_file_write(Path::new(&pf.path))
    {
        return Err(anyhow!(
            "capability denied: FileWrite does not permit '{}'",
            pf.path
        ));
    }

### L4: `file_edit` capability guard

**File:** `crates/agentzero-tools/src/file_edit.rs`

Add the FileWrite guard after resolving the edit target:

    // Phase L: FileWrite capability enforcement.
    if !ctx.capability_set.is_empty()
        && !ctx.capability_set.allows_file_write(Path::new(&request.path))
    {
        return Err(anyhow!(
            "capability denied: FileWrite does not permit '{}'",
            request.path
        ));
    }

---

## Tests

Add unit tests following the same pattern as Plan 51’s memory tool tests:

- Denied when `capability_set` is non-empty and lacks `FileRead` / `FileWrite`
- Allowed when the set includes a matching glob
- Allowed when the set is empty (legacy config)

---

## Files to Modify

- `crates/agentzero-tools/src/read_file.rs`
- `crates/agentzero-tools/src/write_file.rs`
- `crates/agentzero-tools/src/apply_patch.rs`
- `crates/agentzero-tools/src/file_edit.rs`

---

## Effort Estimate

- Implementation: 0.25 days
- Tests: 0.25 days

---

## Acceptance Criteria

- [x] `read_file` denies access when `capability_set` is non-empty and the
      FileRead glob does not match the requested path.
- [x] `read_file` allows access when the FileRead glob matches the requested
      path.
- [x] `write_file` denies access when `capability_set` is non-empty and the
      FileWrite glob does not match the requested path.
- [x] `write_file` allows access when the FileWrite glob matches the requested
      path.
- [x] `apply_patch` enforces FileWrite per file in the patch.
- [x] `file_edit` enforces FileWrite against the target path.
- [x] Empty `CapabilitySet` preserves legacy behavior.
- [x] `cargo check --workspace` returns 0 errors.
- [x] `cargo test --workspace` passes.

---

## What This Closes

This completes capability enforcement across all tool categories. With
FileRead/FileWrite now enforced at runtime, the Sprint 86 threat model is
fully resolved for file I/O.