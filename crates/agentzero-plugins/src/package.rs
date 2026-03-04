use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

const MANIFEST_FILE_NAME: &str = "manifest.json";
const CURRENT_RUNTIME_API_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    pub id: String,
    pub version: String,
    pub entrypoint: String,
    pub wasm_file: String,
    pub wasm_sha256: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default = "default_runtime_api_version")]
    pub min_runtime_api: u32,
    #[serde(default = "default_runtime_api_version")]
    pub max_runtime_api: u32,
    pub allowed_host_calls: Vec<String>,
}

impl PluginManifest {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.id.trim().is_empty() {
            return Err(anyhow!("plugin id cannot be empty"));
        }
        if self.version.trim().is_empty() {
            return Err(anyhow!("plugin version cannot be empty"));
        }
        if self.entrypoint.trim().is_empty() {
            return Err(anyhow!("plugin entrypoint cannot be empty"));
        }
        if self.wasm_file.trim().is_empty() {
            return Err(anyhow!("plugin wasm_file cannot be empty"));
        }
        if !self.wasm_file.ends_with(".wasm") {
            return Err(anyhow!("plugin wasm_file must end with .wasm"));
        }
        if self.wasm_sha256.len() != 64 || !self.wasm_sha256.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err(anyhow!("plugin wasm_sha256 must be a 64-char hex digest"));
        }
        if self.min_runtime_api == 0 {
            return Err(anyhow!("plugin min_runtime_api must be >= 1"));
        }
        if self.max_runtime_api == 0 {
            return Err(anyhow!("plugin max_runtime_api must be >= 1"));
        }
        if self.min_runtime_api > self.max_runtime_api {
            return Err(anyhow!(
                "plugin runtime API range is invalid (min_runtime_api > max_runtime_api)"
            ));
        }
        self.validate_runtime_compatibility(CURRENT_RUNTIME_API_VERSION)?;
        Ok(())
    }

    pub fn validate_runtime_compatibility(&self, current_api: u32) -> anyhow::Result<()> {
        if current_api < self.min_runtime_api || current_api > self.max_runtime_api {
            return Err(anyhow!(
                "plugin runtime API compatibility failed: current={current_api}, supported={}..={}",
                self.min_runtime_api,
                self.max_runtime_api
            ));
        }
        Ok(())
    }
}

fn default_runtime_api_version() -> u32 {
    CURRENT_RUNTIME_API_VERSION
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub install_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub wasm_path: PathBuf,
    pub manifest: PluginManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledPluginRecord {
    pub id: String,
    pub version: String,
    pub install_dir: PathBuf,
    pub manifest_path: PathBuf,
}

pub fn package_plugin(
    wasm_module_path: impl AsRef<Path>,
    mut manifest: PluginManifest,
    package_path: impl AsRef<Path>,
) -> anyhow::Result<()> {
    let wasm_module_path = wasm_module_path.as_ref();
    let package_path = package_path.as_ref();

    if wasm_module_path.extension().and_then(|v| v.to_str()) != Some("wasm") {
        return Err(anyhow!("plugin module must be a .wasm file"));
    }
    let wasm_bytes = fs::read(wasm_module_path).with_context(|| {
        format!(
            "failed to read wasm module at {}",
            wasm_module_path.display()
        )
    })?;

    // Always regenerate the checksum at package time.
    manifest.wasm_sha256 = sha256_hex(&wasm_bytes);
    manifest.validate()?;

    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).context("failed to serialize plugin manifest")?;

    if let Some(parent) = package_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create package output dir {}", parent.display()))?;
    }

    let file = fs::File::create(package_path)
        .with_context(|| format!("failed to create package file {}", package_path.display()))?;
    let mut builder = tar::Builder::new(file);

    let mut manifest_header = tar::Header::new_gnu();
    manifest_header.set_size(manifest_bytes.len() as u64);
    manifest_header.set_mode(0o644);
    manifest_header.set_cksum();
    builder
        .append_data(
            &mut manifest_header,
            MANIFEST_FILE_NAME,
            Cursor::new(manifest_bytes),
        )
        .context("failed to append manifest to package")?;

    let mut wasm_header = tar::Header::new_gnu();
    wasm_header.set_size(wasm_bytes.len() as u64);
    wasm_header.set_mode(0o644);
    wasm_header.set_cksum();
    builder
        .append_data(
            &mut wasm_header,
            &manifest.wasm_file,
            Cursor::new(wasm_bytes),
        )
        .context("failed to append wasm module to package")?;

    builder
        .finish()
        .context("failed to finalize plugin package")?;
    Ok(())
}

