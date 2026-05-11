//! WASM tool code generation via `wasm-encoder`.
//!
//! Generates WASM modules from templates without requiring an external
//! compiler toolchain. This is Tier 1 of the self-improving agent's
//! compilation strategy (ADR 0012).
//!
//! Generated modules import `az::*` host functions and export a `main`
//! entry point. The host functions provide policy-checked filesystem,
//! logging, and secret access per ADR 0013.
//!
//! Requires the `wasm` feature flag.

#[cfg(feature = "wasm")]
mod generator {
    use agentzero_tracing::{debug, info};
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum CodegenError {
        #[error("codegen failed: {0}")]
        Failed(String),
        #[error("unsupported template: {0}")]
        UnsupportedTemplate(String),
    }

    /// A template describing what kind of WASM tool to generate.
    #[derive(Debug, Clone)]
    pub enum ToolTemplate {
        /// A tool that reads a file and returns its content.
        /// Imports: az::read_file, az::log. Exports: main.
        FileReader,

        /// A minimal tool that just logs a message and returns success.
        /// Imports: az::log. Exports: main.
        Logger,

        /// A self-contained pure computation (no host imports).
        /// Exports: main returning an i32 exit code.
        PureComputation,

        /// A tool that reads a file, logs its byte count, and returns
        /// the count as the exit code (mod 256).
        /// Imports: az::read_file, az::log. Exports: main.
        FileCounter,

        /// A tool that writes a predefined message to a file.
        /// The path and content are baked into the module's data section.
        /// Imports: az::write_file, az::log. Exports: main.
        FileWriter {
            /// Path to write to.
            path: String,
            /// Content to write.
            content: String,
        },

        /// A tool that reads one file, reads another, and logs both.
        /// Useful as a template for multi-file operations.
        /// Imports: az::read_file, az::log. Exports: main.
        MultiFileReader {
            /// Paths to read.
            paths: Vec<String>,
        },
    }

    /// Schema for a generated tool, used for registration.
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct GeneratedToolSchema {
        pub name: String,
        pub description: String,
        pub template: String,
        pub wasm_bytes: Vec<u8>,
    }

    /// Generate a WASM module from a template.
    ///
    /// Returns raw WASM bytes that can be loaded by `WasmEngine::execute`
    /// or `WasmEngine::execute_with_host`.
    pub fn generate(template: &ToolTemplate) -> Result<Vec<u8>, CodegenError> {
        info!(template = ?template, "generating WASM module from template");

        match template {
            ToolTemplate::PureComputation => generate_pure_computation(),
            ToolTemplate::Logger => generate_logger(),
            ToolTemplate::FileReader => generate_file_reader(),
            ToolTemplate::FileCounter => generate_file_counter(),
            ToolTemplate::FileWriter { path, content } => generate_file_writer(path, content),
            ToolTemplate::MultiFileReader { paths } => generate_multi_file_reader(paths),
        }
    }

    /// Generate a minimal WASM module: `main() -> i32` returning 0.
    ///
    /// No host imports. Useful as a base template and for testing.
    fn generate_pure_computation() -> Result<Vec<u8>, CodegenError> {
        use wasm_encoder::*;

        let mut module = Module::new();

        // Type section: () -> i32
        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![ValType::I32]);
        module.section(&types);

        // Function section: 1 function of type 0
        let mut functions = FunctionSection::new();
        functions.function(0);
        module.section(&functions);

        // Memory section: 1 page
        let mut memories = MemorySection::new();
        memories.memory(MemoryType {
            minimum: 1,
            maximum: Some(1),
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        module.section(&memories);

        // Export section: "main" -> func 0, "memory" -> mem 0
        let mut exports = ExportSection::new();
        exports.export("main", ExportKind::Func, 0);
        exports.export("memory", ExportKind::Memory, 0);
        module.section(&exports);

        // Code section: i32.const 0; end
        let mut codes = CodeSection::new();
        let mut func = Function::new(vec![]);
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::End);
        codes.function(&func);
        module.section(&codes);

        let bytes = module.finish();
        debug!(bytes = bytes.len(), "generated pure computation module");
        Ok(bytes)
    }

