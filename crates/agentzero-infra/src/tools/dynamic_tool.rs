//! Dynamic tools — runtime-created tools that persist across sessions.
//!
//! A [`DynamicTool`] wraps an execution strategy (LLM, shell, HTTP, or
//! composite) and implements the [`Tool`] trait. Tools are stored in
//! `.agentzero/dynamic-tools.json` (encrypted) via [`DynamicToolRegistry`]
//! and loaded automatically on startup.

use agentzero_core::{Tool, ToolContext, ToolResult, ToolSource};
use agentzero_storage::EncryptedJsonStore;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

// ── Types ────────────────────────────────────────────────────────────────────

/// Persistent definition of a dynamic tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicToolDef {
    /// Unique tool identifier (e.g. `"whisper_transcribe"`).
    pub name: String,
    /// Human-readable description for LLM tool selection.
    pub description: String,
    /// How this tool executes.
    pub strategy: DynamicToolStrategy,
    /// Optional JSON Schema for structured input.
    #[serde(default)]
    pub input_schema: Option<serde_json::Value>,
    /// Unix timestamp when this tool was created.
    pub created_at: u64,
    // ── Quality tracking ─────────────────────────────────────────────────
    #[serde(default)]
    pub total_invocations: u32,
    #[serde(default)]
    pub total_successes: u32,
    #[serde(default)]
    pub total_failures: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Evolution generation (0 = original, incremented on each auto-fix/improve).
    #[serde(default)]
    pub generation: u32,
    /// Name of the tool this was evolved from (lineage tracking).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_name: Option<String>,
    /// Whether a user has explicitly rated this tool (prevents auto-retirement).
    #[serde(default)]
    pub user_rated: bool,
}

impl DynamicToolDef {
    /// Success rate as a fraction (0.0..=1.0). Returns 0.5 when no invocations.
    pub fn success_rate(&self) -> f64 {
        if self.total_invocations == 0 {
            return 0.5;
        }
        self.total_successes as f64 / self.total_invocations as f64
    }
}

/// Execution strategy for a dynamic tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DynamicToolStrategy {
    /// Delegate to an LLM with a specialized system prompt.
    Llm { system_prompt: String },
    /// Execute a shell command template. `{{input}}` is replaced with the
    /// tool input at execution time.
    Shell { command_template: String },
    /// Call an HTTP endpoint.
    Http {
        url: String,
        method: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
    /// Chain existing tools sequentially: each step's output becomes the
    /// next step's input.
    Composite { steps: Vec<CompositeStep> },
    /// LLM-generated Rust source compiled to WASM and loaded via the plugin
    /// runtime. The most capable strategy — runs native-speed sandboxed code.
    Codegen {
        /// Rust source code (using `declare_tool!` from plugin SDK).
        source: String,
        /// Path to the compiled `.wasm` file (relative to `.agentzero/codegen/`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wasm_path: Option<String>,
        /// SHA-256 hex digest of the `.wasm` file for integrity verification.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wasm_sha256: Option<String>,
        /// SHA-256 of the source code — used for rebuild avoidance.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_hash: Option<String>,
        /// Last compilation error (cleared on success, used for retry/evolution).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        compile_error: Option<String>,
    },
}

/// A step in a composite tool chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositeStep {
    /// Tool name to invoke.
    pub tool_name: String,
    /// Optional input override (if absent, uses previous step's output).
    #[serde(default)]
    pub input_override: Option<String>,
}

// ── DynamicTool (Tool trait impl) ────────────────────────────────────────────

/// Callback type for resolving tools by name during composite execution.
pub type ToolResolver = Arc<dyn Fn(&str) -> Option<Arc<dyn Tool>> + Send + Sync>;

/// A runtime-created tool wrapping a [`DynamicToolStrategy`].
pub struct DynamicTool {
    /// Leaked for `&'static str` lifetime requirement on `Tool::name()`.
    name: &'static str,
    /// Leaked for `&'static str` lifetime requirement on `Tool::description()`.
    description: &'static str,
    /// The execution strategy.
    strategy: DynamicToolStrategy,
    /// Optional JSON Schema.
    schema: Option<serde_json::Value>,
    /// Optional tool resolver for real composite execution.
    /// When set, Composite tools actually invoke sub-tools in sequence.
    tool_resolver: Option<ToolResolver>,
}

