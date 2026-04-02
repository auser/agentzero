//! `tool_create` — LLM-callable tool for creating, listing, and deleting
//! dynamic tools at runtime. Created tools persist across sessions.

use crate::tools::dynamic_tool::{DynamicToolDef, DynamicToolRegistry, DynamicToolStrategy};
use agentzero_core::{Provider, Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// LLM-callable tool for runtime tool creation.
///
/// Actions:
/// - `create` — describe a tool in natural language → LLM derives the definition → registered
/// - `list` — enumerate all dynamic tools
/// - `delete` — remove a dynamic tool by name
/// - `export` — export a tool's definition as shareable JSON
/// - `import` — import a tool definition from JSON
///
/// Gated by `ctx.depth == 0` (only root agents can create tools).
#[tool(
    name = "tool_create",
    description = "Create, list, delete, export, or import dynamic tools at runtime. Created tools persist across sessions and are immediately available."
)]
pub struct ToolCreateTool {
    registry: Arc<DynamicToolRegistry>,
    provider: Arc<dyn Provider>,
}

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct ToolCreateSchema {
    /// Action to perform
    #[schema(enum_values = ["create", "list", "delete", "export", "import", "rate", "bundle_export", "bundle_import"])]
    action: String,
    /// Natural language description of the tool to create (for 'create' action)
    #[serde(default)]
    description: Option<String>,
    /// Tool name (for 'delete' and 'export' actions)
    #[serde(default)]
    name: Option<String>,
    /// Optional hint for which strategy type to use (for 'create' action)
    #[serde(default)]
    #[schema(enum_values = ["shell", "http", "llm", "composite", "codegen"])]
    strategy_hint: Option<String>,
    /// JSON tool definition to import (for 'import' action)
    #[serde(default)]
    json: Option<String>,
    /// Quality rating for 'rate' action
    #[serde(default)]
    #[schema(enum_values = ["good", "bad", "reset"])]
    rating: Option<String>,
}

impl ToolCreateTool {
    pub fn new(registry: Arc<DynamicToolRegistry>, provider: Arc<dyn Provider>) -> Self {
        Self { registry, provider }
    }
}

const TOOL_CREATE_PROMPT: &str = r#"You are a tool definition generator. Given a natural language description of a desired tool, output a JSON definition.

Output a JSON object with this exact structure:
{
  "name": "short_snake_case_name",
  "description": "One-line description for LLM tool selection",
  "strategy": {
    "type": "shell",
    "command_template": "echo {{input}}"
  }
}

Strategy types:
- "shell": Execute a shell command. Use {{input}} as the placeholder for the tool's input.
  Example: {"type": "shell", "command_template": "whisper {{input}} --output_format txt"}
- "http": Call an HTTP endpoint.
  Example: {"type": "http", "url": "https://api.example.com/v1", "method": "POST", "headers": {}}
- "llm": Delegate to an LLM with a specialized system prompt.
  Example: {"type": "llm", "system_prompt": "You are an expert code reviewer. Review the following code."}

Rules:
- Choose the simplest strategy that accomplishes the task
- For CLI tools, prefer "shell" strategy
- For API integrations, prefer "http" strategy
- For reasoning/analysis tasks, prefer "llm" strategy
- The name must be snake_case with only alphanumeric characters and underscores
- Output ONLY the JSON object, no markdown fences or explanation"#;

const CODEGEN_PROMPT: &str = r#"You are a Rust WASM tool generator. Given a description, output a complete Rust source file that compiles to a WASM plugin.

Use this exact template:

```rust
use agentzero_plugin_sdk::prelude::*;

declare_tool!("tool_name", handler);

fn handler(input: ToolInput) -> ToolOutput {
    // Parse input
    let req: serde_json::Value = match serde_json::from_str(&input.input) {
        Ok(v) => v,
        Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
    };

    // Your logic here

    ToolOutput::success("result".to_string())
}
```

