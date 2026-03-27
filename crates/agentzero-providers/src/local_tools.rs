//! Shared utilities for local LLM providers (builtin + candle).
//!
//! Contains tool call parsing, ChatML prompt formatting, and JSON repair
//! logic that is common to all in-process inference backends.

use agentzero_core::types::{ConversationMessage, ToolDefinition, ToolUseRequest};
use tracing::warn;

// ---------------------------------------------------------------------------
// Tool call parsing
// ---------------------------------------------------------------------------

/// Parse tool calls from model output.
///
/// Handles three formats that local models commonly produce:
/// 1. `<tool_call>{"name":...}</tool_call>` — Qwen/ChatML style
/// 2. `` ```json\n{"name":...}\n``` `` — fenced code blocks
/// 3. Bare `{"name": "...", "arguments": {...}}` JSON objects
///
/// Returns `(text_output, tool_calls)` where `text_output` is the model
/// response with tool call blocks removed and `tool_calls` is the parsed list.
pub fn parse_tool_calls(raw: &str) -> (String, Vec<ToolUseRequest>) {
    let mut tool_calls = Vec::new();
    let mut text = String::new();
    let mut remaining = raw;
    let mut call_index = 0usize;

    // Pass 1: extract <tool_call> blocks
    loop {
        match remaining.find("<tool_call>") {
            None => {
                text.push_str(remaining);
                break;
            }
            Some(start) => {
                text.push_str(&remaining[..start]);

                let after_open = &remaining[start + "<tool_call>".len()..];
                match after_open.find("</tool_call>") {
                    None => {
                        text.push_str(&remaining[start..]);
                        break;
                    }
                    Some(end) => {
                        let json_str = after_open[..end].trim();
                        if let Some(tc) = parse_single_tool_call(json_str, call_index) {
                            call_index += 1;
                            tool_calls.push(tc);
                        } else {
                            warn!(json = json_str, "failed to parse tool_call JSON");
                            text.push_str(
                                &remaining[start
                                    ..start + "<tool_call>".len() + end + "</tool_call>".len()],
                            );
                        }
                        remaining = &after_open[end + "</tool_call>".len()..];
                    }
                }
            }
        }
    }

    // Pass 2: if no <tool_call> blocks found, try ```json code blocks and bare JSON
    if tool_calls.is_empty() {
        let cleaned = text.clone();
        text.clear();
        remaining = &cleaned;

        loop {
            // Try fenced code block first
            if let Some(fence_start) = remaining.find("```") {
                let after_fence = &remaining[fence_start + 3..];
                // Skip optional language tag
                if let Some(newline) = after_fence.find('\n') {
                    let content = &after_fence[newline + 1..];
                    if let Some(fence_end) = content.find("```") {
                        let block = content[..fence_end].trim();
                        if let Some(tc) = parse_single_tool_call(block, call_index) {
                            call_index += 1;
                            text.push_str(&remaining[..fence_start]);
                            tool_calls.push(tc);
                            remaining = &content[fence_end + 3..];
                            continue;
                        }
                    }
                }
                // Not a tool call code block — keep it as text
                text.push_str(&remaining[..fence_start + 3]);
                remaining = &remaining[fence_start + 3..];
                continue;
            }
            // No more fences
            text.push_str(remaining);
            break;
        }

        // Pass 3: still nothing? try bare JSON object
        if tool_calls.is_empty() {
            let trimmed = text.trim();
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                if let Some(tc) = parse_single_tool_call(trimmed, call_index) {
                    tool_calls.push(tc);
                    text.clear();
                }
            }
        }
    }

    let text = text.trim().to_string();
    (text, tool_calls)
}

/// Parse a single tool call JSON object.
///
/// Accepts `{"name": "...", "arguments": {...}}` with several key aliases:
/// - `"arguments"`, `"parameters"`, `"params"`, or `"input"` for the args
/// - `"function"` as alias for `"name"`
///
/// Falls back to [`try_repair_json`] if initial parse fails.
fn parse_single_tool_call(json_str: &str, index: usize) -> Option<ToolUseRequest> {
    // Try direct parse first
    if let Some(tc) = try_parse_tool_json(json_str, index) {
        return Some(tc);
    }

    // Try repairing common small-model JSON mistakes
    if let Some(repaired) = try_repair_json(json_str) {
        if let Some(tc) = try_parse_tool_json(&repaired, index) {
            return Some(tc);
        }
    }

    None
}

