use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;

const MEMORY_FILE: &str = ".agentzero/memory.json";
const DEFAULT_NAMESPACE: &str = "default";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct MemoryStore {
    namespaces: HashMap<String, HashMap<String, String>>,
}

impl MemoryStore {
    async fn load(workspace_root: &str) -> anyhow::Result<Self> {
        let path = Path::new(workspace_root).join(MEMORY_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(&path)
            .await
            .context("failed to read memory store")?;
        serde_json::from_str(&data).context("failed to parse memory store")
    }

    async fn save(&self, workspace_root: &str) -> anyhow::Result<()> {
        let path = Path::new(workspace_root).join(MEMORY_FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("failed to create .agentzero directory")?;
        }
        let data = serde_json::to_string_pretty(self).context("failed to serialize memory")?;
        fs::write(&path, data)
            .await
            .context("failed to write memory store")
    }
}

// --- memory_store ---

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct MemoryStoreInput {
    /// The key to store
    key: String,
    /// The value to store
    value: String,
    /// Optional namespace for grouping
    #[serde(default)]
    namespace: Option<String>,
}

#[tool(
    name = "memory_store",
    description = "Store a key-value pair in persistent memory, optionally under a namespace."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct MemoryStoreTool;

#[async_trait]
impl Tool for MemoryStoreTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(MemoryStoreInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: MemoryStoreInput = serde_json::from_str(input)
            .context("memory_store expects JSON: {\"key\", \"value\", \"namespace\"?}")?;

        if req.key.trim().is_empty() {
            return Err(anyhow!("key must not be empty"));
        }

        let ns = req
            .namespace
            .as_deref()
            .unwrap_or(DEFAULT_NAMESPACE)
            .to_string();

        // Phase J — Sprint 90: enforce memory namespace scope from capability set.
        if !ctx.capability_set.is_empty() && !ctx.capability_set.allows_memory(&ns) {
            return Err(anyhow::anyhow!(
                "memory access denied: capability set does not grant access to namespace '{ns}'"
            ));
        }
        let mut store = MemoryStore::load(&ctx.workspace_root).await?;
        store
            .namespaces
            .entry(ns.clone())
            .or_default()
            .insert(req.key.clone(), req.value.clone());
        store.save(&ctx.workspace_root).await?;

        Ok(ToolResult {
            output: format!(
                "stored key={} namespace={} bytes={}",
                req.key,
                ns,
                req.value.len()
            ),
        })
    }
}

// --- memory_recall ---

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct MemoryRecallInput {
    /// Specific key to recall
    #[serde(default)]
    key: Option<String>,
    /// Namespace to search within
    #[serde(default)]
    namespace: Option<String>,
    /// Max entries to return (default: 50)
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

#[tool(
    name = "memory_recall",
    description = "Recall stored values from memory by key or list recent entries in a namespace."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct MemoryRecallTool;

#[async_trait]
impl Tool for MemoryRecallTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(MemoryRecallInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: MemoryRecallInput = serde_json::from_str(input)
            .context("memory_recall expects JSON: {\"key\"?, \"namespace\"?, \"limit\"?}")?;

        let ns = req.namespace.as_deref().unwrap_or(DEFAULT_NAMESPACE);

        // Phase J — Sprint 90: enforce memory namespace scope from capability set.
        if !ctx.capability_set.is_empty() && !ctx.capability_set.allows_memory(&ns) {
            return Err(anyhow::anyhow!(
                "memory access denied: capability set does not grant access to namespace '{ns}'"
            ));
        }
        let store = MemoryStore::load(&ctx.workspace_root).await?;

        let entries = match store.namespaces.get(ns) {
            Some(map) => map,
            None => {
                return Ok(ToolResult {
                    output: "no entries found".to_string(),
                });
            }
        };

        if let Some(ref key) = req.key {
            match entries.get(key.as_str()) {
                Some(value) => {
                    return Ok(ToolResult {
                        output: value.clone(),
                    });
                }
                None => {
                    return Ok(ToolResult {
                        output: format!("key not found: {key}"),
                    });
                }
            }
        }

        // List all keys in namespace.
        let limit = if req.limit == 0 { 50 } else { req.limit };
        let mut keys: Vec<&String> = entries.keys().collect();
        keys.sort();
        let results: Vec<String> = keys
            .iter()
            .take(limit)
            .map(|k| format!("{}={}", k, entries[k.as_str()]))
            .collect();

        if results.is_empty() {
            return Ok(ToolResult {
                output: "no entries found".to_string(),
            });
        }

        Ok(ToolResult {
            output: results.join("\n"),
        })
    }
}

// --- memory_forget ---

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct MemoryForgetInput {
    /// The key to forget
    key: String,
    /// Namespace the key belongs to
    #[serde(default)]
    namespace: Option<String>,
}

#[tool(
    name = "memory_forget",
    description = "Remove a key-value pair from memory."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct MemoryForgetTool;

