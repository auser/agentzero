//! AgentZero Brain — WASM guest plugin.
//!
//! Compiles to `wasm32-unknown-unknown`, exports `run(ptr, len) -> i64`
//! and `alloc(size) -> ptr`. Uses `az::*` host imports for filesystem
//! and clock access.

use agentzero_brain::{
    brain_capture, brain_init, brain_query, brain_today, format_results, load_config, BrainFs,
    InitOptions, QueryOptions,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};

// ---------------------------------------------------------------------------
// Host import declarations (az::* module)
// ---------------------------------------------------------------------------

#[link(wasm_import_module = "az")]
extern "C" {
    fn read_file(ptr: i32, len: i32) -> i64;
    fn write_file(path_ptr: i32, path_len: i32, content_ptr: i32, content_len: i32) -> i32;
    fn append_file(path_ptr: i32, path_len: i32, content_ptr: i32, content_len: i32) -> i32;
    fn list_dir(ptr: i32, len: i32) -> i64;
    fn create_dir(ptr: i32, len: i32) -> i32;
    fn file_exists(ptr: i32, len: i32) -> i32;
    fn now() -> i64;
    fn log(ptr: i32, len: i32);
}

// ---------------------------------------------------------------------------
// Bump allocator — exported for host string passing
// ---------------------------------------------------------------------------

static BUMP: AtomicU32 = AtomicU32::new(1024);

#[no_mangle]
pub extern "C" fn alloc(size: i32) -> i32 {
    let ptr = BUMP.fetch_add(size as u32, Ordering::SeqCst);
    ptr as i32
}

// ---------------------------------------------------------------------------
// Unpack a packed i64 (ptr << 32 | len) into a String
// ---------------------------------------------------------------------------