Available types from `agentzero_plugin_sdk::prelude::*`:
- `ToolInput` — has `.input: String` (JSON from LLM) and `.workspace_root: String`
- `ToolOutput::success(msg: String)` — successful result
- `ToolOutput::error(msg: String)` — error result

Available crates (add a `// deps: name1, name2` comment on the first line if needed):
- `serde_json` (always available)
- `regex`, `chrono`, `url`, `base64`, `sha2`, `hex`, `rand`, `csv`, `serde`

Rules:
- Output ONLY the Rust source code, no markdown fences
- The tool_name in declare_tool! must be snake_case
- The handler function must be synchronous (no async)
- Keep it simple — one function, minimal error handling
- On the first line, add `// deps: crate1, crate2` if you need extra crates beyond serde_json"#;

#[async_trait]
impl Tool for ToolCreateTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(ToolCreateSchema::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        // Only root agents can create tools.
        if ctx.depth > 0 {
            return Err(anyhow::anyhow!(
                "tool_create is only available to root agents (depth=0)"
            ));
        }

        let parsed: serde_json::Value =
            serde_json::from_str(input).map_err(|e| anyhow::anyhow!("invalid input JSON: {e}"))?;

        let action = parsed["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing 'action' field"))?;

        match action {
            "create" => self.action_create(&parsed).await,
            "list" => self.action_list().await,
            "delete" => self.action_delete(&parsed).await,
            "export" => self.action_export(&parsed).await,
            "import" => self.action_import(&parsed).await,
            "rate" => self.action_rate(&parsed).await,
            "bundle_export" => self.action_bundle_export(&parsed).await,
            "bundle_import" => self.action_bundle_import(&parsed).await,
            other => Err(anyhow::anyhow!(
                "unknown action '{other}'; expected create, list, delete, export, import, rate, bundle_export, or bundle_import"
            )),
        }
    }
}

