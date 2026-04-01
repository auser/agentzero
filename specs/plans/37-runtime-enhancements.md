# Plan: Runtime Enhancements — Audit Replay, Typed IDs, Delegation Injection, Plugin Shims

## Context

After researching external agent runtime patterns, we identified five enhancements to integrate
into AgentZero.

---

## 1. Monotonic Sequence Numbers on AuditEvent

**Problem**: `AuditEvent` currently has `stage` + `detail` only. No ordering guarantee, no
replay capability, no incremental sync.

**Changes**:

- **`crates/agentzero-core/src/types.rs`** (~line 948): Add `seq: u64` and `session_id: String`
  fields to `AuditEvent`. Add a constructor that takes an `AtomicU64` counter reference.

  ```rust
  pub struct AuditEvent {
      pub seq: u64,              // NEW: monotonic per-session
      pub session_id: String,    // NEW: groups events for replay
      pub stage: String,
      pub detail: Value,
  }
  ```

- **`crates/agentzero-infra/src/audit.rs`**: Add `SessionAuditCounter` that wraps
  `Arc<AtomicU64>` and is passed into the runtime execution context. `FileAuditSink::record()`
  already writes JSON lines — `seq` and `session_id` will serialize naturally.

- **`crates/agentzero-infra/src/runtime.rs`**: Create a `SessionAuditCounter` at the start of
  each `RuntimeExecution` and thread it through tool execution so every event gets a sequence
  number.

- **`crates/agentzero-gateway/src/handlers.rs`**: Add `GET /v1/runs/{run_id}/events?since_seq=N`
  endpoint that reads the audit log and returns events with `seq > N`. This enables incremental
  sync for channels.

**Files to modify**:
- `crates/agentzero-core/src/types.rs` (AuditEvent struct)
- `crates/agentzero-infra/src/audit.rs` (counter + FileAuditSink)
- `crates/agentzero-infra/src/runtime.rs` (thread counter through execution)
- `crates/agentzero-gateway/src/handlers.rs` (events endpoint)
- `crates/agentzero-gateway/src/models.rs` (EventsRequest/Response types)

---

## 2. ID-Based Public API Surface for Gateway + FFI

**Problem**: Gateway models already use `String` IDs (good), but there's no enforced type
distinction between a `run_id`, `agent_id`, or `session_id` — they're all `String`. Rich
internal types leak into handlers.

**Changes**:

- **`crates/agentzero-core/src/types.rs`**: Introduce newtype wrappers with Serialize/Deserialize:
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
  pub struct SessionId(pub String);
  
  #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]  
  pub struct AgentId(pub String);
  ```
  `RunId` already exists — make `SessionId` and `AgentId` follow the same pattern.

- **`crates/agentzero-gateway/src/models.rs`**: Replace raw `String` fields with typed IDs.
  `ChatRequest`, `AsyncSubmitRequest`, `JobStatusResponse`, `AgentDetailResponse` etc. all
  get typed ID fields. JSON serialization is transparent (newtypes serialize as strings).

- **`crates/agentzero-gateway/src/handlers.rs`**: Update handler signatures to use typed IDs.
  Path extractors parse into `RunId`, `AgentId` directly.

- **Rule to enforce going forward**: Public API methods (gateway handlers, FFI exports) must
  accept/return JSON-serializable data only. No `Arc<Mutex<...>>`, no trait objects, no
  internal types crossing the boundary.

**Files to modify**:
- `crates/agentzero-core/src/types.rs` (newtype IDs)
- `crates/agentzero-gateway/src/models.rs` (typed ID fields)
- `crates/agentzero-gateway/src/handlers.rs` (typed path extractors)

---

## 3. Agent-Agnostic Instruction Injection for Delegation

**Problem**: `DelegateConfig` has a single `system_prompt: Option<String>` field. All sub-agents
get instructions the same way (system prompt injection). But different agent runtimes accept
instructions differently — CLI flags, env vars, system prompt, tool definitions, etc.

**Changes**:

- **`crates/agentzero-core/src/delegation.rs`**: Add an `InstructionMethod` enum to
  `DelegateConfig`:
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize, Default)]
  pub enum InstructionMethod {
      #[default]
      SystemPrompt,                           // inject as system prompt (current behavior)
      EnvVar { key: String },                 // set env var before execution
      CliFlag { flag: String },               // pass as CLI argument
      ToolDefinition { tool_name: String },   // inject as a tool's description
      Custom { template: String },            // user-defined template with {instructions} placeholder
  }
  ```

- **`crates/agentzero-tools/src/delegate.rs`**: In `execute_delegate()`, match on
  `config.instruction_method` to determine how to inject the system prompt / instructions
  into the sub-agent. `SystemPrompt` keeps current behavior. Other variants prepare the
  instruction delivery differently before calling the provider.

- **`crates/agentzero-config/src/model.rs`**: Add `instruction_method` field to the delegate
  agent TOML config section. Default to `SystemPrompt` for backward compatibility.

- **Site docs**: Update `site/src/content/docs/guides/providers.md` and
  `site/src/content/docs/config/reference.md` with the new config option.