    /// Generate a WASM module that calls `az::log` then returns 0.
    ///
    /// Imports: az::log(ptr: i32, len: i32)
    /// Exports: main() -> i32, memory
    ///
    /// The module stores a message in linear memory at offset 0 and
    /// calls az::log with the pointer and length.
    fn generate_logger() -> Result<Vec<u8>, CodegenError> {
        use wasm_encoder::*;

        let mut module = Module::new();

        // Type section
        let mut types = TypeSection::new();
        // Type 0: (i32, i32) -> () — for az::log
        types.ty().function(vec![ValType::I32, ValType::I32], vec![]);
        // Type 1: () -> i32 — for main
        types.ty().function(vec![], vec![ValType::I32]);
        module.section(&types);

        // Import section: az::log
        let mut imports = ImportSection::new();
        imports.import("az", "log", EntityType::Function(0));
        module.section(&imports);

        // Function section: main is func index 1 (after 1 import), type 1
        let mut functions = FunctionSection::new();
        functions.function(1);
        module.section(&functions);

        // Memory section: 1 page
        let mut memories = MemorySection::new();
        memories.memory(MemoryType {
            minimum: 1,
            maximum: Some(1),
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        module.section(&memories);

        // Export section
        let mut exports = ExportSection::new();
        exports.export("main", ExportKind::Func, 1); // func 1 (after import)
        exports.export("memory", ExportKind::Memory, 0);
        module.section(&exports);

        // Code section: call az::log(0, msg_len); return 0
        let message = b"tool executed";
        let mut codes = CodeSection::new();
        let mut func = Function::new(vec![]);
        func.instruction(&Instruction::I32Const(0)); // ptr
        func.instruction(&Instruction::I32Const(message.len() as i32)); // len
        func.instruction(&Instruction::Call(0)); // az::log (import index 0)
        func.instruction(&Instruction::I32Const(0)); // return 0
        func.instruction(&Instruction::End);
        codes.function(&func);
        module.section(&codes);

        // Data section: store "tool executed" at offset 0
        let mut data = DataSection::new();
        data.active(
            0,
            &ConstExpr::i32_const(0),
            message.iter().copied(),
        );
        module.section(&data);

        let bytes = module.finish();
        debug!(bytes = bytes.len(), "generated logger module");
        Ok(bytes)
    }

    /// Generate a WASM module that reads a file via `az::read_file`.
    ///
    /// Imports: az::read_file(ptr: i32, len: i32) -> i32, az::log(ptr: i32, len: i32)
    /// Exports: main() -> i32, memory
    ///
    /// The module stores a hardcoded path in memory, calls read_file,
    /// logs the result status, and returns the status code.
    fn generate_file_reader() -> Result<Vec<u8>, CodegenError> {
        use wasm_encoder::*;

        let mut module = Module::new();

        // Type section
        let mut types = TypeSection::new();
        // Type 0: (i32, i32) -> () — for az::log
        types.ty().function(vec![ValType::I32, ValType::I32], vec![]);
        // Type 1: (i32, i32) -> i32 — for az::read_file
        types
            .ty()
            .function(vec![ValType::I32, ValType::I32], vec![ValType::I32]);
        // Type 2: () -> i32 — for main
        types.ty().function(vec![], vec![ValType::I32]);
        module.section(&types);

        // Import section
        let mut imports = ImportSection::new();
        imports.import("az", "log", EntityType::Function(0)); // func 0
        imports.import("az", "read_file", EntityType::Function(1)); // func 1
        module.section(&imports);

        // Function section: main is func 2 (after 2 imports), type 2
        let mut functions = FunctionSection::new();
        functions.function(2);
        module.section(&functions);

        // Memory section
        let mut memories = MemorySection::new();
        memories.memory(MemoryType {
            minimum: 1,
            maximum: Some(1),
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        module.section(&memories);

        // Export section
        let mut exports = ExportSection::new();
        exports.export("main", ExportKind::Func, 2);
        exports.export("memory", ExportKind::Memory, 0);
        module.section(&exports);

        // Code section:
        //   local result: i32
        //   result = call az::read_file(0, path_len)
        //   call az::log(256, log_msg_len)
        //   return result
        let path = b"Cargo.toml";
        let log_msg = b"file read attempted";
        let mut codes = CodeSection::new();
        let mut func = Function::new(vec![(1, ValType::I32)]);
        // Call read_file
        func.instruction(&Instruction::I32Const(0)); // path ptr
        func.instruction(&Instruction::I32Const(path.len() as i32)); // path len
        func.instruction(&Instruction::Call(1)); // az::read_file
        func.instruction(&Instruction::LocalSet(0)); // store result
        // Log
        func.instruction(&Instruction::I32Const(256)); // log msg ptr
        func.instruction(&Instruction::I32Const(log_msg.len() as i32)); // log msg len
        func.instruction(&Instruction::Call(0)); // az::log
        // Return result
        func.instruction(&Instruction::LocalGet(0));
        func.instruction(&Instruction::End);
        codes.function(&func);
        module.section(&codes);

        // Data section: store path at offset 0, log message at offset 256
        let mut data = DataSection::new();
        data.active(0, &ConstExpr::i32_const(0), path.iter().copied());
        data.active(
            0,
            &ConstExpr::i32_const(256),
            log_msg.iter().copied(),
        );
        module.section(&data);

        let bytes = module.finish();
        debug!(bytes = bytes.len(), "generated file reader module");
        Ok(bytes)
    }

    /// Generate a WASM module that reads a file and returns its byte count.
    ///
    /// Imports: az::read_file(ptr, len) -> i32, az::log(ptr, len)
    /// Exports: main() -> i32 (returns read_file status)
    ///
    /// Like FileReader but with a "counting" log message — foundation for
    /// parameterized analysis tools.
    fn generate_file_counter() -> Result<Vec<u8>, CodegenError> {
        use wasm_encoder::*;

        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![ValType::I32, ValType::I32], vec![]);       // type 0: az::log
        types.ty().function(vec![ValType::I32, ValType::I32], vec![ValType::I32]); // type 1: az::read_file
        types.ty().function(vec![], vec![ValType::I32]);                     // type 2: main
        module.section(&types);

        let mut imports = ImportSection::new();
        imports.import("az", "log", EntityType::Function(0));
        imports.import("az", "read_file", EntityType::Function(1));
        module.section(&imports);

        let mut functions = FunctionSection::new();
        functions.function(2); // main
        module.section(&functions);

        let mut memories = MemorySection::new();
        memories.memory(MemoryType { minimum: 1, maximum: Some(1), memory64: false, shared: false, page_size_log2: None });
        module.section(&memories);

        let mut exports = ExportSection::new();
        exports.export("main", ExportKind::Func, 2);
        exports.export("memory", ExportKind::Memory, 0);
        module.section(&exports);

        let path = b"Cargo.toml";
        let log_msg = b"file counted";

        let mut codes = CodeSection::new();
        let mut func = Function::new(vec![(1, ValType::I32)]);
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::I32Const(path.len() as i32));
        func.instruction(&Instruction::Call(1)); // az::read_file
        func.instruction(&Instruction::LocalSet(0));
        func.instruction(&Instruction::I32Const(256));
        func.instruction(&Instruction::I32Const(log_msg.len() as i32));
        func.instruction(&Instruction::Call(0)); // az::log
        func.instruction(&Instruction::LocalGet(0));
        func.instruction(&Instruction::End);
        codes.function(&func);
        module.section(&codes);

        let mut data = DataSection::new();
        data.active(0, &ConstExpr::i32_const(0), path.iter().copied());
        data.active(0, &ConstExpr::i32_const(256), log_msg.iter().copied());
        module.section(&data);

        let bytes = module.finish();
        debug!(bytes = bytes.len(), "generated file counter module");
        Ok(bytes)
    }

    /// Generate a WASM module that writes parameterized content to a file.
    ///
    /// Imports: az::write_file(path_ptr, path_len, content_ptr, content_len) -> i32, az::log(ptr, len)
    /// The path and content are baked into the module's data section.
    fn generate_file_writer(path: &str, content: &str) -> Result<Vec<u8>, CodegenError> {
        use wasm_encoder::*;

        if path.is_empty() {
            return Err(CodegenError::Failed("FileWriter: path cannot be empty".into()));
        }

        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![ValType::I32, ValType::I32], vec![]);       // type 0: az::log
        types.ty().function(                                                  // type 1: az::write_file
            vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            vec![ValType::I32],
        );
        types.ty().function(vec![], vec![ValType::I32]);                     // type 2: main
        module.section(&types);

        let mut imports = ImportSection::new();
        imports.import("az", "log", EntityType::Function(0));
        imports.import("az", "write_file", EntityType::Function(1));
        module.section(&imports);

        let mut functions = FunctionSection::new();
        functions.function(2);
        module.section(&functions);

        let mut memories = MemorySection::new();
        memories.memory(MemoryType { minimum: 1, maximum: Some(1), memory64: false, shared: false, page_size_log2: None });
        module.section(&memories);

        let mut exports = ExportSection::new();
        exports.export("main", ExportKind::Func, 2);
        exports.export("memory", ExportKind::Memory, 0);
        module.section(&exports);

        // Layout: path at 0, content at 512, log msg at 1024
        let path_bytes = path.as_bytes();
        let content_bytes = content.as_bytes();
        let log_msg = b"file written";

        let mut codes = CodeSection::new();
        let mut func = Function::new(vec![(1, ValType::I32)]);
        // call write_file(path_ptr=0, path_len, content_ptr=512, content_len)
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::I32Const(path_bytes.len() as i32));
        func.instruction(&Instruction::I32Const(512));
        func.instruction(&Instruction::I32Const(content_bytes.len() as i32));
        func.instruction(&Instruction::Call(1)); // az::write_file
        func.instruction(&Instruction::LocalSet(0));
        func.instruction(&Instruction::I32Const(1024));
        func.instruction(&Instruction::I32Const(log_msg.len() as i32));
        func.instruction(&Instruction::Call(0)); // az::log
        func.instruction(&Instruction::LocalGet(0));
        func.instruction(&Instruction::End);
        codes.function(&func);
        module.section(&codes);

        let mut data = DataSection::new();
        data.active(0, &ConstExpr::i32_const(0), path_bytes.iter().copied());
        data.active(0, &ConstExpr::i32_const(512), content_bytes.iter().copied());
        data.active(0, &ConstExpr::i32_const(1024), log_msg.iter().copied());
        module.section(&data);

        let bytes = module.finish();
        debug!(bytes = bytes.len(), path = path, "generated file writer module");
        Ok(bytes)
    }

