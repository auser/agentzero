//! Plugin manifest and registry for WASM-based AgentZero plugins.
//!
//! Plugins are discovered from `.agentzero/plugins/<name>/` directories.
//! Each plugin has a `PLUGIN.toml` manifest declaring name, version,
//! description, and available commands. The CLI dispatches plugin commands
//! via WASM `execute_with_input`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during plugin operations.
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin IO error: {0}")]
    IoError(String),
    #[error("plugin manifest parse error: {0}")]
    ParseError(String),
    #[error("plugin not found: {0}")]
    NotFound(String),
    #[error("missing PLUGIN.toml in source directory")]
    MissingManifest,
}

/// Full plugin manifest parsed from `PLUGIN.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    #[serde(default)]
    pub commands: Vec<PluginCommand>,
}

/// Plugin metadata section of the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default = "default_runtime")]
    pub runtime: String,
    pub wasm_path: Option<String>,
}

fn default_runtime() -> String {
    "wasm".to_string()
}

/// A command exposed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCommand {
    pub name: String,
    pub description: String,
}

/// Registry that discovers and manages installed plugins.
pub struct PluginRegistry {
    plugins_dir: PathBuf,
}

impl PluginRegistry {
    /// Create a new registry rooted at the given plugins directory.
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self { plugins_dir }
    }

    /// Scan the plugins directory for installed plugins.
    ///
    /// Returns a list of `(manifest, plugin_dir)` pairs for each valid plugin.
    pub fn list(&self) -> Vec<(PluginManifest, PathBuf)> {
        let mut plugins = Vec::new();

        let entries = match std::fs::read_dir(&self.plugins_dir) {
            Ok(e) => e,
            Err(_) => return plugins,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let manifest_path = path.join("PLUGIN.toml");
            if !manifest_path.exists() {
                continue;
            }

            if let Ok(manifest) = load_manifest(&manifest_path) {
                plugins.push((manifest, path));
            }
        }

        plugins.sort_by(|a, b| a.0.plugin.name.cmp(&b.0.plugin.name));
        plugins
    }

    /// Get a specific plugin by name.
    pub fn get(&self, name: &str) -> Option<(PluginManifest, PathBuf)> {
        let plugin_dir = self.plugins_dir.join(name);
        let manifest_path = plugin_dir.join("PLUGIN.toml");

        if !manifest_path.exists() {
            return None;
        }

        load_manifest(&manifest_path).ok().map(|m| (m, plugin_dir))
    }

    /// Find the WASM module bytes for a plugin.
    ///
    /// Checks the manifest's `wasm_path` first, then scans for any `.wasm`
    /// file in the plugin directory.
    pub fn find_wasm(&self, name: &str) -> Option<Vec<u8>> {
        let plugin_dir = self.plugins_dir.join(name);
        if !plugin_dir.exists() {
            return None;
        }

        let manifest_path = plugin_dir.join("PLUGIN.toml");
        if let Ok(manifest) = load_manifest(&manifest_path) {
            if let Some(ref wasm_path) = manifest.plugin.wasm_path {
                let full_path = plugin_dir.join(wasm_path);
                if full_path.exists() {
                    return std::fs::read(&full_path).ok();
                }
            }
        }

        // Fallback: scan for any .wasm file in the plugin dir
        find_wasm_in_dir(&plugin_dir)
    }

    /// Install a plugin from a source directory.
    ///
    /// Copies `PLUGIN.toml` and any `.wasm` files from `source` to
    /// `.agentzero/plugins/<name>/`.
    pub fn install(&self, source: &Path) -> Result<String, PluginError> {
        let manifest_path = source.join("PLUGIN.toml");
        if !manifest_path.exists() {
            return Err(PluginError::MissingManifest);
        }

        let manifest = load_manifest(&manifest_path)?;
        let name = &manifest.plugin.name;
        let target_dir = self.plugins_dir.join(name);

        std::fs::create_dir_all(&target_dir).map_err(|e| {
            PluginError::IoError(format!("create dir {}: {e}", target_dir.display()))
        })?;

        // Copy PLUGIN.toml
        let target_manifest = target_dir.join("PLUGIN.toml");
        std::fs::copy(&manifest_path, &target_manifest)
            .map_err(|e| PluginError::IoError(format!("copy PLUGIN.toml: {e}")))?;

        // Copy all .wasm files
        let entries = std::fs::read_dir(source)
            .map_err(|e| PluginError::IoError(format!("read source dir: {e}")))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
                if let Some(filename) = path.file_name() {
                    let target_file = target_dir.join(filename);
                    std::fs::copy(&path, &target_file).map_err(|e| {
                        PluginError::IoError(format!("copy {}: {e}", path.display()))
                    })?;
                }
            }
        }

        Ok(name.clone())
    }
}

/// Parse a `PLUGIN.toml` file into a `PluginManifest`.
pub fn load_manifest(path: &Path) -> Result<PluginManifest, PluginError> {
    let content = std::fs::read_to_string(path).map_err(|e| PluginError::IoError(e.to_string()))?;
    toml::from_str(&content).map_err(|e| PluginError::ParseError(e.to_string()))
}

/// Scan a directory for the first `.wasm` file and return its bytes.
fn find_wasm_in_dir(dir: &Path) -> Option<Vec<u8>> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
            return std::fs::read(&path).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agentzero-plugin-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be after epoch")
                .as_nanos()
        ))
    }

    const SAMPLE_MANIFEST: &str = r#"
[plugin]
name = "brain"
version = "0.1.0"
description = "Personal LLM wiki"
runtime = "wasm"
wasm_path = "brain.wasm"

[[commands]]
name = "init"
description = "Initialize a brain vault"

