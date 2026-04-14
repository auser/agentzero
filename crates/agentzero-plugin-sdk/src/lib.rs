//! AgentZero Plugin SDK
//!
//! Minimal SDK for building WASM plugins that integrate with AgentZero's
//! tool system. Plugins compile to `wasm32-wasip1` and are loaded by the
//! AgentZero runtime via the ABI v2 protocol.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use agentzero_plugin_sdk::prelude::*;
//!
//! declare_tool!("my_tool", execute);
//!
//! fn execute(input: ToolInput) -> ToolOutput {
//!     ToolOutput::success(format!("got: {}", input.input))
//! }
//! ```
//!
//! Build with: `cargo build --target wasm32-wasip1 --release`

pub mod prelude;

use serde::{Deserialize, Serialize};

/// Input provided to a plugin tool by the AgentZero runtime.
///
/// The runtime serializes this as JSON, writes it into WASM linear memory,
/// and passes the pointer/length to `az_tool_execute`.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolInput {
    /// The raw input string from the LLM tool call.
    pub input: String,
    /// Absolute path to the workspace root directory.
    pub workspace_root: String,
}

/// Output returned by a plugin tool to the AgentZero runtime.
///
/// Serialized as JSON and returned via a packed ptr|len i64.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// The tool's output text (shown to the LLM).
    pub output: String,
    /// Optional error message. If set with empty output, treated as a tool error.
    /// If set with non-empty output, appended as a warning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Privacy boundary for this output.  Controls which channels may receive
    /// it: `"local_only"` restricts to CLI/transcription, `"any"` (default)
    /// allows all channels.  The host respects this when routing output.
    #[serde(default = "default_boundary")]
    pub privacy_boundary: String,
}

fn default_boundary() -> String {
    "any".to_string()
}

impl ToolOutput {
    /// Create a successful output.
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            error: None,
            privacy_boundary: default_boundary(),
        }
    }

    /// Create an error output with no result text.
    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            output: String::new(),
            error: Some(msg.into()),
            privacy_boundary: default_boundary(),
        }
    }

    /// Create an output with both result text and a warning.
    pub fn with_warning(output: impl Into<String>, warning: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            error: Some(warning.into()),
            privacy_boundary: default_boundary(),
        }
    }

    /// Set the privacy boundary on this output (builder pattern).
    pub fn with_boundary(mut self, boundary: impl Into<String>) -> Self {
        self.privacy_boundary = boundary.into();
        self
    }
}

/// Pack a (ptr, len) pair into a single i64 for the ABI v2 return convention.
///
/// Layout: bits 0-31 = ptr, bits 32-63 = len.
#[inline]
pub fn pack_ptr_len(ptr: u32, len: u32) -> i64 {
    (ptr as i64) | ((len as i64) << 32)
}

/// Allocate `size` bytes via the Rust allocator and leak the allocation.
///
/// Returns a raw pointer suitable for sharing with the host via linear memory.
/// These allocations are intentionally leaked — WASM plugin instances are
/// short-lived and all memory is reclaimed when the instance is dropped.
#[inline]
pub fn sdk_alloc(size: usize) -> *mut u8 {
    let mut buf = vec![0u8; size];
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Write bytes into WASM linear memory at the given pointer.
///
/// # Safety
///
/// `dst` must point to at least `src.len()` bytes of valid, writable memory.
#[inline]
pub unsafe fn write_to_memory(dst: *mut u8, src: &[u8]) {
    core::ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len());
}

