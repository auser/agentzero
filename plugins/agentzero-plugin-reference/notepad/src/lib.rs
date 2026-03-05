//! Notepad — AgentZero reference WASM plugin.
//!
//! This plugin is the canonical example for plugin authors. It demonstrates
//! every SDK pattern in one concise implementation:
//!
//! 1. **Typed input** — `#[derive(Deserialize)]` request struct instead of
//!    manual `serde_json::Value` indexing.
//! 2. **`az_log` host call** — structured logging via the host ABI.
//! 3. **`ToolOutput::with_warning`** — returning a result with a non-fatal warning.
//! 4. **WASI filesystem** — flat note storage in the workspace sandbox.
//! 5. **Path security** — validating note IDs before constructing file paths.
//! 6. **Action dispatch** — routing on the `action` field.
//!
//! # Actions
//!
//! | action   | required fields       | effect                     |
//! |----------|-----------------------|----------------------------|
//! | `write`  | `note_id`, `content`  | Create or overwrite a note |
//! | `read`   | `note_id`             | Read a note's content      |
//! | `list`   | —                     | List all note IDs          |
//! | `delete` | `note_id`             | Delete a note (idempotent) |
//!
//! # Build
//!
//! ```sh
//! cd plugins/agentzero-plugin-reference/notepad
//! cargo build --release
//! # Output: target/wasm32-wasip1/release/notepad_plugin.wasm
//! ```

use agentzero_plugin_sdk::prelude::*;
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::io;

// ---------------------------------------------------------------------------
// ABI v2 entrypoint
// ---------------------------------------------------------------------------

declare_tool!("notepad", execute);

// ---------------------------------------------------------------------------
// az_log host call
// ---------------------------------------------------------------------------
//
// The AgentZero runtime registers `az_log` in the "az" WASM import module.
// It is always allowed (no capability gate required).
//
// Level constants: 0 = ERROR, 1 = WARN, 2 = INFO, 3 = DEBUG, 4 = TRACE
//
// The SDK does not wrap host calls; this extern block is the correct idiom.
// For az_env_get (environment variable access), see the composio/pushover
// plugins which use `std::env::var()` via WASI instead.

#[link(wasm_import_module = "az")]
extern "C" {
    fn az_log(level: i32, msg_ptr: *const u8, msg_len: i32);
}

const LOG_ERROR: i32 = 0;
const LOG_WARN: i32 = 1;
const LOG_INFO: i32 = 2;
const LOG_DEBUG: i32 = 3;

/// Log a message to the host at the given level.
///
/// SAFETY: `msg.as_ptr()` and `msg.len()` are valid for the lifetime of `msg`.
/// The host reads the bytes synchronously before `az_log` returns.
fn log(level: i32, msg: &str) {
    unsafe { az_log(level, msg.as_ptr(), msg.len() as i32) }
}

// ---------------------------------------------------------------------------
// Typed input (vs manual Value indexing used by other plugins)
// ---------------------------------------------------------------------------
//
// Alternative: use `#[serde(tag = "action")]` on an enum for exhaustive
// dispatch when actions have disjoint required fields.