/// Create a dynamic tool from a natural language description using the LLM.
///
/// Returns the name of the created tool.
pub async fn create_tool_from_nl(
    registry: &DynamicToolRegistry,
    provider: &dyn Provider,
    description: &str,
    strategy_hint: Option<&str>,
) -> anyhow::Result<String> {
    if strategy_hint == Some("codegen") {
        return create_codegen_tool(registry, provider, description).await;
    }

    let hint = strategy_hint.unwrap_or("");
    let prompt = if hint.is_empty() {
        format!("{TOOL_CREATE_PROMPT}\n\nTool description: {description}")
    } else {
        format!(
            "{TOOL_CREATE_PROMPT}\n\nPreferred strategy type: {hint}\n\nTool description: {description}"
        )
    };

    let result = provider.complete(&prompt).await?;
    let response = result.output_text.trim();

    let partial: serde_json::Value = parse_json_from_response(response)?;

    let name = partial["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("LLM response missing 'name' field"))?
        .to_string();

    let tool_description = partial["description"]
        .as_str()
        .unwrap_or(description)
        .to_string();

    let strategy: DynamicToolStrategy = serde_json::from_value(partial["strategy"].clone())
        .map_err(|e| anyhow::anyhow!("failed to parse strategy from LLM response: {e}"))?;

    let def = DynamicToolDef {
        name: name.clone(),
        description: tool_description,
        strategy,
        input_schema: partial.get("input_schema").cloned(),
        created_at: now_secs(),
        total_invocations: 0,
        total_successes: 0,
        total_failures: 0,
        last_error: None,
        generation: 0,
        parent_name: None,
        user_rated: false,
    };

    registry.register(def).await?;
    Ok(name)
}

/// Maximum compilation retry attempts when LLM-generated code fails to compile.
const MAX_CODEGEN_RETRIES: usize = 3;

/// Create a codegen (compiled WASM) tool from a natural language description.
async fn create_codegen_tool(
    registry: &DynamicToolRegistry,
    provider: &dyn Provider,
    description: &str,
) -> anyhow::Result<String> {
    use crate::tools::codegen::{extract_deps_from_source, CodegenCompiler};

    let prompt = format!("{CODEGEN_PROMPT}\n\nTool description: {description}");

    let result = provider.complete(&prompt).await?;
    let mut source = extract_rust_source(&result.output_text);

    // Extract tool name from declare_tool!("name", ...)
    let name = extract_tool_name_from_source(&source).ok_or_else(|| {
        anyhow::anyhow!("could not find declare_tool!(\"name\", ...) in LLM response")
    })?;

    // Find the data dir from the registry (use temp for now, real path from context)
    let data_dir = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".agentzero");
    let compiler = CodegenCompiler::new(&data_dir);

    // Check toolchain first
    compiler.check_toolchain().await?;

    // Compile with retry loop
    let mut last_error = String::new();
    for attempt in 0..MAX_CODEGEN_RETRIES {
        let extra_deps = extract_deps_from_source(&source);
        match compiler.build_tool(&name, &source, &extra_deps).await {
            Ok((wasm_path, wasm_sha256, source_hash)) => {
                let def = DynamicToolDef {
                    name: name.clone(),
                    description: description.to_string(),
                    strategy: DynamicToolStrategy::Codegen {
                        source: source.clone(),
                        wasm_path: Some(wasm_path.to_string_lossy().to_string()),
                        wasm_sha256: Some(wasm_sha256),
                        source_hash: Some(source_hash),
                        compile_error: None,
                    },
                    input_schema: None,
                    created_at: now_secs(),
                    total_invocations: 0,
                    total_successes: 0,
                    total_failures: 0,
                    last_error: None,
                    generation: 0,
                    parent_name: None,
                    user_rated: false,
                };

                registry.register(def).await?;
                tracing::info!(tool = %name, attempt = attempt + 1, "codegen tool compiled and registered");
                return Ok(name);
            }
            Err(e) => {
                last_error = e.to_string();
                tracing::warn!(
                    tool = %name,
                    attempt = attempt + 1,
                    error = %last_error,
                    "codegen compilation failed, asking LLM to fix"
                );

                if attempt + 1 < MAX_CODEGEN_RETRIES {
                    // Feed the error back to the LLM for a fix
                    let fix_prompt = format!(
                        "{CODEGEN_PROMPT}\n\n\
                        Tool description: {description}\n\n\
                        The previous source code failed to compile. Fix the error.\n\n\
                        Previous source:\n```rust\n{source}\n```\n\n\
                        Compilation error:\n{last_error}\n\n\
                        Output ONLY the corrected Rust source code."
                    );
                    let fix_result = provider.complete(&fix_prompt).await?;
                    source = extract_rust_source(&fix_result.output_text);
                }
            }
        }
    }

    // All retries exhausted — register with compile error so it can be retried later
    let def = DynamicToolDef {
        name: name.clone(),
        description: description.to_string(),
        strategy: DynamicToolStrategy::Codegen {
            source,
            wasm_path: None,
            wasm_sha256: None,
            source_hash: None,
            compile_error: Some(last_error.clone()),
        },
        input_schema: None,
        created_at: now_secs(),
        total_invocations: 0,
        total_successes: 0,
        total_failures: 0,
        last_error: Some(format!("compilation failed: {last_error}")),
        generation: 0,
        parent_name: None,
        user_rated: false,
    };

    registry.register(def).await?;
    Err(anyhow::anyhow!(
        "codegen tool '{name}' failed to compile after {MAX_CODEGEN_RETRIES} attempts: {last_error}"
    ))
}

/// Extract Rust source from an LLM response (strip markdown fences if present).
fn extract_rust_source(response: &str) -> String {
    let trimmed = response.trim();

    // Try ```rust ... ``` block
    if let Some(start) = trimmed.find("```rust") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }

    // Try ``` ... ``` block
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        // Skip language tag on same line
        let after = if let Some(nl) = after.find('\n') {
            &after[nl + 1..]
        } else {
            after
        };
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }

    // No fences — use as-is
    trimmed.to_string()
}