pub fn install_packaged_plugin(
    package_path: impl AsRef<Path>,
    install_root: impl AsRef<Path>,
) -> anyhow::Result<InstalledPlugin> {
    let package_path = package_path.as_ref();
    let install_root = install_root.as_ref();

    let archive_file = fs::File::open(package_path)
        .with_context(|| format!("failed to open package {}", package_path.display()))?;
    let mut archive = tar::Archive::new(archive_file);

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for entry in archive
        .entries()
        .context("failed to read package entries")?
    {
        let mut entry = entry.context("failed to parse package entry")?;
        let entry_path = entry
            .path()
            .context("failed to read package entry path")?
            .to_string_lossy()
            .to_string();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .with_context(|| format!("failed to read package entry `{entry_path}`"))?;
        files.insert(entry_path, bytes);
    }

    let manifest_bytes = files
        .get(MANIFEST_FILE_NAME)
        .ok_or_else(|| anyhow!("package missing manifest.json"))?;
    let manifest: PluginManifest =
        serde_json::from_slice(manifest_bytes).context("failed to deserialize plugin manifest")?;
    manifest.validate()?;

    let wasm_bytes = files
        .get(&manifest.wasm_file)
        .ok_or_else(|| anyhow!("package missing wasm module `{}`", manifest.wasm_file))?;

    let digest = sha256_hex(wasm_bytes);
    if digest != manifest.wasm_sha256 {
        return Err(anyhow!(
            "integrity check failed for `{}`: checksum mismatch",
            manifest.wasm_file
        ));
    }

    let install_dir = install_root.join(&manifest.id).join(&manifest.version);
    fs::create_dir_all(&install_dir)
        .with_context(|| format!("failed to create install dir {}", install_dir.display()))?;

    let manifest_path = install_dir.join(MANIFEST_FILE_NAME);
    let wasm_path = install_dir.join(&manifest.wasm_file);
    fs::write(&manifest_path, manifest_bytes)
        .with_context(|| format!("failed to write manifest at {}", manifest_path.display()))?;
    fs::write(&wasm_path, wasm_bytes)
        .with_context(|| format!("failed to write wasm at {}", wasm_path.display()))?;

    Ok(InstalledPlugin {
        install_dir,
        manifest_path,
        wasm_path,
        manifest,
    })
}