impl DynamicTool {
    /// Create a `DynamicTool` from a persistent definition.
    ///
    /// Uses `Box::leak` for name/description to satisfy the `Tool` trait's
    /// `&'static str` lifetime requirement (same pattern as MCP tools).
    pub fn from_def(def: &DynamicToolDef) -> Self {
        Self {
            name: Box::leak(def.name.clone().into_boxed_str()),
            description: Box::leak(def.description.clone().into_boxed_str()),
            strategy: def.strategy.clone(),
            schema: def.input_schema.clone(),
            tool_resolver: None,
        }
    }

    /// Create a `DynamicTool` with a tool resolver for real composite execution.
    pub fn from_def_with_resolver(def: &DynamicToolDef, resolver: ToolResolver) -> Self {
        Self {
            name: Box::leak(def.name.clone().into_boxed_str()),
            description: Box::leak(def.description.clone().into_boxed_str()),
            strategy: def.strategy.clone(),
            schema: def.input_schema.clone(),
            tool_resolver: Some(resolver),
        }
    }
}

#[async_trait]
impl Tool for DynamicTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        self.schema.clone()
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        match &self.strategy {
            DynamicToolStrategy::Shell { command_template } => {
                let cmd = command_template.replace("{{input}}", input);
                let output = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .output()
                    .await
                    .map_err(|e| anyhow::anyhow!("shell execution failed: {e}"))?;

                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                if output.status.success() {
                    Ok(ToolResult {
                        output: stdout.to_string(),
                    })
                } else {
                    Err(anyhow::anyhow!(
                        "command exited with {}: {}",
                        output.status,
                        stderr
                    ))
                }
            }

            DynamicToolStrategy::Http {
                url,
                method,
                headers,
            } => {
                let client = reqwest::Client::new();
                let mut req = match method.to_uppercase().as_str() {
                    "POST" => client.post(url),
                    "PUT" => client.put(url),
                    "DELETE" => client.delete(url),
                    "PATCH" => client.patch(url),
                    _ => client.get(url),
                };
                for (k, v) in headers {
                    req = req.header(k.as_str(), v.as_str());
                }
                req = req.body(input.to_string());

                let resp = req
                    .send()
                    .await
                    .map_err(|e| anyhow::anyhow!("HTTP request failed: {e}"))?;
                let text = resp
                    .text()
                    .await
                    .map_err(|e| anyhow::anyhow!("failed to read HTTP response: {e}"))?;
                Ok(ToolResult { output: text })
            }

            DynamicToolStrategy::Llm { system_prompt } => {
                // LLM strategy returns the system prompt + input as guidance.
                // Actual LLM execution is handled by the agent loop — the tool
                // output instructs the agent what to do with the input.
                Ok(ToolResult {
                    output: format!(
                        "[Dynamic LLM tool — system prompt below]\n\n{system_prompt}\n\n[Input]\n{input}"
                    ),
                })
            }

            DynamicToolStrategy::Codegen {
                wasm_path,
                compile_error,
                ..
            } => {
                if let Some(err) = compile_error {
                    return Err(anyhow::anyhow!(
                        "codegen tool '{}' failed to compile: {err}",
                        self.name
                    ));
                }
                let wasm = wasm_path.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "codegen tool '{}' has no compiled WASM (compilation may have failed)",
                        self.name
                    )
                })?;

                // Execute via the WASM plugin runtime.
                crate::tools::codegen::execute_codegen_tool(wasm, input, ctx).await
            }

            DynamicToolStrategy::Composite { steps } => {
                let mut current_input = input.to_string();

                // If we have a tool resolver, actually execute the pipeline.
                if let Some(ref resolver) = self.tool_resolver {
                    for step in steps {
                        let step_input = step
                            .input_override
                            .as_deref()
                            .unwrap_or(&current_input)
                            .to_string();
                        let tool = resolver(&step.tool_name).ok_or_else(|| {
                            anyhow::anyhow!("composite step tool '{}' not found", step.tool_name)
                        })?;
                        let result = tool.execute(&step_input, ctx).await?;
                        current_input = result.output;
                    }
                    return Ok(ToolResult {
                        output: current_input,
                    });
                }

                // Fallback: describe the pipeline (no resolver available).
                for step in steps {
                    let step_input = step.input_override.as_deref().unwrap_or(&current_input);
                    current_input =
                        format!("[Step: {} with input: {}]", step.tool_name, step_input);
                }
                Ok(ToolResult {
                    output: current_input,
                })
            }
        }
    }
}

