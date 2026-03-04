use anyhow::{anyhow, Context};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

const MANIFEST_FILE_NAME: &str = "manifest.json";
const CURRENT_RUNTIME_API_VERSION: u32 = 2;
const LOCK_FILE_NAME: &str = ".agentzero-plugins.lock";

/// Acquire an exclusive file lock on the install root directory.
/// Returns a guard that releases the lock when dropped.
fn lock_install_root(install_root: &Path) -> anyhow::Result<fs::File> {
    fs::create_dir_all(install_root)
        .with_context(|| format!("failed to create install root {}", install_root.display()))?;
    let lock_path = install_root.join(LOCK_FILE_NAME);
    let lock_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&lock_path)
        .with_context(|| format!("failed to open lock file {}", lock_path.display()))?;
    lock_file
        .lock_exclusive()
        .with_context(|| format!("failed to acquire lock on {}", lock_path.display()))?;
    Ok(lock_file)
}

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

    // Acquire an exclusive lock so concurrent installs don't corrupt state.
    let _lock = lock_install_root(install_root)?;

    let archive_file = fs::File::open(package_path)
        .with_context(|| format!("failed to open package {}", package_path.display()))?;
    let mut archive = tar::Archive::new(archive_file);

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for entry in archive
        .entries()
        .context("failed to read package entries")?
    {
        let mut entry = entry.context("failed to parse package entry")?;

        // Reject symlinks — they can be used to escape the install directory.
        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            anyhow::bail!("plugin package contains a symlink entry (rejected for security)");
        }

        let entry_path = entry
            .path()
            .context("failed to read package entry path")?
            .to_string_lossy()
            .to_string();

        // Reject path traversal: absolute paths or parent-directory components.
        if entry_path.starts_with('/') || entry_path.contains("..") {
            anyhow::bail!(
                "path traversal in plugin package: `{entry_path}` (rejected for security)"
            );
        }

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

    records.sort_by(|a, b| {
        a.id.cmp(&b.id).then_with(|| {
            match (
                semver::Version::parse(&a.version),
                semver::Version::parse(&b.version),
            ) {
                (Ok(va), Ok(vb)) => va.cmp(&vb),
                _ => a.version.cmp(&b.version),
            }
        })
    });
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

    // Acquire an exclusive lock so concurrent removes don't corrupt state.
    let _lock = lock_install_root(install_root)?;

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
                        Some(existing)
                            if version_ge(&existing.manifest.version, &plugin.manifest.version) => {
                        }
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

/// Compare two version strings using semver when possible, falling back to
/// lexicographic comparison for non-semver strings.
fn version_ge(a: &str, b: &str) -> bool {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(va), Ok(vb)) => va >= vb,
        _ => a >= b,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    format!("{digest:x}")
}

// ── Plugin State Management ──────────────────────────────────────────

/// Persistent state for a single installed plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginStateEntry {
    pub version: String,
    pub enabled: bool,
    pub installed_at: String,
    /// Where this plugin was installed from.
    pub source: String,
}

/// Top-level plugin state file, stored at `{data_dir}/plugin-state.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginState {
    pub plugins: std::collections::HashMap<String, PluginStateEntry>,
}

const STATE_FILE_NAME: &str = "plugin-state.json";