pub fn list_installed_plugins(
    install_root: impl AsRef<Path>,
) -> anyhow::Result<Vec<InstalledPluginRecord>> {
    let install_root = install_root.as_ref();
    if !install_root.exists() {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    for plugin_dir in fs::read_dir(install_root)
        .with_context(|| format!("failed to read install root {}", install_root.display()))?
    {
        let plugin_dir = plugin_dir.context("failed to read plugin dir entry")?;
        if !plugin_dir.file_type()?.is_dir() {
            continue;
        }
        let plugin_id = plugin_dir.file_name().to_string_lossy().to_string();

        for version_dir in fs::read_dir(plugin_dir.path()).with_context(|| {
            format!(
                "failed to read plugin versions for {}",
                plugin_dir.path().display()
            )
        })? {
            let version_dir = version_dir.context("failed to read plugin version entry")?;
            if !version_dir.file_type()?.is_dir() {
                continue;
            }
            let version = version_dir.file_name().to_string_lossy().to_string();
            let manifest_path = version_dir.path().join(MANIFEST_FILE_NAME);
            if !manifest_path.exists() {
                continue;
            }
            records.push(InstalledPluginRecord {
                id: plugin_id.clone(),
                version,
                install_dir: version_dir.path(),
                manifest_path,
            });
        }
    }

    records.sort_by(|a, b| a.id.cmp(&b.id).then(a.version.cmp(&b.version)));
    Ok(records)
}

pub fn remove_installed_plugin(
    install_root: impl AsRef<Path>,
    plugin_id: &str,
    version: Option<&str>,
) -> anyhow::Result<usize> {
    let install_root = install_root.as_ref();
    if plugin_id.trim().is_empty() {
        return Err(anyhow!("plugin id cannot be empty"));
    }
    let plugin_root = install_root.join(plugin_id);
    if !plugin_root.exists() {
        return Ok(0);
    }

    if let Some(version) = version {
        let target = plugin_root.join(version);
        if !target.exists() {
            return Ok(0);
        }
        fs::remove_dir_all(&target)
            .with_context(|| format!("failed to remove plugin dir {}", target.display()))?;
        if plugin_root
            .read_dir()
            .with_context(|| format!("failed to read plugin dir {}", plugin_root.display()))?
            .next()
            .is_none()
        {
            fs::remove_dir_all(&plugin_root).with_context(|| {
                format!("failed to remove plugin root {}", plugin_root.display())
            })?;
        }
        return Ok(1);
    }

    let mut removed = 0usize;
    for entry in fs::read_dir(&plugin_root)
        .with_context(|| format!("failed to read plugin dir {}", plugin_root.display()))?
    {
        let entry = entry.context("failed to parse plugin version entry")?;
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path()).with_context(|| {
                format!(
                    "failed to remove plugin version dir {}",
                    entry.path().display()
                )
            })?;
            removed += 1;
        }
    }
    fs::remove_dir_all(&plugin_root)
        .with_context(|| format!("failed to remove plugin root {}", plugin_root.display()))?;
    Ok(removed)
}

/// A plugin discovered on disk, ready to be loaded as a `WasmTool`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPlugin {
    pub manifest: PluginManifest,
    pub wasm_path: PathBuf,
    /// Whether this plugin was found in a development directory (CWD/plugins/).
    pub dev_mode: bool,
}

/// Discover installed plugins by scanning up to three directory tiers:
///
/// 1. **Global**: `{global_plugin_dir}/` — user-installed plugins
/// 2. **Project**: `{project_plugin_dir}/` — project-specific plugins
/// 3. **Development**: `{cwd_plugin_dir}/` — in-development plugins (hot-reload)
///
/// Later tiers override earlier ones when the same plugin id is found.
/// Each directory is expected to have the structure used by `install_packaged_plugin`:
///   `<plugin_id>/<version>/manifest.json` + `<wasm_file>`
///
/// The development directory also supports a flat layout for convenience:
///   `<plugin_id>/manifest.json` + `<wasm_file>` (no version subdir)
///
/// Invalid manifests are warned and skipped — they never cause a hard failure.
pub fn discover_plugins(
    global_plugin_dir: Option<&Path>,
    project_plugin_dir: Option<&Path>,
    cwd_plugin_dir: Option<&Path>,
) -> Vec<DiscoveredPlugin> {
    let mut plugins: std::collections::HashMap<String, DiscoveredPlugin> =
        std::collections::HashMap::new();

    // Tier 1: Global
    if let Some(dir) = global_plugin_dir {
        for plugin in scan_plugin_dir(dir, false) {
            plugins.insert(plugin.manifest.id.clone(), plugin);
        }
    }

    // Tier 2: Project
    if let Some(dir) = project_plugin_dir {
        for plugin in scan_plugin_dir(dir, false) {
            plugins.insert(plugin.manifest.id.clone(), plugin);
        }
    }

    // Tier 3: Development (CWD)
    if let Some(dir) = cwd_plugin_dir {
        for plugin in scan_plugin_dir(dir, true) {
            plugins.insert(plugin.manifest.id.clone(), plugin);
        }
    }

    let mut result: Vec<DiscoveredPlugin> = plugins.into_values().collect();
    result.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    result
}