// ── DynamicToolRegistry ──────────────────────────────────────────────────────

/// Persistent store for dynamic tool definitions.
const DYNAMIC_TOOLS_FILE: &str = "dynamic-tools.json";

/// Wrapper type for serde — `EncryptedJsonStore` needs a single top-level type.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DynamicToolsData {
    tools: Vec<DynamicToolDef>,
}

/// Registry for runtime-created tools. Persists definitions to encrypted JSON
/// and implements [`ToolSource`] so the agent can pick up new tools mid-session.
///
/// # Capability Security
///
/// The authoritative gate for tool creation is `Tool { name: "tool_create" }` in
/// `CapabilitySet` (Sprint 86 Phase A). When `CapabilitySet` is non-empty,
/// `ToolSecurityPolicy::allows_tool("tool_create")` is checked before any call
/// to [`DynamicToolRegistry::register`] reaches this type.
///
/// The `enable_dynamic_tools` boolean in `ToolSecurityPolicy` is a coarser
/// kill-switch fallback that activates when `CapabilitySet::is_empty()` is true
/// (i.e., the operator has not yet opted in to `[[capabilities]]` config).
///
/// When Sprint 86 Phase A4 is fully wired through `build_runtime_execution`, a
/// `DynamicToolDef` will carry a `capability_set: Option<CapabilitySet>` field
/// so that evolved tools cannot exceed the permissions of their creator agent.
///
/// # Capability enforcement (Sprint 86)
///
/// Tool creation via this registry is gated by `enable_dynamic_tools` (boolean kill-switch,
/// Sprint 84B). Starting Sprint 86, the preferred gate is:
///
/// ```rust,ignore
/// policy.capability_set.allows_tool("tool_create")
/// ```
///
/// The boolean `enable_dynamic_tools` remains as a coarser fallback when `capability_set.is_empty()`.
///
/// Dynamically created tools should carry the creator agent's `CapabilitySet`, not the
/// server-wide policy. When `DynamicToolDef` gains a `capability_set` field (Phase 2), each
/// tool invocation will be bounded by its creator's permissions. Until then, the kill-switch
/// is the only enforcement mechanism.
pub struct DynamicToolRegistry {
    defs: Arc<RwLock<Vec<DynamicToolDef>>>,
    store: EncryptedJsonStore,
}