impl PluginState {
    /// Load plugin state from a data directory. Returns default (empty) if
    /// the file doesn't exist or can't be parsed.
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join(STATE_FILE_NAME);
        match fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save plugin state to a data directory.
    pub fn save(&self, data_dir: &Path) -> anyhow::Result<()> {
        let path = data_dir.join(STATE_FILE_NAME);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create state dir: {}", parent.display()))?;
        }
        let json =
            serde_json::to_string_pretty(self).context("failed to serialize plugin state")?;
        fs::write(&path, json)
            .with_context(|| format!("failed to write plugin state: {}", path.display()))?;
        Ok(())
    }

    /// Check whether a plugin is enabled. Returns `true` if the plugin has
    /// no state entry (default is enabled).
    pub fn is_enabled(&self, id: &str) -> bool {
        self.plugins.get(id).map(|e| e.enabled).unwrap_or(true)
    }

    /// Enable a plugin. Creates an entry if one doesn't exist.
    pub fn enable(&mut self, id: &str) -> anyhow::Result<()> {
        match self.plugins.get_mut(id) {
            Some(entry) => {
                entry.enabled = true;
                Ok(())
            }
            None => Err(anyhow!(
                "plugin '{}' has no state entry (not installed via CLI)",
                id
            )),
        }
    }

    /// Disable a plugin. Creates an entry if one doesn't exist.
    pub fn disable(&mut self, id: &str) -> anyhow::Result<()> {
        match self.plugins.get_mut(id) {
            Some(entry) => {
                entry.enabled = false;
                Ok(())
            }
            None => Err(anyhow!(
                "plugin '{}' has no state entry (not installed via CLI)",
                id
            )),
        }
    }

    /// Record that a plugin was installed.
    pub fn record_install(&mut self, id: &str, version: &str, source: &str) {
        self.plugins.insert(
            id.to_string(),
            PluginStateEntry {
                version: version.to_string(),
                enabled: true,
                installed_at: chrono_now_iso(),
                source: source.to_string(),
            },
        );
    }

    /// Remove a plugin's state entry.
    pub fn remove(&mut self, id: &str) {
        self.plugins.remove(id);
    }
}

fn chrono_now_iso() -> String {
    // Simple ISO 8601 timestamp without chrono dependency
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Format as seconds-since-epoch (precise enough for state tracking)
    format!("{secs}")
}

/// Filter discovered plugins by state, removing disabled ones.
pub fn filter_by_state(
    plugins: Vec<DiscoveredPlugin>,
    state: &PluginState,
) -> Vec<DiscoveredPlugin> {
    plugins
        .into_iter()
        .filter(|p| state.is_enabled(&p.manifest.id))
        .collect()
}

/// Download a plugin package from a URL, verify its SHA256 if provided,
/// and install it.
pub fn install_from_url(
    url: &str,
    install_root: &Path,
    expected_sha256: Option<&str>,
) -> anyhow::Result<InstalledPlugin> {
    // Use a blocking HTTP GET — the plugin CLI commands are already async
    // but the core package functions are sync. This uses std::net via ureq
    // would be ideal, but we'll use a minimal approach with the existing
    // reqwest/hyper stack or fall back to curl. For now, check if the URL
    // is a file:// URL first.
    let bytes = if let Some(file_path) = url.strip_prefix("file://") {
        fs::read(file_path).with_context(|| format!("failed to read local package: {file_path}"))?
    } else {
        return Err(anyhow!(
            "URL-based install requires HTTP support. Use 'file://<path>' for local archives \
             or download the package manually and use '--package <path>'."
        ));
    };

    // Verify SHA256 if provided
    if let Some(expected) = expected_sha256 {
        let actual = sha256_hex(&bytes);
        if actual != expected {
            return Err(anyhow!(
                "SHA256 mismatch: expected {expected}, got {actual}"
            ));
        }
    }

    // Write to a temp file and install
    let tmp_dir =
        std::env::temp_dir().join(format!("agentzero-plugin-download-{}", std::process::id()));
    fs::create_dir_all(&tmp_dir)?;
    let tmp_path = tmp_dir.join("package.tar");
    fs::write(&tmp_path, &bytes)?;

    let result = install_packaged_plugin(&tmp_path, install_root);

    // Clean up temp file
    fs::remove_dir_all(&tmp_dir).ok();

    result
}

// ── Plugin Registry ───────────────────────────────────────────────────────

/// A single version entry in the registry index for a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryVersionEntry {
    pub version: String,
    pub download_url: String,
    pub sha256: String,
    pub min_runtime_api: u32,
    pub max_runtime_api: u32,
}

/// An entry in the registry index representing one plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub repository: String,
    pub latest: String,
    pub versions: Vec<RegistryVersionEntry>,
}

