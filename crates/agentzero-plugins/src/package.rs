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
}