impl DynamicToolRegistry {
    /// Open or create the registry in the given data directory.
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        let store = EncryptedJsonStore::in_config_dir(data_dir, DYNAMIC_TOOLS_FILE)?;
        let data: DynamicToolsData = store.load_or_default()?;
        Ok(Self {
            defs: Arc::new(RwLock::new(data.tools)),
            store,
        })
    }

    /// Register a new dynamic tool definition. Persists to disk and returns
    /// the tool ready for use.
    pub async fn register(&self, def: DynamicToolDef) -> anyhow::Result<Box<dyn Tool>> {
        let tool = DynamicTool::from_def(&def);

        let mut defs = self.defs.write().await;
        // Replace if tool with same name already exists.
        defs.retain(|d| d.name != def.name);
        defs.push(def);
        self.persist(&defs)?;
        drop(defs);

        Ok(Box::new(tool))
    }

    /// Load all persisted tools as `Box<dyn Tool>`.
    pub async fn load_all(&self) -> Vec<Box<dyn Tool>> {
        let defs = self.defs.read().await;
        defs.iter()
            .map(|d| Box::new(DynamicTool::from_def(d)) as Box<dyn Tool>)
            .collect()
    }

    /// List all registered tool definitions.
    pub async fn list(&self) -> Vec<DynamicToolDef> {
        self.defs.read().await.clone()
    }

    /// Remove a dynamic tool by name. Returns `true` if found.
    pub async fn remove(&self, name: &str) -> anyhow::Result<bool> {
        let mut defs = self.defs.write().await;
        let len_before = defs.len();
        defs.retain(|d| d.name != name);
        let removed = defs.len() < len_before;
        if removed {
            self.persist(&defs)?;
        }
        Ok(removed)
    }

    /// Export a single tool definition as shareable JSON.
    pub async fn export_tool(&self, name: &str) -> anyhow::Result<Option<String>> {
        let defs = self.defs.read().await;
        let def = defs.iter().find(|d| d.name == name);
        match def {
            Some(d) => {
                let json = serde_json::to_string_pretty(d)
                    .map_err(|e| anyhow::anyhow!("failed to serialize tool: {e}"))?;
                Ok(Some(json))
            }
            None => Ok(None),
        }
    }

    /// Export all tool definitions as a shareable JSON array.
    pub async fn export_all(&self) -> anyhow::Result<String> {
        let defs = self.defs.read().await;
        serde_json::to_string_pretty(&*defs)
            .map_err(|e| anyhow::anyhow!("failed to serialize tools: {e}"))
    }

    /// Import tool definitions from a JSON string (single def or array).
    pub async fn import_tools(&self, json: &str) -> anyhow::Result<Vec<String>> {
        // Try array first, then single object.
        let imported: Vec<DynamicToolDef> =
            if let Ok(arr) = serde_json::from_str::<Vec<DynamicToolDef>>(json) {
                arr
            } else {
                let single: DynamicToolDef = serde_json::from_str(json)
                    .map_err(|e| anyhow::anyhow!("failed to parse tool definition: {e}"))?;
                vec![single]
            };

        let mut names = Vec::new();
        for def in imported {
            names.push(def.name.clone());
            self.register(def).await?;
        }
        Ok(names)
    }

    /// Record a tool execution outcome, updating quality counters and persisting.
    pub async fn record_outcome(
        &self,
        name: &str,
        success: bool,
        error: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut defs = self.defs.write().await;
        if let Some(def) = defs.iter_mut().find(|d| d.name == name) {
            def.total_invocations += 1;
            if success {
                def.total_successes += 1;
            } else {
                def.total_failures += 1;
                def.last_error = error.map(|e| {
                    if e.len() > 500 {
                        format!("{}...", &e[..497])
                    } else {
                        e.to_string()
                    }
                });
            }
            self.persist(&defs)?;
        }
        Ok(())
    }

    /// Get a clone of a tool definition by name.
    pub async fn get_def(&self, name: &str) -> Option<DynamicToolDef> {
        self.defs
            .read()
            .await
            .iter()
            .find(|d| d.name == name)
            .cloned()
    }

    /// Check if a tool name belongs to a dynamic tool.
    pub async fn is_dynamic(&self, name: &str) -> bool {
        self.defs.read().await.iter().any(|d| d.name == name)
    }

    /// Apply a user quality rating (good/bad/reset).
    pub async fn apply_user_rating(&self, name: &str, rating: &str) -> anyhow::Result<()> {
        let mut defs = self.defs.write().await;
        if let Some(def) = defs.iter_mut().find(|d| d.name == name) {
            match rating {
                "good" => {
                    def.total_invocations += 3;
                    def.total_successes += 3;
                    def.user_rated = true;
                }
                "bad" => {
                    def.total_invocations += 3;
                    def.total_failures += 3;
                    def.user_rated = true;
                }
                "reset" => {
                    def.total_invocations = 0;
                    def.total_successes = 0;
                    def.total_failures = 0;
                    def.last_error = None;
                }
                _ => anyhow::bail!("unknown rating: {rating} (expected good/bad/reset)"),
            }
            self.persist(&defs)?;
            Ok(())
        } else {
            anyhow::bail!("tool not found: {name}")
        }
    }

    /// Export a tool as a shareable bundle with related recipes and lineage.
    pub async fn export_bundle(
        &self,
        name: &str,
        recipe_store: Option<&crate::tool_recipes::RecipeStore>,
    ) -> anyhow::Result<Option<ToolBundle>> {
        let defs = self.defs.read().await;
        let def = match defs.iter().find(|d| d.name == name) {
            Some(d) => d.clone(),
            None => return Ok(None),
        };

        // Walk lineage chain.
        let mut lineage = Vec::new();
        let mut current = def.parent_name.clone();
        while let Some(ref parent) = current {
            lineage.push(parent.clone());
            current = defs
                .iter()
                .find(|d| d.name == *parent)
                .and_then(|d| d.parent_name.clone());
        }

        // Collect related recipes.
        let related_recipes = recipe_store
            .map(|store| store.export_for_tools(&[name.to_string()]))
            .unwrap_or_default();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(Some(ToolBundle {
            version: 1,
            tool: def,
            related_recipes,
            lineage,
            exported_at: now,
        }))
    }

    /// Import a tool bundle, resetting quality counters (imported tools must re-prove themselves).
    pub async fn import_bundle(
        &self,
        bundle: ToolBundle,
        recipe_store: Option<&mut crate::tool_recipes::RecipeStore>,
    ) -> anyhow::Result<String> {
        let mut def = bundle.tool;
        // Reset quality counters — imported tools start fresh.
        def.total_invocations = 0;
        def.total_successes = 0;
        def.total_failures = 0;
        def.last_error = None;
        def.user_rated = false;

        let name = def.name.clone();
        self.register(def).await?;

        // Import related recipes.
        if let Some(store) = recipe_store {
            for recipe in bundle.related_recipes {
                let tools = recipe.tools_used.clone();
                if let Err(e) = store.record(&recipe.goal_summary, &tools, recipe.success) {
                    tracing::warn!(error = %e, "failed to import recipe from bundle");
                }
            }
        }

        Ok(name)
    }

    fn persist(&self, defs: &[DynamicToolDef]) -> anyhow::Result<()> {
        let data = DynamicToolsData {
            tools: defs.to_vec(),
        };
        self.store.save(&data)
    }
}