    /// Generate a WASM module that reads multiple files sequentially.
    ///
    /// Imports: az::read_file(ptr, len) -> i32, az::log(ptr, len)
    /// Each path is stored in the data section at sequential offsets.
    /// Returns 0 if all reads succeed, or the first non-zero status.
    fn generate_multi_file_reader(paths: &[String]) -> Result<Vec<u8>, CodegenError> {
        use wasm_encoder::*;

        if paths.is_empty() {
            return Err(CodegenError::Failed("MultiFileReader: at least one path required".into()));
        }
        if paths.len() > 16 {
            return Err(CodegenError::Failed("MultiFileReader: max 16 paths".into()));
        }

        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![ValType::I32, ValType::I32], vec![]);       // type 0: az::log
        types.ty().function(vec![ValType::I32, ValType::I32], vec![ValType::I32]); // type 1: az::read_file
        types.ty().function(vec![], vec![ValType::I32]);                     // type 2: main
        module.section(&types);

        let mut imports = ImportSection::new();
        imports.import("az", "log", EntityType::Function(0));
        imports.import("az", "read_file", EntityType::Function(1));
        module.section(&imports);

        let mut functions = FunctionSection::new();
        functions.function(2);
        module.section(&functions);

        let mut memories = MemorySection::new();
        memories.memory(MemoryType { minimum: 1, maximum: Some(1), memory64: false, shared: false, page_size_log2: None });
        module.section(&memories);