**Files to modify**:
- `crates/agentzero-core/src/delegation.rs` (InstructionMethod enum + field)
- `crates/agentzero-tools/src/delegate.rs` (dispatch on method in execute_delegate)
- `crates/agentzero-config/src/model.rs` (TOML field)
- `site/src/content/docs/config/reference.md`
- `site/src/content/docs/guides/providers.md`

---

## 4. CLI Shim Bridge for WASM Plugin Host Calls

**Problem**: WASM plugins currently communicate with the host via a custom ABI (`az_log`,
`az_env_get` in the `"az"` namespace). Adding new host capabilities requires ABI changes,
new host function registration, and plugin SDK updates.

**Changes**:

- **`crates/agentzero-plugins/src/shim_server.rs`** (new file): Lightweight HTTP server on
  `127.0.0.1:0` that exposes host tools as POST endpoints. Each tool gets
  `/tools/{tool_name}` accepting JSON input, returning JSON output. Server lifetime tied to
  plugin execution.

- **`crates/agentzero-plugins/src/shim_gen.rs`** (new file): Generates shell scripts for each
  exposed tool:
  ```bash
  #!/bin/sh
  curl -s -X POST "http://127.0.0.1:${AZ_HOST_PORT}/tools/read_file" \
    -H "Content-Type: application/json" -d "$1"
  ```
  These get written to a temp directory and pre-opened into the WASM WASI filesystem.

- **`crates/agentzero-plugins/src/wasm.rs`**: During plugin execution setup, start shim server,
  generate shims, set `AZ_HOST_PORT` env var in WASI context, pre-open shim directory at
  `/usr/local/bin/`.

- **`crates/agentzero-plugins/src/wasm.rs`** (WasmIsolationPolicy): Add
  `allowed_host_tools: Vec<String>` to control which tools are exposed via shims.

**Files to modify/create**:
- `crates/agentzero-plugins/src/shim_server.rs` (new)
- `crates/agentzero-plugins/src/shim_gen.rs` (new)
- `crates/agentzero-plugins/src/wasm.rs` (integration)
- `crates/agentzero-plugins/src/lib.rs` (module declarations)

---

## 5. CoW Overlay for WASM Plugin Filesystem

**Problem**: WASM plugins (and eventually native tools) write directly to the real filesystem.
Failed multi-step operations leave partial state. Concurrent sub-agents can conflict on writes.
No dry-run capability exists.

**What this gives us**:

- **Rollback on failure**: Multi-step tool chain fails partway? Discard the overlay, zero cleanup.
- **Dry-run mode**: Run an entire agent session against the overlay, show the user a diff of
  what would change, then commit or discard. Critical for autonomy workflows.
- **Concurrent agent isolation**: Multiple sub-agents operate on independent overlays of the
  same workspace, merge at the end. No write conflicts.
- **Built-in audit trail**: The overlay's whiteout set + modified file set is a complete record
  of what the agent changed.

**Scope**: WASM plugins first (we control the WASI filesystem via pre-opened directories, so
no FUSE needed). Native tool execution would require platform-specific work later.

**Changes**:

- **`crates/agentzero-plugins/src/overlay.rs`** (new file): `WasiOverlayFs` struct:
  ```rust
  pub struct WasiOverlayFs {
      base: PathBuf,                          // read-only source of truth
      scratch: TempDir,                       // writes land here
      whiteouts: HashSet<PathBuf>,            // tracks deletions
  }

  impl WasiOverlayFs {
      pub fn new(base: PathBuf) -> Result<Self>;
      pub fn read(&self, path: &Path) -> Result<Vec<u8>>;   // scratch first, then base
      pub fn write(&self, path: &Path, data: &[u8]) -> Result<()>; // always to scratch
      pub fn delete(&self, path: &Path) -> Result<()>;      // add to whiteouts
      pub fn diff(&self) -> OverlayDiff;                     // list all changes
      pub fn commit(&self) -> Result<CommitReport>;          // apply scratch to base
      pub fn discard(self);                                   // drop scratch dir
  }
  ```

- **`crates/agentzero-plugins/src/wasm.rs`**: During plugin execution, create `WasiOverlayFs`
  over the workspace root. Pre-open the overlay's virtual root instead of the real directory.
  After execution, auto-commit on success or discard on failure (configurable).

- **`crates/agentzero-plugins/src/wasm.rs`** (host calls): Register `az_fs_commit` and
  `az_fs_discard` host functions so plugins can explicitly control when changes land.

- **`crates/agentzero-plugins/src/wasm.rs`** (WasmIsolationPolicy): Add
  `overlay_mode: OverlayMode` field:
  ```rust
  pub enum OverlayMode {
      Disabled,           // direct writes (current behavior, backward compat)
      AutoCommit,         // commit on success, discard on failure
      ExplicitCommit,     // plugin must call az_fs_commit
      DryRun,             // always discard, return diff
  }
  ```

**Edge cases to handle**:
- Symlinks: resolve before overlay lookup to prevent escape
- Directory creation: track in scratch, merge on commit
- File permissions: preserve from base, allow override in scratch
- Atomic rename: handle within scratch layer; cross-layer rename = copy + whiteout