/// Attempt to parse a well-formed JSON tool call.
fn try_parse_tool_json(json_str: &str, index: usize) -> Option<ToolUseRequest> {
    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let name = v
        .get("name")
        .or_else(|| v.get("function"))
        .and_then(|v| v.as_str())?;
    let arguments = v
        .get("arguments")
        .or_else(|| v.get("parameters"))
        .or_else(|| v.get("params"))
        .or_else(|| v.get("input"))
        .cloned()
        .unwrap_or(serde_json::Value::Object(Default::default()));

    Some(ToolUseRequest {
        id: format!("local_tc_{index}"),
        name: name.to_string(),
        input: arguments,
    })
}

/// Attempt to repair common JSON mistakes from small local models.
///
/// Handles:
/// - Trailing commas: `{"a": 1,}` → `{"a": 1}`
/// - Unquoted keys: `{name: "x"}` → `{"name": "x"}`
/// - Single-quoted strings: `{'name': 'x'}` → `{"name": "x"}`
fn try_repair_json(raw: &str) -> Option<String> {
    let mut s = raw.to_string();

    // Strip trailing commas before } or ]
    let trailing_comma = regex::Regex::new(r",\s*([}\]])").ok()?;
    s = trailing_comma.replace_all(&s, "$1").to_string();

    // Replace single quotes with double quotes (naive but covers most cases)
    if s.contains('\'') && !s.contains('"') {
        s = s.replace('\'', "\"");
    }

    // Add quotes around unquoted keys: {name: → {"name":
    let unquoted_key = regex::Regex::new(r"(?m)\{(\s*)(\w+)\s*:").ok()?;
    s = unquoted_key.replace_all(&s, "{$1\"$2\":").to_string();
    let unquoted_key_mid = regex::Regex::new(r"(?m),(\s*)(\w+)\s*:").ok()?;
    s = unquoted_key_mid.replace_all(&s, ",$1\"$2\":").to_string();

    // Only return if different from input (repair actually changed something)
    if s != raw {
        Some(s)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// ChatML prompt formatting
// ---------------------------------------------------------------------------

/// Format conversation messages into a ChatML prompt string with optional
/// tool definitions injected into the system prompt.
pub fn format_chatml_prompt(messages: &[ConversationMessage], tools: &[ToolDefinition]) -> String {
    let mut prompt = String::new();
    let mut has_system = false;

    for msg in messages {
        match msg {
            ConversationMessage::System { content } => {
                prompt.push_str("<|im_start|>system\n");
                prompt.push_str(content);
                if !tools.is_empty() {
                    prompt.push_str(&format_tools_system_block(tools));
                }
                prompt.push_str("<|im_end|>\n");
                has_system = true;
            }
            ConversationMessage::User { content, .. } => {
                if !has_system && !tools.is_empty() {
                    prompt.push_str("<|im_start|>system\n");
                    prompt.push_str("You are a helpful assistant.");
                    prompt.push_str(&format_tools_system_block(tools));
                    prompt.push_str("<|im_end|>\n");
                    has_system = true;
                }
                prompt.push_str("<|im_start|>user\n");
                prompt.push_str(content);
                prompt.push_str("<|im_end|>\n");
            }
            ConversationMessage::Assistant {
                content,
                tool_calls,
            } => {
                prompt.push_str("<|im_start|>assistant\n");
                if let Some(text) = content {
                    prompt.push_str(text);
                }
                for tc in tool_calls {
                    prompt.push_str("\n<tool_call>\n");
                    let call_json = serde_json::json!({
                        "name": tc.name,
                        "arguments": tc.input,
                    });
                    prompt.push_str(&serde_json::to_string(&call_json).unwrap_or_default());
                    prompt.push_str("\n</tool_call>");
                }
                prompt.push_str("<|im_end|>\n");
            }
            ConversationMessage::ToolResult(result) => {
                prompt.push_str("<|im_start|>tool\n");
                prompt.push_str(&result.content);
                prompt.push_str("<|im_end|>\n");
            }
        }
    }

    // Start assistant turn
    prompt.push_str("<|im_start|>assistant\n");
    prompt
}

/// Build the tool-definition block appended to the system prompt.
///
/// Uses a compact, model-agnostic format optimised for small (3B-7B) models:
/// - Markdown list of tool names, descriptions, and parameter signatures
/// - Simple `<tool_call>` JSON instruction (one tool at a time)
fn format_tools_system_block(tools: &[ToolDefinition]) -> String {
    let mut block = String::from("\n\n# Available Tools\n\n");

    for tool in tools {
        block.push_str(&format!("- **{}**: {}\n", tool.name, tool.description));

        if let Some(props) = tool
            .input_schema
            .get("properties")
            .and_then(|p| p.as_object())
        {
            let required: Vec<&str> = tool
                .input_schema
                .get("required")
                .and_then(|r| r.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            let params: Vec<String> = props
                .iter()
                .map(|(name, schema)| {
                    let typ = schema.get("type").and_then(|t| t.as_str()).unwrap_or("any");
                    let opt = if required.contains(&name.as_str()) {
                        ""
                    } else {
                        "?"
                    };
                    format!("{name}{opt}: {typ}")
                })
                .collect();

            if !params.is_empty() {
                block.push_str(&format!("  Parameters: {}\n", params.join(", ")));
            }
        }
    }

    block.push_str(
        "\n## How to call a tool\n\
         To use a tool, write ONLY a JSON object inside <tool_call> tags. Example:\n\
         <tool_call>\n\
         {\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}\n\
         </tool_call>\n\
         Call ONE tool at a time. Wait for the result before calling another.",
    );

    block
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::types::ToolResultMessage;

    // ── parse_tool_calls tests ─────────────────────────────────────────

    #[test]
    fn parse_tool_calls_extracts_single_call() {
        let raw = "I'll search for that.\n\
                    <tool_call>\n\
                    {\"name\": \"web_search\", \"arguments\": {\"query\": \"rust programming\"}}\n\
                    </tool_call>";

        let (text, calls) = parse_tool_calls(raw);
        assert_eq!(text, "I'll search for that.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "web_search");
        assert_eq!(calls[0].input["query"], "rust programming");
    }

    #[test]
    fn parse_tool_calls_extracts_multiple_calls() {
        let raw = "Let me do two things.\n\
                    <tool_call>\n\
                    {\"name\": \"web_search\", \"arguments\": {\"q\": \"a\"}}\n\
                    </tool_call>\n\
                    <tool_call>\n\
                    {\"name\": \"write\", \"arguments\": {\"f\": \"b\"}}\n\
                    </tool_call>";

        let (text, calls) = parse_tool_calls(raw);
        assert_eq!(text, "Let me do two things.");
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn parse_tool_calls_no_calls_returns_text() {
        let raw = "Just a normal response with no tool calls.";
        let (text, calls) = parse_tool_calls(raw);
        assert_eq!(text, raw);
        assert!(calls.is_empty());
    }

    #[test]
    fn parse_tool_calls_handles_malformed_json() {
        let raw = "Trying something.\n\
                    <tool_call>\n\
                    {not valid json}\n\
                    </tool_call>";

        let (text, calls) = parse_tool_calls(raw);
        assert!(text.contains("{not valid json}"));
        assert!(calls.is_empty());
    }

    #[test]
    fn parse_tool_calls_handles_unterminated_tag() {
        let raw = "Some text\n<tool_call>\n{\"name\": \"x\"}";
        let (text, calls) = parse_tool_calls(raw);
        assert!(text.contains("<tool_call>"));
        assert!(calls.is_empty());
    }

    #[test]
    fn parse_tool_calls_handles_missing_arguments() {
        let raw = "<tool_call>\n\
                    {\"name\": \"simple_tool\"}\n\
                    </tool_call>";

        let (_, calls) = parse_tool_calls(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "simple_tool");
        assert!(calls[0].input.is_object());
    }

    #[test]
    fn parse_tool_calls_extracts_from_json_code_block() {
        let raw = "I'll search.\n```json\n{\"name\": \"web_search\", \"arguments\": {\"query\": \"AI\"}}\n```";
        let (text, calls) = parse_tool_calls(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "web_search");
        assert_eq!(text, "I'll search.");
    }

    #[test]
    fn parse_tool_calls_extracts_from_bare_json() {
        let raw = "{\"name\": \"web_search\", \"arguments\": {\"query\": \"test\"}}";
        let (text, calls) = parse_tool_calls(raw);
        assert_eq!(calls.len(), 1);
        assert!(text.is_empty());
    }

    #[test]
    fn parse_tool_calls_accepts_parameters_alias() {
        let raw = "<tool_call>{\"name\": \"web_fetch\", \"parameters\": {\"url\": \"https://example.com\"}}</tool_call>";
        let (_, calls) = parse_tool_calls(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].input["url"], "https://example.com");
    }

    #[test]
    fn parse_tool_calls_accepts_function_alias() {
        let raw = "<tool_call>{\"function\": \"shell\", \"params\": {\"cmd\": \"ls\"}}</tool_call>";
        let (_, calls) = parse_tool_calls(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(calls[0].input["cmd"], "ls");
    }

    // ── try_repair_json tests ──────────────────────────────────────────

    #[test]
    fn repair_trailing_comma() {
        let bad = r#"{"name": "search", "arguments": {"query": "rust",}}"#;
        let repaired = try_repair_json(bad).expect("should repair");
        let v: serde_json::Value = serde_json::from_str(&repaired).expect("valid JSON");
        assert_eq!(v["name"], "search");
    }

    #[test]
    fn repair_single_quotes() {
        let bad = "{'name': 'search', 'arguments': {'query': 'rust'}}";
        let repaired = try_repair_json(bad).expect("should repair");
        let v: serde_json::Value = serde_json::from_str(&repaired).expect("valid JSON");
        assert_eq!(v["name"], "search");
    }

    #[test]
    fn repair_unquoted_keys() {
        let bad = r#"{name: "search", arguments: {"query": "rust"}}"#;
        let repaired = try_repair_json(bad).expect("should repair");
        let v: serde_json::Value = serde_json::from_str(&repaired).expect("valid JSON");
        assert_eq!(v["name"], "search");
    }

    #[test]
    fn repair_returns_none_for_valid_json() {
        let good = r#"{"name": "search"}"#;
        assert!(try_repair_json(good).is_none());
    }

    #[test]
    fn parse_single_tool_call_with_repaired_json() {
        // Trailing comma — should be repaired and parsed
        let bad = r#"{"name": "web_search", "arguments": {"query": "test",}}"#;
        let tc = parse_single_tool_call(bad, 0).expect("should parse after repair");
        assert_eq!(tc.name, "web_search");
    }

    // ── ChatML formatting tests ────────────────────────────────────────

    #[test]
    fn format_chatml_basic_conversation() {
        let messages = vec![
            ConversationMessage::System {
                content: "You are helpful.".to_string(),
            },
            ConversationMessage::User {
                content: "Hello".to_string(),
                parts: vec![],
            },
        ];
        let formatted = format_chatml_prompt(&messages, &[]);
        assert!(formatted.contains("<|im_start|>system"));
        assert!(formatted.contains("You are helpful."));
        assert!(formatted.contains("<|im_start|>user"));
        assert!(formatted.contains("Hello"));
        assert!(formatted.ends_with("<|im_start|>assistant\n"));
    }

    #[test]
    fn format_chatml_with_tools_injects_block() {
        let tools = vec![ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "query": {"type": "string"} },
                "required": ["query"]
            }),
        }];
        let messages = vec![ConversationMessage::User {
            content: "Search".to_string(),
            parts: vec![],
        }];
        let formatted = format_chatml_prompt(&messages, &tools);
        assert!(formatted.contains("# Available Tools"));
        assert!(formatted.contains("web_search"));
        assert!(formatted.contains("<tool_call>"));
    }

    #[test]
    fn format_chatml_replays_tool_history() {
        let messages = vec![
            ConversationMessage::User {
                content: "Search for AI".to_string(),
                parts: vec![],
            },
            ConversationMessage::Assistant {
                content: Some("Searching.".to_string()),
                tool_calls: vec![ToolUseRequest {
                    id: "tc_0".to_string(),
                    name: "web_search".to_string(),
                    input: serde_json::json!({"query": "AI"}),
                }],
            },
            ConversationMessage::ToolResult(ToolResultMessage {
                tool_use_id: "tc_0".to_string(),
                content: "Found results.".to_string(),
                is_error: false,
            }),
        ];
        let formatted = format_chatml_prompt(&messages, &[]);
        assert!(formatted.contains("<tool_call>"));
        assert!(formatted.contains("web_search"));
        assert!(formatted.contains("<|im_start|>tool"));
        assert!(formatted.contains("Found results."));
    }
}