#[async_trait]
impl Tool for MemoryForgetTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(MemoryForgetInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: MemoryForgetInput = serde_json::from_str(input)
            .context("memory_forget expects JSON: {\"key\", \"namespace\"?}")?;

        if req.key.trim().is_empty() {
            return Err(anyhow!("key must not be empty"));
        }

        let ns = req
            .namespace
            .as_deref()
            .unwrap_or(DEFAULT_NAMESPACE)
            .to_string();

        // Phase J — Sprint 90: enforce memory namespace scope from capability set.
        if !ctx.capability_set.is_empty() && !ctx.capability_set.allows_memory(&ns) {
            return Err(anyhow::anyhow!(
                "memory access denied: capability set does not grant access to namespace '{ns}'"
            ));
        }
        let mut store = MemoryStore::load(&ctx.workspace_root).await?;

        let removed = store
            .namespaces
            .get_mut(&ns)
            .and_then(|map| map.remove(&req.key))
            .is_some();

        if removed {
            store.save(&ctx.workspace_root).await?;
            Ok(ToolResult {
                output: format!("forgotten key={} namespace={}", req.key, ns),
            })
        } else {
            Ok(ToolResult {
                output: format!("key not found: {} in namespace={}", req.key, ns),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MemoryForgetTool, MemoryRecallTool, MemoryStoreTool};
    use agentzero_core::{Tool, ToolContext};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-memory-tools-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn memory_store_recall_roundtrip() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let store = MemoryStoreTool;
        store
            .execute(r#"{"key": "greeting", "value": "hello world"}"#, &ctx)
            .await
            .expect("store should succeed");

        let recall = MemoryRecallTool;
        let result = recall
            .execute(r#"{"key": "greeting"}"#, &ctx)
            .await
            .expect("recall should succeed");
        assert_eq!(result.output, "hello world");
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn memory_forget_removes_key() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        MemoryStoreTool
            .execute(r#"{"key": "temp", "value": "data"}"#, &ctx)
            .await
            .unwrap();

        let forget = MemoryForgetTool;
        let result = forget
            .execute(r#"{"key": "temp"}"#, &ctx)
            .await
            .expect("forget should succeed");
        assert!(result.output.contains("forgotten"));

        let recall = MemoryRecallTool;
        let result = recall
            .execute(r#"{"key": "temp"}"#, &ctx)
            .await
            .expect("recall should succeed");
        assert!(result.output.contains("key not found"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn memory_namespace_isolation() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        MemoryStoreTool
            .execute(r#"{"key": "x", "value": "default_val"}"#, &ctx)
            .await
            .unwrap();
        MemoryStoreTool
            .execute(
                r#"{"key": "x", "value": "custom_val", "namespace": "custom"}"#,
                &ctx,
            )
            .await
            .unwrap();

        let result = MemoryRecallTool
            .execute(r#"{"key": "x"}"#, &ctx)
            .await
            .unwrap();
        assert_eq!(result.output, "default_val");

        let result = MemoryRecallTool
            .execute(r#"{"key": "x", "namespace": "custom"}"#, &ctx)
            .await
            .unwrap();
        assert_eq!(result.output, "custom_val");
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn memory_store_rejects_empty_key_negative_path() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let err = MemoryStoreTool
            .execute(r#"{"key": "", "value": "data"}"#, &ctx)
            .await
            .expect_err("empty key should fail");
        assert!(err.to_string().contains("key must not be empty"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn memory_recall_lists_all_keys() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        MemoryStoreTool
            .execute(r#"{"key": "a", "value": "1"}"#, &ctx)
            .await
            .unwrap();
        MemoryStoreTool
            .execute(r#"{"key": "b", "value": "2"}"#, &ctx)
            .await
            .unwrap();

        let result = MemoryRecallTool
            .execute(r#"{}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("a=1"));
        assert!(result.output.contains("b=2"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn memory_store_denied_by_capability_set() {
        use agentzero_core::security::capability::{Capability, CapabilitySet};
        let dir = temp_dir();
        let mut ctx = ToolContext::new(dir.to_string_lossy().to_string());
        ctx.capability_set = CapabilitySet::new(
            vec![Capability::Tool { name: "memory_store".to_string() }],
            vec![],
        );
        // capability_set has no Memory grant → access denied
        let err = MemoryStoreTool
            .execute(r#"{"key": "x", "value": "v"}"#, &ctx)
            .await
            .expect_err("should be denied");
        assert!(err.to_string().contains("memory access denied"), "{err}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn memory_store_allowed_by_scoped_capability() {
        use agentzero_core::security::capability::{Capability, CapabilitySet};
        let dir = temp_dir();
        let mut ctx = ToolContext::new(dir.to_string_lossy().to_string());
        ctx.capability_set = CapabilitySet::new(
            vec![Capability::Memory { scope: Some("agent_a".to_string()) }],
            vec![],
        );
        // Allowed in "agent_a" namespace
        MemoryStoreTool
            .execute(r#"{"key": "k", "value": "v", "namespace": "agent_a"}"#, &ctx)
            .await
            .expect("scoped access should succeed");
        // Denied in "agent_b" namespace
        let err = MemoryStoreTool
            .execute(r#"{"key": "k", "value": "v", "namespace": "agent_b"}"#, &ctx)
            .await
            .expect_err("should deny wrong namespace");
        assert!(err.to_string().contains("memory access denied"), "{err}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn memory_store_empty_capability_set_allows_all() {
        // Empty capability set → backward-compatible unrestricted access.
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        MemoryStoreTool
            .execute(r#"{"key": "k", "value": "v", "namespace": "anything"}"#, &ctx)
            .await
            .expect("empty cap set should allow all namespaces");
        std::fs::remove_dir_all(dir).ok();
    }

}