/// Extract the tool name from a `declare_tool!("name", handler)` invocation.
fn extract_tool_name_from_source(source: &str) -> Option<String> {
    // Look for: declare_tool!("some_name"
    let marker = "declare_tool!(\"";
    let start = source.find(marker)? + marker.len();
    let rest = &source[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

impl ToolCreateTool {
    async fn action_create(&self, input: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let description = input["description"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing 'description' field for create action"))?;

        let strategy_hint = input["strategy_hint"].as_str();

        let name = create_tool_from_nl(
            &self.registry,
            self.provider.as_ref(),
            description,
            strategy_hint,
        )
        .await?;

        Ok(ToolResult {
            output: format!(
                "Dynamic tool '{name}' created and registered. Available immediately and persists across sessions.",
            ),
        })
    }

    async fn action_list(&self) -> anyhow::Result<ToolResult> {
        let defs = self.registry.list().await;
        if defs.is_empty() {
            return Ok(ToolResult {
                output: "No dynamic tools registered.".to_string(),
            });
        }

        let mut lines = Vec::with_capacity(defs.len());
        for def in &defs {
            let strategy_type = match &def.strategy {
                DynamicToolStrategy::Shell { .. } => "shell",
                DynamicToolStrategy::Http { .. } => "http",
                DynamicToolStrategy::Llm { .. } => "llm",
                DynamicToolStrategy::Composite { .. } => "composite",
                DynamicToolStrategy::Codegen { .. } => "codegen",
            };
            lines.push(format!(
                "- {} [{}]: {}",
                def.name, strategy_type, def.description
            ));
        }

        Ok(ToolResult {
            output: format!("{} dynamic tool(s):\n{}", defs.len(), lines.join("\n")),
        })
    }

    async fn action_delete(&self, input: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let name = input["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing 'name' field for delete action"))?;

        let removed = self.registry.remove(name).await?;
        if removed {
            Ok(ToolResult {
                output: format!("Dynamic tool '{name}' deleted."),
            })
        } else {
            Ok(ToolResult {
                output: format!("No dynamic tool named '{name}' found."),
            })
        }
    }

    async fn action_export(&self, input: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let name = input["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing 'name' field for export action"))?;

        match self.registry.export_tool(name).await? {
            Some(json) => Ok(ToolResult { output: json }),
            None => Ok(ToolResult {
                output: format!("No dynamic tool named '{name}' found."),
            }),
        }
    }

    async fn action_import(&self, input: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let json = input["json"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing 'json' field for import action"))?;

        let names = self.registry.import_tools(json).await?;
        Ok(ToolResult {
            output: format!("Imported {} tool(s): {}", names.len(), names.join(", ")),
        })
    }

    async fn action_rate(&self, input: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let name = input["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing 'name' field for rate action"))?;
        let rating = input["rating"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing 'rating' field (expected good/bad/reset)"))?;

        self.registry.apply_user_rating(name, rating).await?;

        let msg = match rating {
            "good" => format!(
                "Rated '{name}' as good — quality counters boosted, tool is now user-endorsed."
            ),
            "bad" => format!(
                "Rated '{name}' as bad — quality counters penalized, tool is now user-endorsed."
            ),
            "reset" => format!("Reset quality counters for '{name}'."),
            _ => format!("Applied rating '{rating}' to '{name}'."),
        };
        Ok(ToolResult { output: msg })
    }

    async fn action_bundle_export(&self, input: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let name = input["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing 'name' field for bundle_export action"))?;

        let bundle = self
            .registry
            .export_bundle(name, None)
            .await?
            .ok_or_else(|| anyhow::anyhow!("tool not found: {name}"))?;

        let json = serde_json::to_string_pretty(&bundle)
            .map_err(|e| anyhow::anyhow!("failed to serialize bundle: {e}"))?;

        Ok(ToolResult {
            output: format!("Exported bundle for '{name}':\n{json}"),
        })
    }

    async fn action_bundle_import(&self, input: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let json = input["json"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing 'json' field for bundle_import action"))?;

        let bundle: crate::tools::dynamic_tool::ToolBundle = serde_json::from_str(json)
            .map_err(|e| anyhow::anyhow!("failed to parse tool bundle: {e}"))?;

        let name = self.registry.import_bundle(bundle, None).await?;

        Ok(ToolResult {
            output: format!("Imported tool bundle '{name}' (quality counters reset to zero)."),
        })
    }
}

/// Parse JSON from an LLM response (handles markdown fences, leading text).
fn parse_json_from_response(response: &str) -> anyhow::Result<serde_json::Value> {
    let trimmed = response.trim();

    // Try ```json ... ``` block.
    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            if let Ok(v) = serde_json::from_str(after[..end].trim()) {
                return Ok(v);
            }
        }
    }

    // Try ``` ... ``` block.
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        if let Some(end) = after.find("```") {
            if let Ok(v) = serde_json::from_str(after[..end].trim()) {
                return Ok(v);
            }
        }
    }

    // Try { ... } directly.
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if let Ok(v) = serde_json::from_str(&trimmed[start..=end]) {
                return Ok(v);
            }
        }
    }

    // Last resort: try the whole thing.
    serde_json::from_str(trimmed)
        .map_err(|e| anyhow::anyhow!("failed to parse tool definition from LLM response: {e}"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ChatResult;

    struct MockCreateProvider {
        response: String,
    }

    #[async_trait]
    impl Provider for MockCreateProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            Ok(ChatResult {
                output_text: self.response.clone(),
                tool_calls: vec![],
                stop_reason: None,
                input_tokens: 0,
                output_tokens: 0,
            })
        }
    }

    fn test_data_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "agentzero-tool-create-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[tokio::test]
    async fn create_tool_from_nl_description() {
        let dir = test_data_dir();
        let registry = Arc::new(DynamicToolRegistry::open(&dir).expect("open"));
        let provider = Arc::new(MockCreateProvider {
            response: r#"{
                "name": "whisper_transcribe",
                "description": "Transcribe audio/video using Whisper",
                "strategy": {
                    "type": "shell",
                    "command_template": "whisper {{input}} --output_format txt"
                }
            }"#
            .to_string(),
        });

        let tool = ToolCreateTool::new(Arc::clone(&registry), provider);
        let ctx = ToolContext::new("/tmp".to_string());

        let input = serde_json::json!({
            "action": "create",
            "description": "A tool that transcribes audio files using Whisper CLI"
        });

        let result = tool
            .execute(&input.to_string(), &ctx)
            .await
            .expect("create should succeed");

        assert!(result.output.contains("whisper_transcribe"));
        assert!(result.output.contains("persists across sessions"));

        // Tool should be in registry.
        let all = registry.list().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "whisper_transcribe");
    }

    #[tokio::test]
    async fn list_tools_empty() {
        let dir = test_data_dir();
        let registry = Arc::new(DynamicToolRegistry::open(&dir).expect("open"));
        let provider = Arc::new(MockCreateProvider {
            response: "{}".to_string(),
        });

        let tool = ToolCreateTool::new(registry, provider);
        let ctx = ToolContext::new("/tmp".to_string());

        let result = tool
            .execute(r#"{"action":"list"}"#, &ctx)
            .await
            .expect("list");
        assert!(result.output.contains("No dynamic tools"));
    }

    #[tokio::test]
    async fn delete_tool() {
        let dir = test_data_dir();
        let registry = Arc::new(DynamicToolRegistry::open(&dir).expect("open"));

        // Pre-register a tool.
        registry
            .register(DynamicToolDef {
                name: "to_delete".to_string(),
                description: "Test".to_string(),
                strategy: DynamicToolStrategy::Shell {
                    command_template: "echo x".to_string(),
                },
                input_schema: None,
                created_at: now_secs(),
                total_invocations: 0,
                total_successes: 0,
                total_failures: 0,
                last_error: None,
                generation: 0,
                parent_name: None,
                user_rated: false,
            })
            .await
            .expect("register");

        let provider = Arc::new(MockCreateProvider {
            response: "{}".to_string(),
        });
        let tool = ToolCreateTool::new(registry, provider);
        let ctx = ToolContext::new("/tmp".to_string());

        let result = tool
            .execute(r#"{"action":"delete","name":"to_delete"}"#, &ctx)
            .await
            .expect("delete");
        assert!(result.output.contains("deleted"));
    }

    #[tokio::test]
    async fn depth_restriction() {
        let dir = test_data_dir();
        let registry = Arc::new(DynamicToolRegistry::open(&dir).expect("open"));
        let provider = Arc::new(MockCreateProvider {
            response: "{}".to_string(),
        });

        let tool = ToolCreateTool::new(registry, provider);
        let mut ctx = ToolContext::new("/tmp".to_string());
        ctx.depth = 1; // Sub-agent depth.

        let err = tool.execute(r#"{"action":"list"}"#, &ctx).await;
        assert!(err.is_err(), "should reject sub-agent calls");
    }

    #[test]
    fn parse_json_from_various_formats() {
        let clean = r#"{"name":"test","strategy":{"type":"shell","command_template":"echo"}}"#;
        assert!(parse_json_from_response(clean).is_ok());

        let fenced = "```json\n{\"name\":\"test\"}\n```";
        assert!(parse_json_from_response(fenced).is_ok());

        let with_text = "Here's the tool:\n{\"name\":\"test\"}";
        assert!(parse_json_from_response(with_text).is_ok());
    }

    #[test]
    fn extract_rust_source_from_fenced() {
        let fenced = "Here's the code:\n```rust\nuse foo;\nfn bar() {}\n```\nDone.";
        assert_eq!(extract_rust_source(fenced), "use foo;\nfn bar() {}");
    }

    #[test]
    fn extract_rust_source_bare() {
        let bare = "use agentzero_plugin_sdk::prelude::*;\ndeclare_tool!(\"test\", h);";
        assert_eq!(extract_rust_source(bare), bare);
    }

    #[test]
    fn extract_tool_name_from_declare_tool() {
        let source = r#"
use agentzero_plugin_sdk::prelude::*;
declare_tool!("reverse_string", handler);
fn handler(input: ToolInput) -> ToolOutput { todo!() }
"#;
        assert_eq!(
            extract_tool_name_from_source(source),
            Some("reverse_string".to_string())
        );
    }

    #[test]
    fn extract_tool_name_missing() {
        assert_eq!(extract_tool_name_from_source("fn main() {}"), None);
    }

    #[test]
    fn extract_deps_from_comment() {
        use crate::tools::codegen::extract_deps_from_source;

        let source = "// deps: regex, chrono\nuse agentzero_plugin_sdk::prelude::*;";
        let deps = extract_deps_from_source(source);
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].0, "regex");
        assert_eq!(deps[1].0, "chrono");
    }

    #[test]
    fn extract_deps_none() {
        use crate::tools::codegen::extract_deps_from_source;

        let source = "use agentzero_plugin_sdk::prelude::*;";
        assert!(extract_deps_from_source(source).is_empty());
    }

    #[test]
    fn extract_deps_rejects_unlisted() {
        use crate::tools::codegen::extract_deps_from_source;

        let source = "// deps: regex, tokio, chrono";
        let deps = extract_deps_from_source(source);
        // tokio is not in the allowlist
        assert_eq!(deps.len(), 2);
        assert!(deps.iter().all(|(n, _)| *n != "tokio"));
    }
}
