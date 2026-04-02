//! Proc macros for reducing boilerplate in AgentZero tool definitions.
//!
//! Provides three macros:
//! - `#[tool(name = "...", description = "...")]` — generates `name()` and `description()` on Tool impl
//! - `#[derive(ToolSchema)]` — generates `input_schema()` from struct fields and doc comments
//! - `#[tool_fn(name = "...")]` — generates a full `Tool` impl from an async function

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod tool_attr;
mod tool_fn;
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

/// Function-level attribute macro that generates a complete `Tool` implementation from an
/// async function.
///
/// # Usage
/// ```ignore
/// /// Reverse the input string.
/// #[tool_fn(name = "reverse_string")]
/// async fn reverse_string(
///     /// The text to reverse
///     text: String,
///     #[ctx] ctx: &ToolContext,
/// ) -> anyhow::Result<ToolResult> {
///     Ok(ToolResult { output: text.chars().rev().collect() })
/// }
/// ```
///
/// This generates:
/// - `ReverseStringInput` struct with `#[derive(Deserialize)]`
/// - `ReverseStringTool` struct (unit struct for stateless tools)
/// - `Tool` trait impl with name, description (from doc comment), input_schema, and execute
///
/// # Special parameter attributes
/// - `#[ctx]` — marks the `&ToolContext` parameter (not included in schema)
/// - `#[state]` — marks a state parameter; generates a struct with that field + `new()` constructor
/// - `#[serde(default)]` — forwarded to the input struct field
/// - `#[schema(enum_values = [...])]` — adds enum constraint to the JSON schema
#[proc_macro_attribute]
pub fn tool_fn(attr: TokenStream, item: TokenStream) -> TokenStream {
    tool_fn::expand(attr, item)
}