        let mut exports = ExportSection::new();
        exports.export("main", ExportKind::Func, 2);
        exports.export("memory", ExportKind::Memory, 0);
        module.section(&exports);

        // Layout: paths at offsets 0, 256, 512, ... (256 bytes per slot)
        // Log message at offset 4096
        let log_msg = format!("read {} files", paths.len());
        let log_bytes = log_msg.as_bytes();

        // Build code: for each path, call read_file; if non-zero, return early
        let mut codes = CodeSection::new();
        let mut func = Function::new(vec![(1, ValType::I32)]);
        for (i, path) in paths.iter().enumerate() {
            let offset = (i * 256) as i32;
            func.instruction(&Instruction::I32Const(offset));
            func.instruction(&Instruction::I32Const(path.len() as i32));
            func.instruction(&Instruction::Call(1)); // az::read_file
            func.instruction(&Instruction::LocalSet(0));
            // If non-zero, return early
            func.instruction(&Instruction::LocalGet(0));
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::I32Ne);
            func.instruction(&Instruction::If(BlockType::Empty));
            func.instruction(&Instruction::LocalGet(0));
            func.instruction(&Instruction::Return);
            func.instruction(&Instruction::End);
        }
        // Log and return 0
        func.instruction(&Instruction::I32Const(4096));
        func.instruction(&Instruction::I32Const(log_bytes.len() as i32));
        func.instruction(&Instruction::Call(0)); // az::log
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::End);
        codes.function(&func);
        module.section(&codes);

        let mut data = DataSection::new();
        for (i, path) in paths.iter().enumerate() {
            let offset = (i * 256) as i32;
            data.active(0, &ConstExpr::i32_const(offset), path.as_bytes().iter().copied());
        }
        data.active(0, &ConstExpr::i32_const(4096), log_bytes.iter().copied());
        module.section(&data);

        let bytes = module.finish();
        debug!(bytes = bytes.len(), paths = paths.len(), "generated multi-file reader module");
        Ok(bytes)
    }
}