/// Declare a WASM plugin tool with ABI v2 exports.
///
/// Generates three required exports:
/// - `az_alloc(size: i32) -> i32` — allocator for host↔plugin memory sharing
/// - `az_tool_name() -> i64` — packed ptr|len of the tool name string
/// - `az_tool_execute(input_ptr: i32, input_len: i32) -> i64` — main entry point
///
/// # Usage
///
/// ```rust,ignore
/// use agentzero_plugin_sdk::prelude::*;
///
/// declare_tool!("my_tool", handler);
///
/// fn handler(input: ToolInput) -> ToolOutput {
///     ToolOutput::success("hello from plugin")
/// }
/// ```
///
/// The handler function must have signature `fn(ToolInput) -> ToolOutput`.
#[macro_export]
macro_rules! declare_tool {
    ($name:expr, $handler:ident) => {
        /// ABI v2 allocator export. Called by the host to allocate space in
        /// plugin linear memory for writing input data.
        #[no_mangle]
        pub extern "C" fn az_alloc(size: i32) -> i32 {
            $crate::sdk_alloc(size as usize) as i32
        }

        /// ABI v2 tool name export. Returns a packed ptr|len pointing to the
        /// tool name string in linear memory.
        #[no_mangle]
        pub extern "C" fn az_tool_name() -> i64 {
            let name: &[u8] = $name.as_bytes();
            let ptr = $crate::sdk_alloc(name.len());
            unsafe {
                $crate::write_to_memory(ptr, name);
            }
            $crate::pack_ptr_len(ptr as u32, name.len() as u32)
        }

        /// ABI v2 main entry point. Receives JSON input, calls the handler,
        /// and returns JSON output as a packed ptr|len.
        #[no_mangle]
        pub extern "C" fn az_tool_execute(input_ptr: i32, input_len: i32) -> i64 {
            // Read input bytes from linear memory
            let input_bytes =
                unsafe { core::slice::from_raw_parts(input_ptr as *const u8, input_len as usize) };

            // Deserialize input JSON
            let tool_input: $crate::ToolInput = match serde_json::from_slice(input_bytes) {
                Ok(v) => v,
                Err(e) => {
                    // Return a structured error on parse failure
                    let err_output =
                        $crate::ToolOutput::error(format!("failed to parse input: {}", e));
                    let json = serde_json::to_vec(&err_output).unwrap_or_default();
                    let ptr = $crate::sdk_alloc(json.len());
                    unsafe {
                        $crate::write_to_memory(ptr, &json);
                    }
                    return $crate::pack_ptr_len(ptr as u32, json.len() as u32);
                }
            };

            // Call the user's handler
            let output = $handler(tool_input);

            // Serialize output JSON
            let json = serde_json::to_vec(&output).unwrap_or_default();
            let ptr = $crate::sdk_alloc(json.len());
            unsafe {
                $crate::write_to_memory(ptr, &json);
            }
            $crate::pack_ptr_len(ptr as u32, json.len() as u32)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_output_success() {
        let out = ToolOutput::success("hello");
        assert_eq!(out.output, "hello");
        assert!(out.error.is_none());

        let json = serde_json::to_string(&out).unwrap();
        assert!(json.contains("\"output\":\"hello\""));
        assert!(!json.contains("error"));
    }

    #[test]
    fn tool_output_error() {
        let out = ToolOutput::error("something broke");
        assert!(out.output.is_empty());
        assert_eq!(out.error.as_deref(), Some("something broke"));

        let json = serde_json::to_string(&out).unwrap();
        assert!(json.contains("\"error\":\"something broke\""));
    }

    #[test]
    fn tool_output_with_warning() {
        let out = ToolOutput::with_warning("result", "heads up");
        assert_eq!(out.output, "result");
        assert_eq!(out.error.as_deref(), Some("heads up"));
    }

    #[test]
    fn tool_input_deserialize() {
        let json = r#"{"input":"test data","workspace_root":"/tmp/ws"}"#;
        let input: ToolInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.input, "test data");
        assert_eq!(input.workspace_root, "/tmp/ws");
    }

    #[test]
    fn pack_ptr_len_roundtrip() {
        // Test the same encoding the runtime uses
        let ptr: u32 = 0x1000;
        let len: u32 = 42;
        let packed = pack_ptr_len(ptr, len);

        let recovered_ptr = (packed & 0xFFFF_FFFF) as u32;
        let recovered_len = ((packed >> 32) & 0xFFFF_FFFF) as u32;
        assert_eq!(recovered_ptr, ptr);
        assert_eq!(recovered_len, len);
    }

    #[test]
    fn pack_ptr_len_zero() {
        let packed = pack_ptr_len(0, 0);
        assert_eq!(packed, 0);
    }

    #[test]
    fn pack_ptr_len_max_values() {
        let packed = pack_ptr_len(u32::MAX, u32::MAX);
        let recovered_ptr = (packed & 0xFFFF_FFFF) as u32;
        let recovered_len = ((packed >> 32) & 0xFFFF_FFFF) as u32;
        assert_eq!(recovered_ptr, u32::MAX);
        assert_eq!(recovered_len, u32::MAX);
    }

    #[test]
    fn sdk_alloc_returns_valid_pointer() {
        let ptr = sdk_alloc(64);
        assert!(!ptr.is_null());
        // Write to it to verify it's valid memory
        unsafe {
            write_to_memory(ptr, &[0xAB; 64]);
            assert_eq!(*ptr, 0xAB);
            assert_eq!(*ptr.add(63), 0xAB);
        }
    }

    #[test]
    fn sdk_alloc_zero_size() {
        // Zero-size allocation should not panic
        let ptr = sdk_alloc(0);
        // Pointer validity for zero-size is implementation-defined, just ensure no panic
        let _ = ptr;
    }

    // Verify that the declare_tool! macro expands without errors.
    fn test_handler(input: ToolInput) -> ToolOutput {
        ToolOutput::success(format!("echo: {}", input.input))
    }

    declare_tool!("test_plugin", test_handler);

    #[test]
    fn macro_generates_az_alloc() {
        // az_alloc returns a valid (non-null) pointer cast to i32.
        // On native 64-bit this truncates, but the allocation itself succeeds.
        let ptr = az_alloc(32);
        // Just verify no panic — pointer validity can only be tested on wasm32
        let _ = ptr;
    }

    // The remaining macro tests involve unpacking pointers from the packed i64
    // ABI format. This only works correctly on wasm32 where pointers are 32-bit.
    // On native 64-bit, the u32 truncation makes dereferencing unsafe.
    // These are covered by the integration test (build to wasm32 + execute via wasmtime).
    #[cfg(target_pointer_width = "32")]
    mod wasm_abi_tests {
        use super::*;

        #[test]
        fn macro_generates_az_tool_name() {
            let packed = az_tool_name();
            let ptr = (packed & 0xFFFF_FFFF) as u32;
            let len = ((packed >> 32) & 0xFFFF_FFFF) as u32;
            assert_eq!(len, 11); // "test_plugin".len()
            let name = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
            assert_eq!(name, b"test_plugin");
        }

        #[test]
        fn macro_generates_az_tool_execute() {
            let input_json = r#"{"input":"hello world","workspace_root":"/tmp"}"#;
            let input_bytes = input_json.as_bytes();

            let input_ptr = az_alloc(input_bytes.len() as i32);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    input_bytes.as_ptr(),
                    input_ptr as *mut u8,
                    input_bytes.len(),
                );
            }

            let packed = az_tool_execute(input_ptr, input_bytes.len() as i32);
            let out_ptr = (packed & 0xFFFF_FFFF) as u32;
            let out_len = ((packed >> 32) & 0xFFFF_FFFF) as u32;

            let output_bytes =
                unsafe { core::slice::from_raw_parts(out_ptr as *const u8, out_len as usize) };
            let output: ToolOutput = serde_json::from_slice(output_bytes).unwrap();
            assert_eq!(output.output, "echo: hello world");
            assert!(output.error.is_none());
        }

        #[test]
        fn macro_handles_invalid_input() {
            let bad_json = b"not valid json";
            let input_ptr = az_alloc(bad_json.len() as i32);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    bad_json.as_ptr(),
                    input_ptr as *mut u8,
                    bad_json.len(),
                );
            }

            let packed = az_tool_execute(input_ptr, bad_json.len() as i32);
            let out_ptr = (packed & 0xFFFF_FFFF) as u32;
            let out_len = ((packed >> 32) & 0xFFFF_FFFF) as u32;

            let output_bytes =
                unsafe { core::slice::from_raw_parts(out_ptr as *const u8, out_len as usize) };
            let output: ToolOutput = serde_json::from_slice(output_bytes).unwrap();
            assert!(output.output.is_empty());
            assert!(output.error.as_deref().unwrap().contains("failed to parse"));
        }
    }
}
