use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmIsolationPolicy {
    pub max_execution_ms: u64,
    pub max_module_bytes: u64,
    pub max_memory_mb: u32,
    pub allow_network: bool,
    pub allow_fs_write: bool,
}

impl Default for WasmIsolationPolicy {
    fn default() -> Self {
        Self {
            max_execution_ms: 30_000,
            max_module_bytes: 5 * 1024 * 1024,
            max_memory_mb: 256,
            allow_network: false,
            allow_fs_write: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmPluginContainer {
    pub id: String,
    pub module_path: PathBuf,
    pub entrypoint: String,
    pub max_execution_ms: u64,
    pub max_memory_mb: u32,
    pub allow_network: bool,
    pub allow_fs_write: bool,
}

impl WasmPluginContainer {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.id.trim().is_empty() {
            return Err(anyhow!("plugin id cannot be empty"));
        }
        if self.entrypoint.trim().is_empty() {
            return Err(anyhow!("plugin entrypoint cannot be empty"));
        }
        if self.max_execution_ms == 0 {
            return Err(anyhow!("max_execution_ms must be > 0"));
        }
        if self.max_memory_mb == 0 {
            return Err(anyhow!("max_memory_mb must be > 0"));
        }
        if self.module_path.extension().and_then(|e| e.to_str()) != Some("wasm") {
            return Err(anyhow!("plugin module must be a .wasm file"));
        }
        Ok(())
    }
}

pub struct WasmPluginRuntime;

impl WasmPluginRuntime {
    pub fn new() -> Self {
        Self
    }

    pub fn preflight(&self, container: &WasmPluginContainer) -> anyhow::Result<()> {
        self.preflight_with_policy(container, &WasmIsolationPolicy::default())
    }

    pub fn preflight_with_policy(
        &self,
        container: &WasmPluginContainer,
        policy: &WasmIsolationPolicy,
    ) -> anyhow::Result<()> {
        container.validate()?;
        if container.max_execution_ms > policy.max_execution_ms {
            return Err(anyhow!(
                "max_execution_ms exceeds policy limit ({} > {})",
                container.max_execution_ms,
                policy.max_execution_ms
            ));
        }
        if container.max_memory_mb > policy.max_memory_mb {
            return Err(anyhow!(
                "max_memory_mb exceeds policy limit ({} > {})",
                container.max_memory_mb,
                policy.max_memory_mb
            ));
        }
        if container.allow_network && !policy.allow_network {
            return Err(anyhow!(
                "network access is not permitted by isolation policy"
            ));
        }
        if container.allow_fs_write && !policy.allow_fs_write {
            return Err(anyhow!(
                "filesystem write is not permitted by isolation policy"
            ));
        }

        let path = Path::new(&container.module_path);
        if !path.exists() {
            return Err(anyhow!("plugin module does not exist: {}", path.display()));
        }
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("failed to read metadata for {}", path.display()))?;
        if metadata.len() > policy.max_module_bytes {
            return Err(anyhow!(
                "plugin module exceeds size policy ({} > {} bytes)",
                metadata.len(),
                policy.max_module_bytes
            ));
        }
        Ok(())
    }
}

impl Default for WasmPluginRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{WasmIsolationPolicy, WasmPluginContainer, WasmPluginRuntime};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn rejects_non_wasm_paths() {
        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: "plugin.txt".into(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };
        assert!(container.validate().is_err());
    }

    #[test]
    fn preflight_rejects_missing_file() {
        let runtime = WasmPluginRuntime::new();
        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: "missing_plugin.wasm".into(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };
        assert!(runtime.preflight(&container).is_err());
    }

    #[test]
    fn preflight_rejects_policy_violating_capabilities() {
        let runtime = WasmPluginRuntime::new();
        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: "missing_plugin.wasm".into(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1000,
            max_memory_mb: 64,
            allow_network: true,
            allow_fs_write: false,
        };
        let policy = WasmIsolationPolicy::default();
        let result = runtime.preflight_with_policy(&container, &policy);
        assert!(result.is_err());
        assert!(result
            .expect_err("policy violation should fail")
            .to_string()
            .contains("network access is not permitted"));
    }

    #[test]
    fn preflight_rejects_oversized_module() {
        let runtime = WasmPluginRuntime::new();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("oversized-{unique}.wasm"));
        fs::write(&path, vec![1_u8; 32]).expect("temp wasm file should be created");

        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: path.clone(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };
        let policy = WasmIsolationPolicy {
            max_module_bytes: 8,
            ..WasmIsolationPolicy::default()
        };

        let result = runtime.preflight_with_policy(&container, &policy);
        assert!(result.is_err());
        assert!(result
            .expect_err("oversized module should fail")
            .to_string()
            .contains("exceeds size policy"));

        fs::remove_file(path).expect("temp wasm file should be removed");
    }
}