// ── ToolBundle ──────────────────────────────────────────────────────────────

/// A shareable bundle containing a tool, its related recipes, and lineage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolBundle {
    /// Bundle format version.
    pub version: u32,
    /// The tool definition.
    pub tool: DynamicToolDef,
    /// Recipes that reference this tool.
    #[serde(default)]
    pub related_recipes: Vec<crate::tool_recipes::ToolRecipe>,
    /// Parent chain: [parent_name, grandparent_name, ...].
    #[serde(default)]
    pub lineage: Vec<String>,
    /// Unix timestamp when this bundle was exported.
    pub exported_at: u64,
}

// ── ToolSource impl ──────────────────────────────────────────────────────────

/// Blocking impl — reads the current defs and creates tool instances.
impl ToolSource for DynamicToolRegistry {
    fn additional_tools(&self) -> Vec<Box<dyn Tool>> {
        // Use `try_read` to avoid blocking the async runtime.
        match self.defs.try_read() {
            Ok(defs) => defs
                .iter()
                .map(|d| Box::new(DynamicTool::from_def(d)) as Box<dyn Tool>)
                .collect(),
            Err(_) => vec![], // Lock contention — return empty (transient).
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_secs()
    }

    fn test_data_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "agentzero-dynamic-tools-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn shell_def(name: &str, cmd: &str) -> DynamicToolDef {
        DynamicToolDef {
            name: name.to_string(),
            description: format!("Test tool: {name}"),
            strategy: DynamicToolStrategy::Shell {
                command_template: cmd.to_string(),
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
        }
    }

    #[test]
    fn dynamic_tool_def_serde_roundtrip() {
        let def = shell_def("echo_tool", "echo {{input}}");
        let json = serde_json::to_string(&def).expect("serialize");
        let parsed: DynamicToolDef = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.name, "echo_tool");
        if let DynamicToolStrategy::Shell { command_template } = &parsed.strategy {
            assert_eq!(command_template, "echo {{input}}");
        } else {
            panic!("expected Shell strategy");
        }
    }