**Files to modify/create**:
- `crates/agentzero-plugins/src/overlay.rs` (new — core overlay logic)
- `crates/agentzero-plugins/src/wasm.rs` (integration + host calls + OverlayMode)
- `crates/agentzero-plugins/src/lib.rs` (module declaration)
- `crates/agentzero-config/src/model.rs` (overlay_mode config field)

---

## Risks & Mitigations

### 1. Sequence Numbers — Low Risk
- **Audit log format change**: Adding `seq` and `session_id` fields changes the JSON line
  format. Any external tooling parsing audit logs will need updating.
- **Mitigation**: Fields are additive (new keys in JSON objects), so lenient parsers survive.
  Document the format change.

### 2. ID-Based Public API — Low-Medium Risk
- **Mechanical churn**: Every gateway handler and model struct needs updating. Large diff,
  many places to miss a raw `String`.
- **Mitigation**: Newtype wrappers serialize identically to `String` in JSON, so the wire
  format doesn't change. External clients are unaffected. Use clippy + grep to find stragglers.

### 3. Instruction Injection — Medium Risk
- **Template injection**: The `Custom { template }` variant substitutes `{instructions}` into
  a user-defined template. If the template is used to construct shell commands or env vars,
  this could be an injection vector.
- **Mitigation**: Validate that `Custom` templates are only used for system prompt text, not
  shell/env contexts. The `EnvVar` and `CliFlag` variants should sanitize values (no newlines,
  no null bytes, length limits).
- **Premature variants**: `CliFlag` and `EnvVar` imply executing sub-agents as external
  processes, which we don't currently do for delegated agents (they run in-process via provider
  API calls). These variants may be dead code initially.
- **Mitigation**: Implement `SystemPrompt` and `Custom` first. Gate `CliFlag`/`EnvVar` behind
  a feature flag or defer until external agent execution is real.

### 4. CLI Shim Bridge — Medium-High Risk
- **Attack surface**: An HTTP server on localhost per plugin execution. Even on 127.0.0.1,
  other processes on the machine can reach it.
- **Mitigation**: Generate a random bearer token per execution, include in shims, validate on
  every request. Bind to `127.0.0.1` only. Shut down server immediately after plugin exits.
- **Port races**: `127.0.0.1:0` allocation can race with other processes.
- **Mitigation**: OS handles this — port 0 means the OS picks an available port. Read back
  the actual port after bind.
- **Shell command injection in shims**: Shim scripts pass arguments to curl. Malicious input
  could break out of the JSON payload.
- **Mitigation**: Shims should read input from stdin (piped), not from shell arguments.
  Use `--data @-` instead of `-d "$1"`.

### 5. CoW Overlay — Medium Risk
- **Filesystem semantics complexity**: Symlink traversal, hard links, directory renames,
  permission bits, timestamps — all need correct overlay behavior.
- **Mitigation**: Start with file-level overlay only (no directory-level CoW). Symlinks
  resolved and checked against base boundary before any operation. Test against a comprehensive
  filesystem operation matrix.
- **Commit conflicts**: Base filesystem may have changed between overlay creation and commit
  (another agent, user edit, etc.).
- **Mitigation**: `commit()` checks mtime of base files before overwriting. Conflict = error
  with diff, not silent overwrite. Let the caller decide (force, abort, merge).
- **Performance**: Every file read goes through overlay lookup (scratch → whiteout check → base).
- **Mitigation**: Acceptable for WASM plugin workloads (low file I/O frequency). Not a concern
  until we extend to native tools.

### Cross-Cutting Risk: Feature Interaction
Items #4 (shim bridge) and #5 (overlay) both modify WASM plugin execution setup in `wasm.rs`.
Building them sequentially (shims first, overlay second) is important — the overlay should
wrap the shim-enabled filesystem, not the other way around.

---

## Implementation Order

1. **Monotonic sequence numbers** — smallest, no breaking changes, immediate debugging value
2. **Agent-agnostic instruction injection** — small, backward-compatible default, unlocks heterogeneous delegation
3. **ID-based public API** — medium, improves type safety across gateway/FFI boundary
4. **CLI shim bridge** — medium, new files only, no changes to existing plugin behavior
5. **CoW overlay for WASM plugins** — builds on #4, overlay wraps the shim-enabled filesystem

## Verification

- **Sequence numbers**: Existing audit tests + new test that verifies monotonic ordering across concurrent tool calls
- **Instruction injection**: Unit test per `InstructionMethod` variant in delegate.rs. Security test that `Custom` template rejects shell metacharacters.
- **ID-based API**: Existing gateway integration tests should pass (newtypes serialize as strings); add type-safety tests
- **CLI shim bridge**: Integration test that starts shim server, generates shim, executes from WASM guest, verifies host tool was called. Security test that bearer token is required.
- **CoW overlay**: Filesystem operation matrix test (read, write, delete, overwrite, symlink escape, commit, discard). Conflict detection test where base changes during overlay lifetime.
- **All**: `cargo clippy --all-targets --all-features` must pass with zero warnings; `cargo test` green