/// Scan a single plugin directory for installed plugins.
///
/// Supports two layouts:
///   - Versioned: `<id>/<version>/manifest.json`
///   - Flat (dev): `<id>/manifest.json`
fn scan_plugin_dir(dir: &Path, dev_mode: bool) -> Vec<DiscoveredPlugin> {
    let mut found = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return found, // Missing dir = zero plugins
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.path().is_dir() {
            continue;
        }

        // Try flat layout first: <id>/manifest.json
        let flat_manifest = entry.path().join(MANIFEST_FILE_NAME);
        if flat_manifest.exists() {
            if let Some(plugin) = try_load_plugin(&entry.path(), dev_mode) {
                found.push(plugin);
                continue;
            }
        }

        // Try versioned layout: <id>/<version>/manifest.json
        if let Ok(version_entries) = fs::read_dir(entry.path()) {
            // Pick the latest version directory (lexicographic sort, last wins)
            let mut best: Option<DiscoveredPlugin> = None;
            for version_entry in version_entries {
                let version_entry = match version_entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if !version_entry.path().is_dir() {
                    continue;
                }
                if let Some(plugin) = try_load_plugin(&version_entry.path(), dev_mode) {
                    match &best {
                        Some(existing) if existing.manifest.version >= plugin.manifest.version => {}
                        _ => best = Some(plugin),
                    }
                }
            }
            if let Some(plugin) = best {
                found.push(plugin);
            }
        }
    }

    found
}