#[derive(Debug, Deserialize)]
struct Request {
    action: String,
    #[serde(default)]
    note_id: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

// ---------------------------------------------------------------------------
// Main handler
// ---------------------------------------------------------------------------

fn execute(input: ToolInput) -> ToolOutput {
    log(LOG_INFO, "notepad: received request");

    let req: Request = match serde_json::from_str(&input.input) {
        Ok(r) => r,
        Err(e) => {
            log(LOG_ERROR, &format!("notepad: invalid input JSON: {e}"));
            return ToolOutput::error(format!("invalid input: {e}"));
        }
    };

    log(LOG_DEBUG, &format!("notepad: action={}", req.action));

    match req.action.trim() {
        "write" => handle_write(req),
        "read" => handle_read(req),
        "list" => handle_list(),
        "delete" => handle_delete(req),
        other => {
            log(LOG_WARN, &format!("notepad: unknown action: {other}"));
            ToolOutput::error(format!(
                "unknown action '{other}'. supported: write|read|list|delete"
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// WASI filesystem paths
// ---------------------------------------------------------------------------
//
// In the WASI sandbox, the workspace root is preopened as ".".
// All paths must be relative; absolute paths are not accessible.
//
// Read-only vs read-write:
//   The host WasmIsolationPolicy sets allow_fs_read / allow_fs_write.
//   For a read-only plugin, request only "wasi:filesystem/read" in the
//   manifest capabilities and set allow_fs_write: false in the host policy.

fn notes_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(".").join(".agentzero").join("notepad")
}

fn note_path(note_id: &str) -> std::path::PathBuf {
    notes_dir().join(format!("{note_id}.md"))
}

/// Validate that a note_id is a safe, flat identifier.
/// Rejects path traversal sequences and filesystem separators.
fn validate_note_id(raw: &str) -> Result<&str, String> {
    let id = raw.trim();
    if id.is_empty() {
        return Err("note_id must not be empty".into());
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(format!(
            "note_id '{id}' must not contain '/', '\\', or '..'"
        ));
    }
    Ok(id)
}

// ---------------------------------------------------------------------------
// Action handlers
// ---------------------------------------------------------------------------

fn handle_write(req: Request) -> ToolOutput {
    let note_id = match req.note_id.as_deref().map(validate_note_id) {
        Some(Ok(id)) => id,
        Some(Err(e)) => return ToolOutput::error(e),
        None => return ToolOutput::error("write requires 'note_id'"),
    };
    let content = match req.content.as_deref() {
        Some(c) if !c.is_empty() => c,
        _ => return ToolOutput::error("write requires non-empty 'content'"),
    };

    log(LOG_INFO, &format!("notepad: write note_id={note_id}"));

    if let Err(e) = fs::create_dir_all(notes_dir()) {
        log(LOG_ERROR, &format!("notepad: mkdir failed: {e}"));
        return ToolOutput::error(format!("failed to create notes directory: {e}"));
    }

    match fs::write(note_path(note_id), content) {
        Ok(()) => {
            log(LOG_DEBUG, &format!("notepad: wrote {} bytes", content.len()));
            ToolOutput::success(
                json!({ "status": "written", "note_id": note_id, "bytes": content.len() })
                    .to_string(),
            )
        }
        Err(e) => {
            log(LOG_ERROR, &format!("notepad: write failed: {e}"));
            ToolOutput::error(format!("failed to write note '{note_id}': {e}"))
        }
    }
}

fn handle_read(req: Request) -> ToolOutput {
    let note_id = match req.note_id.as_deref().map(validate_note_id) {
        Some(Ok(id)) => id,
        Some(Err(e)) => return ToolOutput::error(e),
        None => return ToolOutput::error("read requires 'note_id'"),
    };

    log(LOG_DEBUG, &format!("notepad: read note_id={note_id}"));

    match fs::read_to_string(note_path(note_id)) {
        Ok(content) => ToolOutput::success(content),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            ToolOutput::error(format!("note '{note_id}' does not exist"))
        }
        Err(e) => {
            log(LOG_ERROR, &format!("notepad: read failed: {e}"));
            ToolOutput::error(format!("failed to read note '{note_id}': {e}"))
        }
    }
}

fn handle_list() -> ToolOutput {
    log(LOG_DEBUG, "notepad: list");

    let dir = notes_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let empty: Vec<String> = vec![];
            return ToolOutput::success(json!({ "notes": empty }).to_string());
        }
        Err(e) => return ToolOutput::error(format!("failed to list notes: {e}")),
    };

    let mut ids: Vec<String> = entries
        .filter_map(|entry| {
            let name = entry.ok()?.file_name();
            let name = name.to_string_lossy();
            name.strip_suffix(".md").map(|id| id.to_string())
        })
        .collect();
    ids.sort();

    ToolOutput::success(json!({ "notes": ids }).to_string())
}

fn handle_delete(req: Request) -> ToolOutput {
    let note_id = match req.note_id.as_deref().map(validate_note_id) {
        Some(Ok(id)) => id,
        Some(Err(e)) => return ToolOutput::error(e),
        None => return ToolOutput::error("delete requires 'note_id'"),
    };

    log(LOG_INFO, &format!("notepad: delete note_id={note_id}"));

    match fs::remove_file(note_path(note_id)) {
        Ok(()) => {
            log(LOG_DEBUG, &format!("notepad: deleted {note_id}"));
            ToolOutput::success(
                json!({ "status": "deleted", "note_id": note_id }).to_string(),
            )
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // ToolOutput::with_warning: the operation is idempotent (delete-if-exists),
            // so we return a success output. The warning signals that the note was
            // already absent — useful for pipelines that want to distinguish
            // "deleted" from "was not there".
            log(LOG_WARN, &format!("notepad: note {note_id} not found"));
            ToolOutput::with_warning(
                json!({ "status": "not_found", "note_id": note_id }).to_string(),
                format!("note '{note_id}' did not exist; nothing was deleted"),
            )
        }
        Err(e) => {
            log(LOG_ERROR, &format!("notepad: delete failed: {e}"));
            ToolOutput::error(format!("failed to delete note '{note_id}': {e}"))
        }
    }
}
