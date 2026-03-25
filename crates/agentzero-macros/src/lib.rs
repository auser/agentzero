//! Proc macros for reducing boilerplate in AgentZero tool definitions.
//!
//! Provides two macros:
//! - `#[tool(name = "...", description = "...")]` — generates `name()` and `description()` on Tool impl
//! - `#[derive(ToolSchema)]` — generates `input_schema()` from struct fields and doc comments

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod tool_attr;
mod tool_schema;

/// Attribute macro for Tool structs that generates `name()` and `description()` trait methods.
///
/// # Usage
/// ```ignore
/// #[tool(name = "read_file", description = "Read file contents")]
/// pub struct ReadFileTool { /* ... */ }
/// ```
///
/// This generates an `impl` block with `name()` and `description()` methods matching the
/// `Tool` trait signature, returning `&'static str`.
#[proc_macro_attribute]
pub fn tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    tool_attr::expand(attr, item)
}

/// Derive macro that generates `input_schema()` returning a JSON Schema from struct fields.
///
/// # Usage
/// ```ignore
/// #[derive(ToolSchema, Deserialize)]
/// struct ReadFileInput {
///     /// Relative path to the file to read
///     path: String,
///     /// Start line offset
///     offset: Option<u64>,
/// }
/// ```
///
/// Generates a function `ReadFileInput::schema() -> serde_json::Value` that returns:
/// ```json
/// {
///   "type": "object",
///   "properties": {
///     "path": { "type": "string", "description": "Relative path to the file to read" },
///     "offset": { "type": "integer", "description": "Start line offset" }
///   },
///   "required": ["path"]
/// }
/// ```
///
/// # Field attributes
/// - `#[schema(enum_values = ["a", "b"])]` — adds `"enum"` constraint
/// - `#[schema(items_type = "string")]` — for `Vec<T>`, overrides the items type
/// - `Option<T>` fields are automatically excluded from `required`
/// - `#[serde(default)]` fields are also excluded from `required`
#[proc_macro_derive(ToolSchema, attributes(schema, serde))]
pub fn derive_tool_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    tool_schema::expand(input)
}