/// Attempt to load a plugin from a directory containing `manifest.json`.
fn try_load_plugin(dir: &Path, dev_mode: bool) -> Option<DiscoveredPlugin> {
    let manifest_path = dir.join(MANIFEST_FILE_NAME);
    let bytes = match fs::read(&manifest_path) {
        Ok(b) => b,
        Err(_) => return None,
    };
    let manifest: PluginManifest = match serde_json::from_slice(&bytes) {
        Ok(m) => m,
        Err(e) => {
            #[cfg(feature = "wasm-runtime")]
            tracing::warn!(
                "skipping plugin at {}: invalid manifest: {e}",
                dir.display()
            );
            let _ = e;
            return None;
        }
    };
    if let Err(e) = manifest.validate() {
        #[cfg(feature = "wasm-runtime")]
        tracing::warn!("skipping plugin {}: validation failed: {e}", manifest.id);
        let _ = e;
        return None;
    }

    let wasm_path = dir.join(&manifest.wasm_file);
    if !wasm_path.exists() {
        #[cfg(feature = "wasm-runtime")]
        tracing::warn!(
            "skipping plugin {}: wasm file not found at {}",
            manifest.id,
            wasm_path.display()
        );
        return None;
    }

    Some(DiscoveredPlugin {
        manifest,
        wasm_path,
        dev_mode,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    format!("{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::{
        install_packaged_plugin, list_installed_plugins, package_plugin, remove_installed_plugin,
        PluginManifest,
    };
    use anyhow::Context;
    use std::fs;
    use std::io::Cursor;

    fn sample_manifest() -> PluginManifest {
        PluginManifest {
            id: "sample-plugin".to_string(),
            version: "1.0.0".to_string(),
            entrypoint: "run".to_string(),
            wasm_file: "plugin.wasm".to_string(),
            wasm_sha256: "0".repeat(64),
            capabilities: vec!["tool.call".to_string()],
            hooks: vec!["before_tool_call".to_string()],
            min_runtime_api: 1,
            max_runtime_api: 2,
            allowed_host_calls: vec![],
        }
    }

    #[test]
    fn package_and_install_round_trip_success_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let wasm_path = tmp.path().join("plugin.wasm");
        let package_path = tmp.path().join("sample-plugin.tar");
        let install_root = tmp.path().join("installed");

        let wasm_bytes = wat::parse_str(
            r#"(module
                (func (export "run") (result i32)
                    i32.const 7)
            )"#,
        )
        .expect("wat should compile");
        fs::write(&wasm_path, wasm_bytes).expect("wasm file should be written");

        package_plugin(&wasm_path, sample_manifest(), &package_path)
            .expect("packaging should succeed");
        let installed =
            install_packaged_plugin(&package_path, &install_root).expect("install should succeed");

        assert_eq!(installed.manifest.id, "sample-plugin");
        assert!(installed.manifest_path.exists());
        assert!(installed.wasm_path.exists());
        assert_eq!(
            installed.install_dir,
            install_root.join("sample-plugin").join("1.0.0")
        );
    }

    #[test]
    fn install_rejects_checksum_mismatch_negative_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let package_path = tmp.path().join("tampered-plugin.tar");
        let install_root = tmp.path().join("installed");

        let wasm_bytes = wat::parse_str(
            r#"(module
                (func (export "run") (result i32)
                    i32.const 1)
            )"#,
        )
        .expect("wat should compile");

        let mut manifest = sample_manifest();
        manifest.wasm_sha256 = "f".repeat(64);
        let manifest_bytes =
            serde_json::to_vec_pretty(&manifest).expect("manifest should serialize");

        let file = fs::File::create(&package_path).expect("package should be created");
        let mut builder = tar::Builder::new(file);

        let mut manifest_header = tar::Header::new_gnu();
        manifest_header.set_size(manifest_bytes.len() as u64);
        manifest_header.set_mode(0o644);
        manifest_header.set_cksum();
        builder
            .append_data(
                &mut manifest_header,
                "manifest.json",
                Cursor::new(manifest_bytes),
            )
            .expect("manifest should be added");

        let mut wasm_header = tar::Header::new_gnu();
        wasm_header.set_size(wasm_bytes.len() as u64);
        wasm_header.set_mode(0o644);
        wasm_header.set_cksum();
        builder
            .append_data(&mut wasm_header, "plugin.wasm", Cursor::new(wasm_bytes))
            .expect("wasm should be added");
        builder.finish().expect("archive should finish");

        let err = install_packaged_plugin(&package_path, &install_root)
            .context("tampered package should fail integrity")
            .expect_err("install should fail");
        let err_text = format!("{err:#}");
        assert!(
            err_text.contains("integrity check failed") || err_text.contains("checksum mismatch"),
            "unexpected tamper error: {err_text}"
        );
    }

    #[test]
    fn list_and_remove_installed_plugins_success_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let wasm_path = tmp.path().join("plugin.wasm");
        let package_path = tmp.path().join("sample-plugin.tar");
        let install_root = tmp.path().join("installed");

        let wasm_bytes = wat::parse_str(
            r#"(module
                (func (export "run") (result i32)
                    i32.const 9)
            )"#,
        )
        .expect("wat should compile");
        fs::write(&wasm_path, wasm_bytes).expect("wasm should be written");
        package_plugin(&wasm_path, sample_manifest(), &package_path)
            .expect("package should succeed");
        install_packaged_plugin(&package_path, &install_root).expect("install should succeed");

        let listed = list_installed_plugins(&install_root).expect("list should succeed");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "sample-plugin");
        assert_eq!(listed[0].version, "1.0.0");

        let removed = remove_installed_plugin(&install_root, "sample-plugin", Some("1.0.0"))
            .expect("remove should succeed");
        assert_eq!(removed, 1);
        assert!(list_installed_plugins(&install_root)
            .expect("list should succeed")
            .is_empty());
    }

    #[test]
    fn remove_installed_plugin_rejects_empty_id_negative_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let install_root = tmp.path().join("installed");
        let err =
            remove_installed_plugin(&install_root, "", None).expect_err("empty id should fail");
        assert!(err.to_string().contains("plugin id cannot be empty"));
    }

    #[test]
    fn manifest_validate_rejects_incompatible_runtime_api_negative_path() {
        let mut manifest = sample_manifest();
        manifest.min_runtime_api = 3;
        manifest.max_runtime_api = 4;

        let err = manifest
            .validate()
            .expect_err("incompatible API should fail");
        assert!(err.to_string().contains("runtime API compatibility failed"));
    }

    // --- Discovery tests ---

    use super::discover_plugins;

    fn write_test_plugin(dir: &std::path::Path, id: &str, version: &str) {
        fs::create_dir_all(dir).expect("plugin dir should be created");
        let wasm_bytes =
            wat::parse_str(r#"(module (func (export "run") (result i32) i32.const 42))"#)
                .expect("wat should compile");
        let sha = super::sha256_hex(&wasm_bytes);
        let manifest = PluginManifest {
            id: id.to_string(),
            version: version.to_string(),
            entrypoint: "run".to_string(),
            wasm_file: "plugin.wasm".to_string(),
            wasm_sha256: sha,
            capabilities: vec![],
            hooks: vec![],
            min_runtime_api: 1,
            max_runtime_api: 2,
            allowed_host_calls: vec![],
        };
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest should serialize"),
        )
        .expect("manifest should write");
        fs::write(dir.join("plugin.wasm"), &wasm_bytes).expect("wasm should write");
    }

    #[test]
    fn discover_plugins_empty_dirs_returns_empty() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let global = tmp.path().join("global");
        let project = tmp.path().join("project");
        let cwd = tmp.path().join("cwd");
        // Dirs don't exist — should return empty, not error
        let found = discover_plugins(Some(&global), Some(&project), Some(&cwd));
        assert!(found.is_empty());
    }

    #[test]
    fn discover_plugins_finds_versioned_layout() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let global = tmp.path().join("global");
        write_test_plugin(&global.join("my-tool").join("1.0.0"), "my-tool", "1.0.0");

        let found = discover_plugins(Some(&global), None, None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.id, "my-tool");
        assert_eq!(found[0].manifest.version, "1.0.0");
        assert!(!found[0].dev_mode);
    }

    #[test]
    fn discover_plugins_finds_flat_layout() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let cwd = tmp.path().join("plugins");
        write_test_plugin(&cwd.join("dev-tool"), "dev-tool", "0.1.0");

        let found = discover_plugins(None, None, Some(&cwd));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.id, "dev-tool");
        assert!(found[0].dev_mode);
    }

    #[test]
    fn discover_plugins_later_tier_overrides_earlier() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let global = tmp.path().join("global");
        let project = tmp.path().join("project");
        write_test_plugin(&global.join("shared").join("1.0.0"), "shared", "1.0.0");
        write_test_plugin(&project.join("shared").join("2.0.0"), "shared", "2.0.0");

        let found = discover_plugins(Some(&global), Some(&project), None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.version, "2.0.0");
    }

    #[test]
    fn discover_plugins_picks_latest_version() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let global = tmp.path().join("global");
        write_test_plugin(&global.join("multi-v").join("1.0.0"), "multi-v", "1.0.0");
        write_test_plugin(&global.join("multi-v").join("2.0.0"), "multi-v", "2.0.0");

        let found = discover_plugins(Some(&global), None, None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.version, "2.0.0");
    }

    #[test]
    fn discover_plugins_skips_invalid_manifest() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let global = tmp.path().join("global");
        let bad_dir = global.join("bad-plugin").join("1.0.0");
        fs::create_dir_all(&bad_dir).expect("dir should be created");
        fs::write(bad_dir.join("manifest.json"), b"not json").expect("write bad manifest");
        fs::write(bad_dir.join("plugin.wasm"), b"\0asm\x01\0\0\0").expect("write wasm");

        let found = discover_plugins(Some(&global), None, None);
        assert!(found.is_empty(), "invalid manifest should be skipped");
    }

    #[test]
    fn discover_plugins_skips_missing_wasm_file() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let global = tmp.path().join("global");
        let dir = global.join("no-wasm").join("1.0.0");
        fs::create_dir_all(&dir).expect("dir should be created");
        let manifest = PluginManifest {
            id: "no-wasm".to_string(),
            version: "1.0.0".to_string(),
            entrypoint: "run".to_string(),
            wasm_file: "plugin.wasm".to_string(),
            wasm_sha256: "a".repeat(64),
            capabilities: vec![],
            hooks: vec![],
            min_runtime_api: 1,
            max_runtime_api: 2,
            allowed_host_calls: vec![],
        };
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("serialize"),
        )
        .expect("write");

        let found = discover_plugins(Some(&global), None, None);
        assert!(found.is_empty(), "missing wasm should be skipped");
    }

    #[test]
    fn discover_plugins_none_dirs_returns_empty() {
        let found = discover_plugins(None, None, None);
        assert!(found.is_empty());
    }
}