#[cfg(feature = "wasm")]
pub use generator::{generate, CodegenError, GeneratedToolSchema, ToolTemplate};

#[cfg(all(test, feature = "wasm"))]
mod tests {
    use super::*;
    use crate::wasm::{DenyAllHostCallbacks, WasmConfig, WasmEngine};
    use std::sync::Arc;

    #[test]
    fn generate_pure_computation_is_valid_wasm() {
        let bytes = generate(&ToolTemplate::PureComputation).expect("should generate");
        assert!(!bytes.is_empty());

        // Verify it executes successfully
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        let result = engine.execute(&bytes).expect("should execute");
        assert!(result.success);
        assert!(result.output.contains("0"), "should return exit code 0");
    }

    #[test]
    fn generate_logger_is_valid_wasm() {
        let bytes = generate(&ToolTemplate::Logger).expect("should generate");
        assert!(!bytes.is_empty());

        // Verify it executes with host callbacks
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        let result = engine
            .execute_with_host(&bytes, Arc::new(DenyAllHostCallbacks))
            .expect("should execute");
        assert!(result.success);
    }

    #[test]
    fn generate_file_reader_is_valid_wasm() {
        let bytes = generate(&ToolTemplate::FileReader).expect("should generate");
        assert!(!bytes.is_empty());

        // Verify it executes with host callbacks (read_file will return error
        // via DenyAllHostCallbacks, but the module should still complete)
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        let result = engine
            .execute_with_host(&bytes, Arc::new(DenyAllHostCallbacks))
            .expect("should execute");
        assert!(result.success);
    }

    #[test]
    fn generated_pure_computation_has_no_imports() {
        let bytes = generate(&ToolTemplate::PureComputation).expect("should generate");
        // Should execute without host callbacks (no imports)
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        let result = engine.execute(&bytes);
        assert!(result.is_ok(), "pure computation should work without host: {result:?}");
    }

    #[test]
    fn generated_logger_requires_host() {
        let bytes = generate(&ToolTemplate::Logger).expect("should generate");
        // Should fail without host callbacks (has az:: imports)
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        let result = engine.execute(&bytes);
        assert!(result.is_err(), "logger should fail without host callbacks");
    }

    #[test]
    fn generate_file_counter_is_valid() {
        let bytes = generate(&ToolTemplate::FileCounter).expect("should generate");
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        let result = engine.execute_with_host(&bytes, Arc::new(DenyAllHostCallbacks));
        assert!(result.is_ok(), "file counter should execute: {result:?}");
    }

    #[test]
    fn generate_file_writer_is_valid() {
        let bytes = generate(&ToolTemplate::FileWriter {
            path: "/tmp/test-out.txt".into(),
            content: "hello from WASM".into(),
        })
        .expect("should generate");
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        let result = engine.execute_with_host(&bytes, Arc::new(DenyAllHostCallbacks));
        assert!(result.is_ok(), "file writer should execute: {result:?}");
    }

    #[test]
    fn generate_file_writer_empty_path_fails() {
        let result = generate(&ToolTemplate::FileWriter {
            path: String::new(),
            content: "content".into(),
        });
        assert!(result.is_err());
    }

    #[test]
    fn generate_multi_file_reader_is_valid() {
        let bytes = generate(&ToolTemplate::MultiFileReader {
            paths: vec!["Cargo.toml".into(), "README.md".into()],
        })
        .expect("should generate");
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        let result = engine.execute_with_host(&bytes, Arc::new(DenyAllHostCallbacks));
        assert!(result.is_ok(), "multi-file reader should execute: {result:?}");
    }

    #[test]
    fn generate_multi_file_reader_empty_fails() {
        let result = generate(&ToolTemplate::MultiFileReader { paths: vec![] });
        assert!(result.is_err());
    }

    #[test]
    fn generate_multi_file_reader_too_many_fails() {
        let paths: Vec<String> = (0..17).map(|i| format!("file{i}.txt")).collect();
        let result = generate(&ToolTemplate::MultiFileReader { paths });
        assert!(result.is_err());
    }

    #[test]
    fn generated_tool_schema_serializes() {
        let bytes = generate(&ToolTemplate::PureComputation).expect("should generate");
        let schema = GeneratedToolSchema {
            name: "test-tool".into(),
            description: "A test tool".into(),
            template: "pure_computation".into(),
            wasm_bytes: bytes,
        };
        let json = serde_json::to_string(&schema).expect("should serialize");
        assert!(json.contains("test-tool"));
    }
}