/// The full registry index loaded from disk or network.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryIndex {
    pub plugins: Vec<RegistryEntry>,
}

const REGISTRY_CACHE_DIR: &str = "registry-cache";
const REGISTRY_INDEX_FILE: &str = "index.json";
const REGISTRY_CACHE_MAX_AGE_SECS: u64 = 3600; // 1 hour

impl RegistryIndex {
    /// Load a cached registry index from the data directory.
    /// Returns `None` if the cache is missing or expired.
    pub fn load_cached(data_dir: &Path) -> Option<Self> {
        let cache_path = data_dir.join(REGISTRY_CACHE_DIR).join(REGISTRY_INDEX_FILE);
        if !cache_path.exists() {
            return None;
        }

        // Check if cache is expired
        if let Ok(meta) = fs::metadata(&cache_path) {
            if let Ok(modified) = meta.modified() {
                let age = std::time::SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or_default();
                if age.as_secs() > REGISTRY_CACHE_MAX_AGE_SECS {
                    return None;
                }
            }
        }

        let data = fs::read_to_string(&cache_path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Save the registry index to the cache directory.
    pub fn save_cache(&self, data_dir: &Path) -> anyhow::Result<()> {
        let cache_dir = data_dir.join(REGISTRY_CACHE_DIR);
        fs::create_dir_all(&cache_dir)?;
        let json = serde_json::to_string_pretty(self)?;
        fs::write(cache_dir.join(REGISTRY_INDEX_FILE), json)?;
        Ok(())
    }

    /// Search the index by query string (case-insensitive substring match
    /// on id, description, and category).
    pub fn search(&self, query: &str) -> Vec<&RegistryEntry> {
        let q = query.to_lowercase();
        self.plugins
            .iter()
            .filter(|p| {
                p.id.to_lowercase().contains(&q)
                    || p.description.to_lowercase().contains(&q)
                    || p.category.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Look up a specific plugin by id.
    pub fn get(&self, id: &str) -> Option<&RegistryEntry> {
        self.plugins.iter().find(|p| p.id == id)
    }
}

impl RegistryEntry {
    /// Get the latest version entry.
    pub fn latest_version(&self) -> Option<&RegistryVersionEntry> {
        self.versions.iter().find(|v| v.version == self.latest)
    }

    /// Check whether a newer version exists compared to `current`.
    pub fn has_update(&self, current: &str) -> bool {
        self.latest != current
    }
}

/// Load or refresh the registry index.
///
/// Tries the cache first. If expired or missing, reads from a local file
/// path (for development) or returns an error suggesting manual refresh.
pub fn load_registry_index(
    data_dir: &Path,
    registry_url: Option<&str>,
) -> anyhow::Result<RegistryIndex> {
    // Try cache first
    if let Some(cached) = RegistryIndex::load_cached(data_dir) {
        return Ok(cached);
    }

    // Try fetching from URL (file:// for now)
    if let Some(url) = registry_url {
        if let Some(file_path) = url.strip_prefix("file://") {
            let data = fs::read_to_string(file_path)
                .with_context(|| format!("failed to read registry index: {file_path}"))?;
            let index: RegistryIndex =
                serde_json::from_str(&data).with_context(|| "failed to parse registry index")?;
            index.save_cache(data_dir)?;
            return Ok(index);
        }
        return Err(anyhow!(
            "HTTP registry fetch not yet supported. Use 'file://<path>' for local registries \
             or run 'plugin refresh' after manually downloading the index."
        ));
    }

    Err(anyhow!(
        "No registry cache found and no registry URL configured. \
         Set 'plugins.registry_url' in config or run 'plugin refresh --url <url>'."
    ))
}

/// Check which installed plugins have updates available in the registry.
pub fn check_outdated(state: &PluginState, index: &RegistryIndex) -> Vec<(String, String, String)> {
    // Returns vec of (id, installed_version, latest_version)
    let mut outdated = Vec::new();
    for (id, entry) in &state.plugins {
        if let Some(reg) = index.get(id) {
            if reg.has_update(&entry.version) {
                outdated.push((id.clone(), entry.version.clone(), reg.latest.clone()));
            }
        }
    }
    outdated
}

/// Parameters for generating a registry index entry.
///
/// Use this instead of a long argument list per Rule 10 (builder pattern for >3-4 params).
#[derive(Debug, Clone)]
pub struct RegistryEntryParams<'a> {
    pub manifest: &'a PluginManifest,
    pub description: &'a str,
    pub category: &'a str,
    pub author: &'a str,
    pub repository: &'a str,
    pub download_url: &'a str,
    pub wasm_sha256: &'a str,
}

/// Generate a registry index entry for a plugin, suitable for `plugin publish`.
pub fn generate_registry_entry(params: &RegistryEntryParams<'_>) -> RegistryEntry {
    RegistryEntry {
        id: params.manifest.id.clone(),
        description: params.description.to_string(),
        category: params.category.to_string(),
        author: params.author.to_string(),
        repository: params.repository.to_string(),
        latest: params.manifest.version.clone(),
        versions: vec![RegistryVersionEntry {
            version: params.manifest.version.clone(),
            download_url: params.download_url.to_string(),
            sha256: params.wasm_sha256.to_string(),
            min_runtime_api: params.manifest.min_runtime_api,
            max_runtime_api: params.manifest.max_runtime_api,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        filter_by_state, install_packaged_plugin, list_installed_plugins, package_plugin,
        remove_installed_plugin, DiscoveredPlugin, PluginManifest, PluginState,
    };
    use anyhow::Context;
    use std::fs;
    use std::io::Cursor;
    use std::path::PathBuf;

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

    // ── Plugin State tests ──────────────────────────────────────────

    #[test]
    fn plugin_state_load_missing_returns_default() {
        let dir = std::env::temp_dir().join(format!("az-state-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let state = PluginState::load(&dir);
        assert!(state.plugins.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn plugin_state_save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("az-state-test-rt-{}", std::process::id()));
        let mut state = PluginState::default();
        state.record_install("test-plugin", "1.0.0", "local");
        state.save(&dir).expect("save should succeed");

        let loaded = PluginState::load(&dir);
        assert_eq!(loaded.plugins.len(), 1);
        let entry = loaded.plugins.get("test-plugin").unwrap();
        assert_eq!(entry.version, "1.0.0");
        assert!(entry.enabled);
        assert_eq!(entry.source, "local");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn plugin_state_enable_disable_toggle() {
        let dir = std::env::temp_dir().join(format!("az-state-test-toggle-{}", std::process::id()));
        let mut state = PluginState::default();
        state.record_install("toggle-me", "0.1.0", "local");

        assert!(state.is_enabled("toggle-me"));

        state.disable("toggle-me").unwrap();
        assert!(!state.is_enabled("toggle-me"));

        state.enable("toggle-me").unwrap();
        assert!(state.is_enabled("toggle-me"));

        state.save(&dir).expect("save should succeed");
        let loaded = PluginState::load(&dir);
        assert!(loaded.is_enabled("toggle-me"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn plugin_state_missing_entry_defaults_to_enabled() {
        let state = PluginState::default();
        assert!(state.is_enabled("nonexistent"));
    }

    #[test]
    fn plugin_state_enable_missing_fails() {
        let mut state = PluginState::default();
        let err = state.enable("unknown").expect_err("should fail");
        assert!(err.to_string().contains("no state entry"));
    }

    #[test]
    fn plugin_state_remove_entry() {
        let mut state = PluginState::default();
        state.record_install("removable", "1.0.0", "url");
        assert!(state.plugins.contains_key("removable"));
        state.remove("removable");
        assert!(!state.plugins.contains_key("removable"));
    }

    #[test]
    fn filter_by_state_removes_disabled() {
        let mut state = PluginState::default();
        state.record_install("enabled-plugin", "1.0.0", "local");
        state.record_install("disabled-plugin", "1.0.0", "local");
        state.disable("disabled-plugin").unwrap();

        let plugins = vec![
            DiscoveredPlugin {
                manifest: PluginManifest {
                    id: "enabled-plugin".to_string(),
                    version: "1.0.0".to_string(),
                    entrypoint: "run".to_string(),
                    wasm_file: "plugin.wasm".to_string(),
                    wasm_sha256: "a".repeat(64),
                    capabilities: vec![],
                    hooks: vec![],
                    min_runtime_api: 1,
                    max_runtime_api: 2,
                    allowed_host_calls: vec![],
                },
                wasm_path: PathBuf::from("/tmp/a.wasm"),
                dev_mode: false,
            },
            DiscoveredPlugin {
                manifest: PluginManifest {
                    id: "disabled-plugin".to_string(),
                    version: "1.0.0".to_string(),
                    entrypoint: "run".to_string(),
                    wasm_file: "plugin.wasm".to_string(),
                    wasm_sha256: "b".repeat(64),
                    capabilities: vec![],
                    hooks: vec![],
                    min_runtime_api: 1,
                    max_runtime_api: 2,
                    allowed_host_calls: vec![],
                },
                wasm_path: PathBuf::from("/tmp/b.wasm"),
                dev_mode: false,
            },
        ];

        let filtered = filter_by_state(plugins, &state);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].manifest.id, "enabled-plugin");
    }

    // ── Registry tests ───────────────────────────────────────────────────

    use super::{
        check_outdated, generate_registry_entry, load_registry_index, RegistryEntry,
        RegistryEntryParams, RegistryIndex, RegistryVersionEntry,
    };

    fn sample_registry_index() -> RegistryIndex {
        RegistryIndex {
            plugins: vec![
                RegistryEntry {
                    id: "hardware-tools".to_string(),
                    description: "Board info and memory tools".to_string(),
                    category: "hardware".to_string(),
                    author: "agentzero".to_string(),
                    repository: "https://github.com/agentzero/plugins".to_string(),
                    latest: "1.2.0".to_string(),
                    versions: vec![
                        RegistryVersionEntry {
                            version: "1.0.0".to_string(),
                            download_url: "https://example.com/hw-1.0.0.tar".to_string(),
                            sha256: "a".repeat(64),
                            min_runtime_api: 2,
                            max_runtime_api: 2,
                        },
                        RegistryVersionEntry {
                            version: "1.2.0".to_string(),
                            download_url: "https://example.com/hw-1.2.0.tar".to_string(),
                            sha256: "b".repeat(64),
                            min_runtime_api: 2,
                            max_runtime_api: 2,
                        },
                    ],
                },
                RegistryEntry {
                    id: "cron-suite".to_string(),
                    description: "Cron job management and scheduling".to_string(),
                    category: "scheduling".to_string(),
                    author: "agentzero".to_string(),
                    repository: "https://github.com/agentzero/plugins".to_string(),
                    latest: "0.3.0".to_string(),
                    versions: vec![RegistryVersionEntry {
                        version: "0.3.0".to_string(),
                        download_url: "https://example.com/cron-0.3.0.tar".to_string(),
                        sha256: "c".repeat(64),
                        min_runtime_api: 2,
                        max_runtime_api: 2,
                    }],
                },
            ],
        }
    }

    #[test]
    fn registry_search_by_id() {
        let index = sample_registry_index();
        let results = index.search("hardware");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "hardware-tools");
    }

    #[test]
    fn registry_search_by_description() {
        let index = sample_registry_index();
        let results = index.search("scheduling");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "cron-suite");
    }

    #[test]
    fn registry_search_case_insensitive() {
        let index = sample_registry_index();
        let results = index.search("CRON");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn registry_search_no_match() {
        let index = sample_registry_index();
        let results = index.search("nonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn registry_get_by_id() {
        let index = sample_registry_index();
        let entry = index.get("hardware-tools");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().latest, "1.2.0");
    }

    #[test]
    fn registry_get_missing_returns_none() {
        let index = sample_registry_index();
        assert!(index.get("missing").is_none());
    }

    #[test]
    fn registry_entry_latest_version() {
        let index = sample_registry_index();
        let entry = index.get("hardware-tools").unwrap();
        let latest = entry.latest_version().unwrap();
        assert_eq!(latest.version, "1.2.0");
        assert!(latest.download_url.contains("1.2.0"));
    }

    #[test]
    fn registry_entry_has_update() {
        let index = sample_registry_index();
        let entry = index.get("hardware-tools").unwrap();
        assert!(entry.has_update("1.0.0"));
        assert!(!entry.has_update("1.2.0"));
    }

    #[test]
    fn registry_cache_save_and_load() {
        let dir =
            std::env::temp_dir().join(format!("az-registry-cache-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);

        let index = sample_registry_index();
        index.save_cache(&dir).expect("save should succeed");

        let loaded = RegistryIndex::load_cached(&dir);
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.plugins.len(), 2);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn registry_cache_missing_returns_none() {
        let dir =
            std::env::temp_dir().join(format!("az-registry-cache-miss-{}", std::process::id()));
        assert!(RegistryIndex::load_cached(&dir).is_none());
    }

    #[test]
    fn load_registry_from_file_url() {
        let dir =
            std::env::temp_dir().join(format!("az-registry-file-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);

        let index = sample_registry_index();
        let index_path = dir.join("test-index.json");
        fs::write(&index_path, serde_json::to_string_pretty(&index).unwrap()).unwrap();

        let url = format!("file://{}", index_path.display());
        let loaded = load_registry_index(&dir, Some(&url)).expect("should load from file");
        assert_eq!(loaded.plugins.len(), 2);

        // Should now be cached
        let cached = RegistryIndex::load_cached(&dir);
        assert!(cached.is_some());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_registry_no_cache_no_url_fails() {
        let dir = std::env::temp_dir().join(format!("az-registry-no-cache-{}", std::process::id()));
        let err = load_registry_index(&dir, None).expect_err("should fail");
        assert!(err.to_string().contains("No registry cache"));
    }

    #[test]
    fn check_outdated_finds_updates() {
        let index = sample_registry_index();
        let mut state = PluginState::default();
        state.record_install("hardware-tools", "1.0.0", "registry");
        state.record_install("cron-suite", "0.3.0", "registry");

        let outdated = check_outdated(&state, &index);
        assert_eq!(outdated.len(), 1);
        assert_eq!(outdated[0].0, "hardware-tools");
        assert_eq!(outdated[0].1, "1.0.0"); // installed
        assert_eq!(outdated[0].2, "1.2.0"); // latest
    }

    #[test]
    fn check_outdated_none_when_up_to_date() {
        let index = sample_registry_index();
        let mut state = PluginState::default();
        state.record_install("hardware-tools", "1.2.0", "registry");

        let outdated = check_outdated(&state, &index);
        assert!(outdated.is_empty());
    }

    #[test]
    fn generate_registry_entry_round_trip() {
        let manifest = sample_manifest();
        let entry = generate_registry_entry(&RegistryEntryParams {
            manifest: &manifest,
            description: "A sample plugin",
            category: "general",
            author: "test-author",
            repository: "https://github.com/test/repo",
            download_url: "https://example.com/sample-1.0.0.tar",
            wasm_sha256: &"f".repeat(64),
        });
        assert_eq!(entry.id, "sample-plugin");
        assert_eq!(entry.latest, "1.0.0");
        assert_eq!(entry.versions.len(), 1);
        assert_eq!(
            entry.versions[0].download_url,
            "https://example.com/sample-1.0.0.tar"
        );
    }

    // ── Path traversal security tests ─────────────────────────────────

    /// Build a tar archive with `manifest.json` and an extra entry at an
    /// arbitrary path. Uses raw header manipulation to bypass the tar
    /// crate's safety checks (which reject `..` and absolute paths).
    fn build_tar_with_malicious_entry(entry_name: &str) -> Vec<u8> {
        let wasm_bytes =
            wat::parse_str(r#"(module (func (export "run") (result i32) i32.const 42))"#)
                .expect("wat should compile");
        let sha = super::sha256_hex(&wasm_bytes);
        let mut manifest = sample_manifest();
        manifest.wasm_sha256 = sha;
        let manifest_bytes =
            serde_json::to_vec_pretty(&manifest).expect("manifest should serialize");

        let file_path = std::env::temp_dir().join(format!(
            "az-tar-test-{}-{}",
            std::process::id(),
            entry_name.replace(['/', '.'], "_")
        ));
        {
            let file = fs::File::create(&file_path).expect("create tar");
            let mut builder = tar::Builder::new(file);

            let mut manifest_header = tar::Header::new_gnu();
            manifest_header.set_size(manifest_bytes.len() as u64);
            manifest_header.set_mode(0o644);
            manifest_header.set_cksum();
            builder
                .append_data(
                    &mut manifest_header,
                    "manifest.json",
                    Cursor::new(&manifest_bytes),
                )
                .expect("add manifest");

            // Write the malicious entry by setting path bytes directly in
            // the header, bypassing the tar crate's path validation.
            let mut header = tar::Header::new_gnu();
            header.set_size(wasm_bytes.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            // Set path bytes directly via the raw header.
            {
                let path_bytes = entry_name.as_bytes();
                let header_bytes = header.as_mut_bytes();
                // Name field is bytes 0..100 in a tar header.
                let len = path_bytes.len().min(100);
                header_bytes[..len].copy_from_slice(&path_bytes[..len]);
                // Zero-fill the rest.
                for b in &mut header_bytes[len..100] {
                    *b = 0;
                }
            }
            header.set_cksum();
            builder
                .append(&header, Cursor::new(&wasm_bytes))
                .expect("add malicious entry");

            builder.finish().expect("finish");
        }
        let bytes = fs::read(&file_path).expect("read tar");
        fs::remove_file(&file_path).ok();
        bytes
    }

    #[test]
    fn install_rejects_path_traversal_with_dotdot() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let package_path = tmp.path().join("evil.tar");
        let install_root = tmp.path().join("installed");

        let tar_bytes = build_tar_with_malicious_entry("../../etc/passwd");
        fs::write(&package_path, tar_bytes).expect("write tar");

        let err = install_packaged_plugin(&package_path, &install_root)
            .expect_err("path traversal should be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("path traversal"),
            "error should mention path traversal: {msg}"
        );
    }

    #[test]
    fn install_rejects_absolute_path_entry() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let package_path = tmp.path().join("evil-abs.tar");
        let install_root = tmp.path().join("installed");

        let tar_bytes = build_tar_with_malicious_entry("/etc/passwd");
        fs::write(&package_path, tar_bytes).expect("write tar");

        let err = install_packaged_plugin(&package_path, &install_root)
            .expect_err("absolute path should be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("path traversal"),
            "error should mention path traversal: {msg}"
        );
    }

    #[test]
    fn install_rejects_symlink_entry() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let package_path = tmp.path().join("evil-symlink.tar");
        let install_root = tmp.path().join("installed");

        let wasm_bytes =
            wat::parse_str(r#"(module (func (export "run") (result i32) i32.const 42))"#)
                .expect("wat should compile");
        let sha = super::sha256_hex(&wasm_bytes);
        let mut manifest = sample_manifest();
        manifest.wasm_sha256 = sha;
        let manifest_bytes =
            serde_json::to_vec_pretty(&manifest).expect("manifest should serialize");

        let file = fs::File::create(&package_path).expect("create tar");
        let mut builder = tar::Builder::new(file);

        let mut manifest_header = tar::Header::new_gnu();
        manifest_header.set_size(manifest_bytes.len() as u64);
        manifest_header.set_mode(0o644);
        manifest_header.set_cksum();
        builder
            .append_data(
                &mut manifest_header,
                "manifest.json",
                Cursor::new(&manifest_bytes),
            )
            .expect("add manifest");

        // Add a symlink entry
        let mut symlink_header = tar::Header::new_gnu();
        symlink_header.set_entry_type(tar::EntryType::Symlink);
        symlink_header.set_size(0);
        symlink_header.set_mode(0o777);
        symlink_header.set_cksum();
        builder
            .append_link(&mut symlink_header, "plugin.wasm", "/etc/passwd")
            .expect("add symlink");

        builder.finish().expect("finish");

        let err = install_packaged_plugin(&package_path, &install_root)
            .expect_err("symlink entry should be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("symlink"),
            "error should mention symlink: {msg}"
        );
    }

    // ── Semver version comparison tests ───────────────────────────────

    use super::version_ge;

    #[test]
    fn version_ge_semver_correct_ordering() {
        // These would fail with lexicographic comparison
        assert!(version_ge("10.0.0", "9.0.0"), "10.0.0 >= 9.0.0");
        assert!(version_ge("0.10.0", "0.2.0"), "0.10.0 >= 0.2.0");
        assert!(version_ge("1.0.0", "1.0.0"), "1.0.0 >= 1.0.0");
        assert!(!version_ge("0.2.0", "0.10.0"), "0.2.0 < 0.10.0");
        assert!(!version_ge("9.0.0", "10.0.0"), "9.0.0 < 10.0.0");
    }

    #[test]
    fn version_ge_falls_back_to_string_for_non_semver() {
        // Non-semver strings fall back to lexicographic comparison
        assert!(version_ge("beta", "alpha"));
        assert!(!version_ge("alpha", "beta"));
    }

    #[test]
    fn discover_plugins_picks_semver_latest_not_lexicographic() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let global = tmp.path().join("global");
        // Version "0.10.0" should beat "0.2.0" with semver, but would lose lexicographically
        write_test_plugin(
            &global.join("semver-test").join("0.2.0"),
            "semver-test",
            "0.2.0",
        );
        write_test_plugin(
            &global.join("semver-test").join("0.10.0"),
            "semver-test",
            "0.10.0",
        );

        let found = discover_plugins(Some(&global), None, None);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].manifest.version, "0.10.0",
            "should pick 0.10.0 over 0.2.0 with semver comparison"
        );
    }

    // ── File locking tests ────────────────────────────────────────────

    #[test]
    fn install_creates_lock_file() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let wasm_path = tmp.path().join("plugin.wasm");
        let package_path = tmp.path().join("sample-plugin.tar");
        let install_root = tmp.path().join("installed");

        let wasm_bytes =
            wat::parse_str(r#"(module (func (export "run") (result i32) i32.const 7))"#)
                .expect("wat should compile");
        fs::write(&wasm_path, wasm_bytes).expect("wasm file should be written");

        package_plugin(&wasm_path, sample_manifest(), &package_path)
            .expect("packaging should succeed");
        install_packaged_plugin(&package_path, &install_root).expect("install should succeed");

        // Lock file should have been created (and released after install)
        let lock_path = install_root.join(super::LOCK_FILE_NAME);
        assert!(lock_path.exists(), "lock file should exist after install");
    }

    #[test]
    fn remove_creates_lock_file() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let wasm_path = tmp.path().join("plugin.wasm");
        let package_path = tmp.path().join("sample-plugin.tar");
        let install_root = tmp.path().join("installed");

        let wasm_bytes =
            wat::parse_str(r#"(module (func (export "run") (result i32) i32.const 7))"#)
                .expect("wat should compile");
        fs::write(&wasm_path, wasm_bytes).expect("wasm file should be written");

        package_plugin(&wasm_path, sample_manifest(), &package_path)
            .expect("packaging should succeed");
        install_packaged_plugin(&package_path, &install_root).expect("install should succeed");

        remove_installed_plugin(&install_root, "sample-plugin", Some("1.0.0"))
            .expect("remove should succeed");

        let lock_path = install_root.join(super::LOCK_FILE_NAME);
        assert!(lock_path.exists(), "lock file should exist after remove");
    }
}