    #[test]
    fn dynamic_tool_def_serde_all_strategies() {
        let llm = DynamicToolDef {
            name: "llm_tool".to_string(),
            description: "LLM tool".to_string(),
            strategy: DynamicToolStrategy::Llm {
                system_prompt: "You are a helpful assistant".to_string(),
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
        let json = serde_json::to_string(&llm).expect("serialize");
        assert!(json.contains(r#""type":"llm""#));

        let http = DynamicToolDef {
            name: "http_tool".to_string(),
            description: "HTTP tool".to_string(),
            strategy: DynamicToolStrategy::Http {
                url: "https://api.example.com".to_string(),
                method: "POST".to_string(),
                headers: HashMap::from([("Authorization".to_string(), "Bearer xxx".to_string())]),
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
        let json = serde_json::to_string(&http).expect("serialize");
        assert!(json.contains(r#""type":"http""#));

        let composite = DynamicToolDef {
            name: "pipe_tool".to_string(),
            description: "Pipeline tool".to_string(),
            strategy: DynamicToolStrategy::Composite {
                steps: vec![
                    CompositeStep {
                        tool_name: "shell".to_string(),
                        input_override: None,
                    },
                    CompositeStep {
                        tool_name: "read_file".to_string(),
                        input_override: Some("output.txt".to_string()),
                    },
                ],
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
        let json = serde_json::to_string(&composite).expect("serialize");
        assert!(json.contains(r#""type":"composite""#));
    }

    #[test]
    fn dynamic_tool_from_def_has_correct_metadata() {
        let def = shell_def("my_tool", "ls");
        let tool = DynamicTool::from_def(&def);
        assert_eq!(tool.name(), "my_tool");
        assert_eq!(tool.description(), "Test tool: my_tool");
        assert!(tool.input_schema().is_none());
    }

    #[tokio::test]
    async fn shell_tool_executes_command() {
        let def = shell_def("echo_test", "echo hello-world");
        let tool = DynamicTool::from_def(&def);
        let ctx = ToolContext::new("/tmp".to_string());
        let result = tool.execute("", &ctx).await.expect("execute");
        assert_eq!(result.output.trim(), "hello-world");
    }

    #[tokio::test]
    async fn shell_tool_substitutes_input() {
        let def = shell_def("echo_input", "echo {{input}}");
        let tool = DynamicTool::from_def(&def);
        let ctx = ToolContext::new("/tmp".to_string());
        let result = tool.execute("greetings", &ctx).await.expect("execute");
        assert_eq!(result.output.trim(), "greetings");
    }

    #[tokio::test]
    async fn shell_tool_returns_error_on_failure() {
        let def = shell_def("fail_tool", "exit 1");
        let tool = DynamicTool::from_def(&def);
        let ctx = ToolContext::new("/tmp".to_string());
        let err = tool.execute("", &ctx).await;
        assert!(err.is_err(), "should fail on non-zero exit");
    }

    #[tokio::test]
    async fn llm_tool_returns_prompt_and_input() {
        let def = DynamicToolDef {
            name: "llm_test".to_string(),
            description: "LLM test".to_string(),
            strategy: DynamicToolStrategy::Llm {
                system_prompt: "Summarize this.".to_string(),
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
        let tool = DynamicTool::from_def(&def);
        let ctx = ToolContext::new("/tmp".to_string());
        let result = tool.execute("some text", &ctx).await.expect("execute");
        assert!(result.output.contains("Summarize this."));
        assert!(result.output.contains("some text"));
    }

    #[tokio::test]
    async fn registry_register_and_load() {
        let dir = test_data_dir();
        let registry = DynamicToolRegistry::open(&dir).expect("open");

        let def = shell_def("test_echo", "echo hi");
        let tool = registry.register(def).await.expect("register");
        assert_eq!(tool.name(), "test_echo");

        let all = registry.load_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name(), "test_echo");
    }

    #[tokio::test]
    async fn registry_persists_across_reopen() {
        let dir = test_data_dir();

        // First session: register a tool.
        {
            let registry = DynamicToolRegistry::open(&dir).expect("open");
            let def = shell_def("persistent_tool", "echo persist");
            registry.register(def).await.expect("register");
        }

        // Second session: tool should still be there.
        {
            let registry = DynamicToolRegistry::open(&dir).expect("reopen");
            let all = registry.load_all().await;
            assert_eq!(all.len(), 1);
            assert_eq!(all[0].name(), "persistent_tool");
        }
    }

    #[tokio::test]
    async fn registry_remove_tool() {
        let dir = test_data_dir();
        let registry = DynamicToolRegistry::open(&dir).expect("open");

        registry
            .register(shell_def("to_remove", "echo x"))
            .await
            .expect("register");

        let removed = registry.remove("to_remove").await.expect("remove");
        assert!(removed);

        let all = registry.load_all().await;
        assert!(all.is_empty());

        // Removing again returns false.
        let removed_again = registry.remove("to_remove").await.expect("remove");
        assert!(!removed_again);
    }

    #[tokio::test]
    async fn registry_replace_existing_tool() {
        let dir = test_data_dir();
        let registry = DynamicToolRegistry::open(&dir).expect("open");

        registry
            .register(shell_def("replaceable", "echo v1"))
            .await
            .expect("register v1");

        registry
            .register(shell_def("replaceable", "echo v2"))
            .await
            .expect("register v2");

        let all = registry.list().await;
        assert_eq!(all.len(), 1, "should replace, not duplicate");
        if let DynamicToolStrategy::Shell { command_template } = &all[0].strategy {
            assert_eq!(command_template, "echo v2");
        }
    }

    #[tokio::test]
    async fn tool_source_returns_additional_tools() {
        let dir = test_data_dir();
        let registry = DynamicToolRegistry::open(&dir).expect("open");

        registry
            .register(shell_def("src_tool", "echo source"))
            .await
            .expect("register");

        let tools = registry.additional_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "src_tool");
    }

    #[tokio::test]
    async fn export_single_tool() {
        let dir = test_data_dir();
        let registry = DynamicToolRegistry::open(&dir).expect("open");
        registry
            .register(shell_def("exportable", "echo export"))
            .await
            .expect("register");

        let json = registry
            .export_tool("exportable")
            .await
            .expect("export")
            .expect("should find tool");

        assert!(json.contains("exportable"));
        assert!(json.contains("echo export"));

        // Should parse back cleanly.
        let _: DynamicToolDef = serde_json::from_str(&json).expect("parse exported JSON");
    }

    #[tokio::test]
    async fn export_all_tools() {
        let dir = test_data_dir();
        let registry = DynamicToolRegistry::open(&dir).expect("open");
        registry
            .register(shell_def("tool_a", "echo a"))
            .await
            .expect("register a");
        registry
            .register(shell_def("tool_b", "echo b"))
            .await
            .expect("register b");

        let json = registry.export_all().await.expect("export all");
        let parsed: Vec<DynamicToolDef> = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed.len(), 2);
    }

    #[tokio::test]
    async fn import_single_tool() {
        let dir = test_data_dir();
        let registry = DynamicToolRegistry::open(&dir).expect("open");

        let json = serde_json::to_string(&shell_def("imported", "echo hi")).expect("serialize");
        let names = registry.import_tools(&json).await.expect("import");
        assert_eq!(names, vec!["imported"]);

        let all = registry.load_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name(), "imported");
    }

    #[tokio::test]
    async fn import_array_of_tools() {
        let dir = test_data_dir();
        let registry = DynamicToolRegistry::open(&dir).expect("open");

        let defs = vec![shell_def("imp_a", "echo a"), shell_def("imp_b", "echo b")];
        let json = serde_json::to_string(&defs).expect("serialize");
        let names = registry.import_tools(&json).await.expect("import");
        assert_eq!(names, vec!["imp_a", "imp_b"]);

        let all = registry.load_all().await;
        assert_eq!(all.len(), 2);
    }
}
