//! Integration tests for the `#[tool_fn]` proc macro.

use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::tool_fn;

// ---------------------------------------------------------------------------
// Basic: stateless tool with input params
// ---------------------------------------------------------------------------

/// Reverse the input string.
#[tool_fn(name = "reverse_string")]
async fn reverse_string(
    /// The text to reverse
    text: String,
    #[ctx] _ctx: &ToolContext,
) -> anyhow::Result<ToolResult> {
    Ok(ToolResult {
        output: text.chars().rev().collect(),
    })
}

#[tokio::test]
async fn basic_tool_name_and_description() {
    let tool = ReverseStringTool;
    assert_eq!(tool.name(), "reverse_string");
    assert_eq!(tool.description(), "Reverse the input string.");
}

#[tokio::test]
async fn basic_tool_schema() {
    let tool = ReverseStringTool;
    let schema = tool.input_schema().expect("should have schema");
    let props = &schema["properties"];
    assert_eq!(props["text"]["type"], "string");
    assert_eq!(props["text"]["description"], "The text to reverse");
    let required = schema["required"].as_array().expect("required array");
    assert_eq!(required.len(), 1);
    assert_eq!(required[0], "text");
}

#[tokio::test]
async fn basic_tool_execute() {
    let tool = ReverseStringTool;
    let ctx = ToolContext::new(".".to_string());
    let result = tool
        .execute(r#"{"text": "hello"}"#, &ctx)
        .await
        .expect("should succeed");
    assert_eq!(result.output, "olleh");
}

// ---------------------------------------------------------------------------
// Optional params
// ---------------------------------------------------------------------------

/// Greet someone.
#[tool_fn(name = "greet")]
async fn greet(
    /// Name of the person
    name: String,
    /// Optional greeting prefix
    #[serde(default)]
    prefix: Option<String>,
    #[ctx] _ctx: &ToolContext,
) -> anyhow::Result<ToolResult> {
    let p = prefix.unwrap_or_else(|| "Hello".to_string());
    Ok(ToolResult {
        output: format!("{p}, {name}!"),
    })
}

#[tokio::test]
async fn optional_param_not_in_required() {
    let tool = GreetTool;
    let schema = tool.input_schema().expect("schema");
    let required = schema["required"].as_array().expect("required");
    assert_eq!(required.len(), 1);
    assert_eq!(required[0], "name");
}

#[tokio::test]
async fn optional_param_defaults() {
    let tool = GreetTool;
    let ctx = ToolContext::new(".".to_string());
    let result = tool
        .execute(r#"{"name": "World"}"#, &ctx)
        .await
        .expect("should succeed");
    assert_eq!(result.output, "Hello, World!");
}

#[tokio::test]
async fn optional_param_provided() {
    let tool = GreetTool;
    let ctx = ToolContext::new(".".to_string());
    let result = tool
        .execute(r#"{"name": "World", "prefix": "Hi"}"#, &ctx)
        .await
        .expect("should succeed");
    assert_eq!(result.output, "Hi, World!");
}

// ---------------------------------------------------------------------------
// No input params (tool takes no structured input)
// ---------------------------------------------------------------------------

/// Return the current timestamp.
#[tool_fn(name = "timestamp")]
async fn timestamp(#[ctx] _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
    Ok(ToolResult {
        output: "1234567890".to_string(),
    })
}

#[tokio::test]
async fn no_input_schema_is_none() {
    let tool = TimestampTool;
    assert!(tool.input_schema().is_none());
}

#[tokio::test]
async fn no_input_execute() {
    let tool = TimestampTool;
    let ctx = ToolContext::new(".".to_string());
    let result = tool.execute("", &ctx).await.expect("should succeed");
    assert_eq!(result.output, "1234567890");
}

// ---------------------------------------------------------------------------
// Description override via attribute
// ---------------------------------------------------------------------------

/// This doc comment should be ignored.
#[tool_fn(name = "echo", description = "Echo the input back.")]
async fn echo(
    /// The text to echo
    text: String,
    #[ctx] _ctx: &ToolContext,
) -> anyhow::Result<ToolResult> {
    Ok(ToolResult { output: text })
}

#[tokio::test]
async fn description_override() {
    let tool = EchoTool;
    assert_eq!(tool.description(), "Echo the input back.");
}

// ---------------------------------------------------------------------------
// Integer and boolean params
// ---------------------------------------------------------------------------

/// Repeat text N times.
#[tool_fn(name = "repeat")]
async fn repeat(
    /// The text to repeat
    text: String,
    /// Number of repetitions
    count: u32,
    /// Whether to add newlines between repetitions
    #[serde(default)]
    newlines: Option<bool>,
    #[ctx] _ctx: &ToolContext,
) -> anyhow::Result<ToolResult> {
    let sep = if newlines.unwrap_or(false) { "\n" } else { "" };
    let output: String = (0..count)
        .map(|_| text.as_str())
        .collect::<Vec<_>>()
        .join(sep);
    Ok(ToolResult { output })
}

#[tokio::test]
async fn integer_and_bool_schema() {
    let tool = RepeatTool;
    let schema = tool.input_schema().expect("schema");
    assert_eq!(schema["properties"]["count"]["type"], "integer");
    assert_eq!(schema["properties"]["newlines"]["type"], "boolean");
}

// ---------------------------------------------------------------------------
// Stateful tool with #[state]
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct GreeterConfig {
    default_greeting: String,
}

/// Greet with configurable default.
#[tool_fn(name = "configurable_greet")]
async fn configurable_greet(
    /// Name of the person
    name: String,
    #[state] config: &GreeterConfig,
    #[ctx] _ctx: &ToolContext,
) -> anyhow::Result<ToolResult> {
    Ok(ToolResult {
        output: format!("{}, {}!", config.default_greeting, name),
    })
}

#[tokio::test]
async fn stateful_tool_constructor() {
    let tool = ConfigurableGreetTool::new(GreeterConfig {
        default_greeting: "Howdy".to_string(),
    });
    let ctx = ToolContext::new(".".to_string());
    let result = tool
        .execute(r#"{"name": "Partner"}"#, &ctx)
        .await
        .expect("should succeed");
    assert_eq!(result.output, "Howdy, Partner!");
}

// ---------------------------------------------------------------------------
// Invalid JSON input
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invalid_json_returns_error() {
    let tool = ReverseStringTool;
    let ctx = ToolContext::new(".".to_string());
    let err = tool
        .execute("not json", &ctx)
        .await
        .expect_err("should fail");
    assert!(err.to_string().contains("reverse_string"));
}

// ---------------------------------------------------------------------------
// Vec<T> param
// ---------------------------------------------------------------------------

/// Join strings with a separator.
#[tool_fn(name = "join_strings")]
async fn join_strings(
    /// The strings to join
    parts: Vec<String>,
    /// Separator between parts
    #[serde(default)]
    separator: Option<String>,
    #[ctx] _ctx: &ToolContext,
) -> anyhow::Result<ToolResult> {
    let sep = separator.unwrap_or_else(|| ", ".to_string());
    Ok(ToolResult {
        output: parts.join(&sep),
    })
}

#[tokio::test]
async fn vec_param_schema() {
    let tool = JoinStringsTool;
    let schema = tool.input_schema().expect("schema");
    assert_eq!(schema["properties"]["parts"]["type"], "array");
    // Vec<T> auto-detects as array; items defaults to "string"
    assert_eq!(schema["properties"]["parts"]["items"]["type"], "string");
}

#[tokio::test]
async fn vec_param_execute() {
    let tool = JoinStringsTool;
    let ctx = ToolContext::new(".".to_string());
    let result = tool
        .execute(r#"{"parts": ["a", "b", "c"]}"#, &ctx)
        .await
        .expect("should succeed");
    assert_eq!(result.output, "a, b, c");
}