[[commands]]
name = "today"
description = "Create or show today's daily note"
"#;

    #[test]
    fn test_parse_manifest() {
        let manifest: PluginManifest =
            toml::from_str(SAMPLE_MANIFEST).expect("should parse manifest");
        assert_eq!(manifest.plugin.name, "brain");
        assert_eq!(manifest.plugin.version, "0.1.0");
        assert_eq!(manifest.plugin.description, "Personal LLM wiki");
        assert_eq!(manifest.plugin.runtime, "wasm");
        assert_eq!(manifest.plugin.wasm_path.as_deref(), Some("brain.wasm"));
        assert_eq!(manifest.commands.len(), 2);
        assert_eq!(manifest.commands[0].name, "init");
        assert_eq!(manifest.commands[1].name, "today");
    }

    #[test]
    fn test_registry_list_empty() {
        let dir = temp_dir("list-empty");
        fs::create_dir_all(&dir).expect("should create");

        let registry = PluginRegistry::new(dir.clone());
        let plugins = registry.list();
        assert!(plugins.is_empty());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_registry_list_nonexistent_dir() {
        let registry = PluginRegistry::new(PathBuf::from("/nonexistent/plugins"));
        let plugins = registry.list();
        assert!(plugins.is_empty());
    }

    #[test]
    fn test_registry_list_with_plugin() {
        let dir = temp_dir("list-plugin");
        let plugin_dir = dir.join("brain");
        fs::create_dir_all(&plugin_dir).expect("should create");
        fs::write(plugin_dir.join("PLUGIN.toml"), SAMPLE_MANIFEST).expect("should write");

        let registry = PluginRegistry::new(dir.clone());
        let plugins = registry.list();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].0.plugin.name, "brain");
        assert_eq!(plugins[0].1, plugin_dir);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_registry_get() {
        let dir = temp_dir("get-plugin");
        let plugin_dir = dir.join("brain");
        fs::create_dir_all(&plugin_dir).expect("should create");
        fs::write(plugin_dir.join("PLUGIN.toml"), SAMPLE_MANIFEST).expect("should write");

        let registry = PluginRegistry::new(dir.clone());

        let result = registry.get("brain");
        assert!(result.is_some());
        let (manifest, path) = result.expect("should find plugin");
        assert_eq!(manifest.plugin.name, "brain");
        assert_eq!(path, plugin_dir);

        assert!(registry.get("nonexistent").is_none());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_registry_install() {
        let source_dir = temp_dir("install-source");
        let target_dir = temp_dir("install-target");
        fs::create_dir_all(&source_dir).expect("should create source");
        fs::create_dir_all(&target_dir).expect("should create target");

        fs::write(source_dir.join("PLUGIN.toml"), SAMPLE_MANIFEST).expect("should write manifest");
        fs::write(source_dir.join("brain.wasm"), b"fake wasm bytes").expect("should write wasm");

        let registry = PluginRegistry::new(target_dir.clone());
        let name = registry.install(&source_dir).expect("should install");
        assert_eq!(name, "brain");

        // Verify files were copied
        let installed_dir = target_dir.join("brain");
        assert!(installed_dir.join("PLUGIN.toml").exists());
        assert!(installed_dir.join("brain.wasm").exists());

        // Verify manifest is loadable from installed location
        let installed = registry.get("brain");
        assert!(installed.is_some());

        fs::remove_dir_all(&source_dir).ok();
        fs::remove_dir_all(&target_dir).ok();
    }

    #[test]
    fn test_registry_install_missing_manifest() {
        let source_dir = temp_dir("install-no-manifest");
        let target_dir = temp_dir("install-target-no-manifest");
        fs::create_dir_all(&source_dir).expect("should create source");
        fs::create_dir_all(&target_dir).expect("should create target");

        let registry = PluginRegistry::new(target_dir.clone());
        let result = registry.install(&source_dir);
        assert!(result.is_err());

        fs::remove_dir_all(&source_dir).ok();
        fs::remove_dir_all(&target_dir).ok();
    }

    #[test]
    fn test_registry_find_wasm() {
        let dir = temp_dir("find-wasm");
        let plugin_dir = dir.join("brain");
        fs::create_dir_all(&plugin_dir).expect("should create");
        fs::write(plugin_dir.join("PLUGIN.toml"), SAMPLE_MANIFEST).expect("should write manifest");
        fs::write(plugin_dir.join("brain.wasm"), b"fake wasm module").expect("should write wasm");

        let registry = PluginRegistry::new(dir.clone());
        let wasm = registry.find_wasm("brain");
        assert!(wasm.is_some());
        assert_eq!(wasm.expect("should find"), b"fake wasm module");

        assert!(registry.find_wasm("nonexistent").is_none());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_registry_find_wasm_fallback_scan() {
        let dir = temp_dir("find-wasm-fallback");
        let plugin_dir = dir.join("myplugin");
        fs::create_dir_all(&plugin_dir).expect("should create");

        // Manifest without wasm_path
        let manifest_no_path = r#"
[plugin]
name = "myplugin"
version = "0.1.0"
description = "Test plugin"
"#;
        fs::write(plugin_dir.join("PLUGIN.toml"), manifest_no_path).expect("should write");
        fs::write(plugin_dir.join("module.wasm"), b"wasm bytes").expect("should write wasm");

        let registry = PluginRegistry::new(dir.clone());
        let wasm = registry.find_wasm("myplugin");
        assert!(wasm.is_some());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_parse_manifest_default_runtime() {
        let manifest_str = r#"
[plugin]
name = "simple"
version = "1.0.0"
description = "A simple plugin"
"#;
        let manifest: PluginManifest = toml::from_str(manifest_str).expect("should parse");
        assert_eq!(manifest.plugin.runtime, "wasm");
        assert!(manifest.commands.is_empty());
    }
}