fn unpack_string(packed: i64) -> Option<String> {
    if packed == -1 {
        return None;
    }
    let ptr = (packed >> 32) as usize;
    let len = (packed & 0xFFFF_FFFF) as usize;
    if len == 0 {
        return Some(String::new());
    }
    // Safety: the host wrote valid UTF-8 into our linear memory via alloc
    let slice = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
    std::str::from_utf8(slice).ok().map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// WasmBrainFs — BrainFs impl using az::* host imports
// ---------------------------------------------------------------------------

struct WasmBrainFs;

impl BrainFs for WasmBrainFs {
    fn read_file(&self, path: &str) -> Result<String, String> {
        let packed = unsafe { read_file(path.as_ptr() as i32, path.len() as i32) };
        unpack_string(packed).ok_or_else(|| format!("read_file failed: {path}"))
    }

    fn write_file(&self, path: &str, content: &str) -> Result<bool, String> {
        let result = unsafe {
            write_file(
                path.as_ptr() as i32,
                path.len() as i32,
                content.as_ptr() as i32,
                content.len() as i32,
            )
        };
        if result == 0 {
            Ok(true)
        } else {
            Err(format!("write_file failed: {path}"))
        }
    }

    fn append_file(&self, path: &str, content: &str) -> Result<bool, String> {
        let result = unsafe {
            append_file(
                path.as_ptr() as i32,
                path.len() as i32,
                content.as_ptr() as i32,
                content.len() as i32,
            )
        };
        if result == 0 {
            Ok(true)
        } else {
            Err(format!("append_file failed: {path}"))
        }
    }

    fn list_dir(&self, path: &str) -> Result<Vec<String>, String> {
        let packed = unsafe { list_dir(path.as_ptr() as i32, path.len() as i32) };
        let json = unpack_string(packed).ok_or_else(|| format!("list_dir failed: {path}"))?;
        serde_json::from_str(&json).map_err(|e| format!("list_dir parse failed: {e}"))
    }

    fn create_dir(&self, path: &str) -> Result<bool, String> {
        let result = unsafe { create_dir(path.as_ptr() as i32, path.len() as i32) };
        if result == 0 {
            Ok(true)
        } else {
            Err(format!("create_dir failed: {path}"))
        }
    }

    fn file_exists(&self, path: &str) -> Result<bool, String> {
        let result = unsafe { file_exists(path.as_ptr() as i32, path.len() as i32) };
        match result {
            0 => Ok(true),   // exists
            1 => Ok(false),  // does not exist
            _ => Err(format!("file_exists failed: {path}")),
        }
    }

    fn now(&self) -> String {
        let packed = unsafe { now() };
        unpack_string(packed).unwrap_or_else(|| "1970-01-01T00:00:00".to_string())
    }
}

fn log_msg(msg: &str) {
    unsafe {
        log(msg.as_ptr() as i32, msg.len() as i32);
    }
}

// ---------------------------------------------------------------------------
// JSON protocol: input/output
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum BrainCommand {
    Init {
        root: String,
        #[serde(default)]
        force: bool,
        #[serde(default)]
        dry_run: bool,
    },
    Today {
        root: String,
        date: Option<String>,
    },
    Capture {
        root: String,
        message: String,
        date: Option<String>,
        section: Option<String>,
    },
    Query {
        root: String,
        term: String,
        #[serde(default)]
        include_raw: bool,
        #[serde(default)]
        json: bool,
        #[serde(default = "default_limit")]
        limit: usize,
    },
}

fn default_limit() -> usize {
    50
}

#[derive(Serialize)]
struct BrainResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ---------------------------------------------------------------------------
// run() export — entry point called by host
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn run(ptr: i32, len: i32) -> i64 {
    // Read input JSON from our linear memory
    let input = unsafe {
        let slice = std::slice::from_raw_parts(ptr as *const u8, len as usize);
        match std::str::from_utf8(slice) {
            Ok(s) => s.to_string(),
            Err(_) => return pack_error("invalid UTF-8 input"),
        }
    };

    let response = match serde_json::from_str::<BrainCommand>(&input) {
        Ok(cmd) => dispatch(cmd),
        Err(e) => BrainResponse {
            success: false,
            output: None,
            error: Some(format!("invalid command: {e}")),
        },
    };

    let json = match serde_json::to_string(&response) {
        Ok(j) => j,
        Err(_) => return pack_error("failed to serialize response"),
    };

    pack_output(&json)
}

fn dispatch(cmd: BrainCommand) -> BrainResponse {
    let fs = WasmBrainFs;

    match cmd {
        BrainCommand::Init {
            root,
            force,
            dry_run,
        } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(_) => agentzero_brain::BrainConfig::default(),
            };
            let opts = InitOptions { force, dry_run };
            match brain_init(&fs, &root, &config, &opts) {
                Ok(result) => {
                    log_msg(&format!("brain init: {}", result.summary()));
                    BrainResponse {
                        success: true,
                        output: Some(result.summary()),
                        error: None,
                    }
                }
                Err(e) => BrainResponse {
                    success: false,
                    output: None,
                    error: Some(e.to_string()),
                },
            }
        }

        BrainCommand::Today { root, date } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(e) => {
                    return BrainResponse {
                        success: false,
                        output: None,
                        error: Some(e.to_string()),
                    }
                }
            };
            match brain_today(&fs, &root, &config, date.as_deref()) {
                Ok(path) => BrainResponse {
                    success: true,
                    output: Some(path),
                    error: None,
                },
                Err(e) => BrainResponse {
                    success: false,
                    output: None,
                    error: Some(e.to_string()),
                },
            }
        }

        BrainCommand::Capture {
            root,
            message,
            date,
            section,
        } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(e) => {
                    return BrainResponse {
                        success: false,
                        output: None,
                        error: Some(e.to_string()),
                    }
                }
            };
            match brain_capture(
                &fs,
                &root,
                &config,
                &message,
                date.as_deref(),
                section.as_deref(),
            ) {
                Ok((path, entry)) => BrainResponse {
                    success: true,
                    output: Some(format!("{path}\n{entry}")),
                    error: None,
                },
                Err(e) => BrainResponse {
                    success: false,
                    output: None,
                    error: Some(e.to_string()),
                },
            }
        }

        BrainCommand::Query {
            root,
            term,
            include_raw,
            json,
            limit,
        } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(e) => {
                    return BrainResponse {
                        success: false,
                        output: None,
                        error: Some(e.to_string()),
                    }
                }
            };
            let opts = QueryOptions {
                include_raw,
                json,
                limit,
            };
            match brain_query(&fs, &root, &config, &term, &opts) {
                Ok(matches) => {
                    let formatted = format_results(&matches, json);
                    BrainResponse {
                        success: true,
                        output: Some(formatted),
                        error: None,
                    }
                }
                Err(e) => BrainResponse {
                    success: false,
                    output: None,
                    error: Some(e.to_string()),
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------
// String packing helpers — write output to our own memory
// ---------------------------------------------------------------------------

fn pack_output(s: &str) -> i64 {
    let ptr = alloc(s.len() as i32);
    // Safety: we just allocated this memory via our bump allocator
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr(), ptr as *mut u8, s.len());
    }
    ((ptr as i64) << 32) | (s.len() as i64)
}

fn pack_error(msg: &str) -> i64 {
    let response = BrainResponse {
        success: false,
        output: None,
        error: Some(msg.to_string()),
    };
    match serde_json::to_string(&response) {
        Ok(json) => pack_output(&json),
        Err(_) => -1,
    }
}
