use agentzero_storage::EncryptedJsonStore;
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateState {
    pub current_version: String,
    pub last_checked_epoch_secs: Option<u64>,
    pub last_target_version: Option<String>,
    pub last_applied_epoch_secs: Option<u64>,
    pub previous_versions: Vec<String>,
    /// Build variant: "default" or "minimal". Defaults to "default" for
    /// backwards compatibility with state files that predate this field.
    #[serde(default = "default_variant")]
    pub variant: String,
}

fn default_variant() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub up_to_date: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApplyUpdateResult {
    pub from_version: String,
    pub to_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollbackUpdateResult {
    pub from_version: String,
    pub to_version: String,
}

pub fn load_state(
    state_path: impl AsRef<Path>,
    current_version: &str,
) -> anyhow::Result<UpdateState> {
    let store = state_store(state_path.as_ref())?;
    let mut state = store
        .load_optional::<UpdateState>()
        .with_context(|| format!("failed to parse update state at {}", store.path().display()))?
        .unwrap_or_else(|| UpdateState {
            current_version: current_version.to_string(),
            last_checked_epoch_secs: None,
            last_target_version: None,
            last_applied_epoch_secs: None,
            previous_versions: vec![],
            variant: default_variant(),
        });
    if state.current_version.trim().is_empty() {
        state.current_version = current_version.to_string();
    }
    Ok(state)
}

pub fn save_state(state_path: impl AsRef<Path>, state: &UpdateState) -> anyhow::Result<()> {
    let store = state_store(state_path.as_ref())?;
    store
        .save(state)
        .with_context(|| format!("failed to write update state at {}", store.path().display()))
}

pub fn check_for_updates(
    state_path: impl AsRef<Path>,
    current_version: &str,
    latest_version_override: Option<&str>,
) -> anyhow::Result<UpdateCheckResult> {
    let mut state = load_state(state_path.as_ref(), current_version)?;
    let latest = latest_version_override
        .map(str::to_string)
        .unwrap_or_else(|| current_version.to_string());
    state.last_checked_epoch_secs = Some(now_epoch_secs());
    save_state(state_path, &state)?;

    Ok(UpdateCheckResult {
        current_version: current_version.to_string(),
        latest_version: latest.clone(),
        up_to_date: latest == current_version,
    })
}

pub fn apply_update(
    state_path: impl AsRef<Path>,
    current_version: &str,
    target_version: &str,
) -> anyhow::Result<ApplyUpdateResult> {
    if target_version.trim().is_empty() {
        return Err(anyhow!("target version cannot be empty"));
    }

    let mut state = load_state(state_path.as_ref(), current_version)?;
    if state.current_version == target_version {
        return Err(anyhow!(
            "target version already applied: {}",
            target_version
        ));
    }

    state.previous_versions.push(state.current_version.clone());
    state.current_version = target_version.to_string();
    state.last_target_version = Some(target_version.to_string());
    state.last_applied_epoch_secs = Some(now_epoch_secs());
    save_state(state_path, &state)?;

    Ok(ApplyUpdateResult {
        from_version: state
            .previous_versions
            .last()
            .cloned()
            .unwrap_or_else(|| current_version.to_string()),
        to_version: target_version.to_string(),
    })
}

pub fn rollback_update(
    state_path: impl AsRef<Path>,
    current_version: &str,
) -> anyhow::Result<RollbackUpdateResult> {
    let mut state = load_state(state_path.as_ref(), current_version)?;
    let Some(previous) = state.previous_versions.pop() else {
        return Err(anyhow!("no previous version available for rollback"));
    };

    let from = state.current_version.clone();
    state.current_version = previous.clone();
    state.last_target_version = Some(previous.clone());
    state.last_applied_epoch_secs = Some(now_epoch_secs());
    save_state(state_path, &state)?;

    Ok(RollbackUpdateResult {
        from_version: from,
        to_version: previous,
    })
}

/// Fetch the latest published version from the GitHub releases API.
///
/// Strips a leading `v` from the tag name so the returned string is a bare
/// semver like `"0.2.0"`.  Pass `github_token` to avoid anonymous rate-limits.
pub async fn fetch_latest_version(github_token: Option<&str>) -> anyhow::Result<String> {
    let client = build_client(github_token)?;
    let body: serde_json::Value = client
        .get("https://api.github.com/repos/auser/agentzero/releases/latest")
        .send()
        .await
        .context("failed to reach GitHub releases API")?
        .error_for_status()
        .context("GitHub releases API returned an error status")?
        .json()
        .await
        .context("failed to parse GitHub releases API response")?;

    let tag = body["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow!("GitHub API response missing 'tag_name' field"))?;

    Ok(tag.trim_start_matches('v').to_string())
}

/// Download the release artifact for the current platform, verify its SHA256
/// checksum, back up the running binary, and atomically replace it.
///
/// After the binary is replaced this process calls the existing
/// [`apply_update`] to record the version transition in the encrypted state
/// file so that [`rollback_update`] / [`restore_backup`] can undo it.
pub async fn download_and_install(
    state_path: impl AsRef<Path>,
    current_version: &str,
    target_version: &str,
    github_token: Option<&str>,
) -> anyhow::Result<ApplyUpdateResult> {
    if target_version.trim().is_empty() {
        return Err(anyhow!("target version cannot be empty"));
    }

    let state = load_state(state_path.as_ref(), current_version)?;
    let variant_suffix = if state.variant == "minimal" {
        "-minimal"
    } else {
        ""
    };
    let (platform, arch) = current_target_name()?;
    let ext = if platform == "windows" { ".exe" } else { "" };
    let artifact_name =
        format!("agentzero-v{target_version}-{platform}-{arch}{variant_suffix}{ext}");

    let client = build_client(github_token)?;

    // --- download and parse SHA256SUMS ---
    let sums_url = format!(
        "https://github.com/auser/agentzero/releases/download/v{target_version}/SHA256SUMS"
    );
    let sums_text = client
        .get(&sums_url)
        .send()
        .await
        .context("failed to download SHA256SUMS")?
        .error_for_status()
        .context("SHA256SUMS download returned an error status")?
        .text()
        .await
        .context("failed to read SHA256SUMS body")?;

    let expected_hash = expected_checksum(&sums_text, &artifact_name)?;

    // --- download artifact and verify checksum ---
    let artifact_url = format!(
        "https://github.com/auser/agentzero/releases/download/v{target_version}/{artifact_name}"
    );
    let bytes = download_verified(&client, &artifact_url, &expected_hash).await?;

    // --- locate running binary ---
    let exe_path =
        std::env::current_exe().context("failed to determine current executable path")?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| anyhow!("executable has no parent directory"))?;
    let exe_stem = exe_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    // --- back up current binary ---
    let backup_path = exe_dir.join(format!("{exe_stem}.backup"));
    tokio::fs::copy(&exe_path, &backup_path)
        .await
        .with_context(|| format!("failed to back up binary to {}", backup_path.display()))?;

    // --- write new binary to a temp file in the same directory ---
    // Must be on the same filesystem as the target for atomic rename.
    let tmp_path = exe_dir.join(format!("{exe_stem}.tmp"));
    tokio::fs::write(&tmp_path, &bytes)
        .await
        .with_context(|| format!("failed to write new binary to {}", tmp_path.display()))?;

    // --- make executable on Unix ---
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&tmp_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&tmp_path, perms)
            .context("failed to set executable permissions on new binary")?;
    }

    // --- atomic replace ---
    tokio::fs::rename(&tmp_path, &exe_path)
        .await
        .with_context(|| format!("failed to replace binary at {}", exe_path.display()))?;

    // Record the version transition directly.  We do NOT call apply_update()
    // here because the state's current_version might already equal
    // target_version (e.g. if a previous state-only apply ran without actually
    // downloading the binary), which would cause a spurious "already applied"
    // error after a successful binary replacement.
    let from_version = state.current_version.clone();
    let mut updated = state;
    if updated.current_version != target_version {
        updated
            .previous_versions
            .push(updated.current_version.clone());
    }
    updated.current_version = target_version.to_string();
    updated.last_target_version = Some(target_version.to_string());
    updated.last_applied_epoch_secs = Some(now_epoch_secs());
    save_state(state_path, &updated)?;

    Ok(ApplyUpdateResult {
        from_version,
        to_version: target_version.to_string(),
    })
}

/// Restore the `.backup` binary created by [`download_and_install`] and
/// update the version state accordingly.
///
/// Returns an error if no backup file is found.
pub async fn restore_backup(
    state_path: impl AsRef<Path>,
    current_version: &str,
) -> anyhow::Result<RollbackUpdateResult> {
    let exe_path =
        std::env::current_exe().context("failed to determine current executable path")?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| anyhow!("executable has no parent directory"))?;
    let exe_stem = exe_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let backup_path = exe_dir.join(format!("{exe_stem}.backup"));

    if !backup_path.exists() {
        return Err(anyhow!(
            "no backup binary found at {}; cannot restore",
            backup_path.display()
        ));
    }

    tokio::fs::rename(&backup_path, &exe_path)
        .await
        .with_context(|| format!("failed to restore backup from {}", backup_path.display()))?;

    rollback_update(state_path, current_version)
}

// ── internal helpers ──────────────────────────────────────────────────────────

fn build_client(github_token: Option<&str>) -> anyhow::Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static("agentzero-updater"),
    );
    if let Some(token) = github_token {
        let value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .context("invalid GitHub token")?;
        headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .context("failed to build HTTP client")
}

/// Map the current OS and CPU architecture to the platform/arch labels used in
/// release artifact names (e.g. `"macos"`, `"aarch64"`).
fn current_target_name() -> anyhow::Result<(&'static str, &'static str)> {
    let platform = match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        "windows" => "windows",
        other => return Err(anyhow!("unsupported platform: {}", other)),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "arm" => "armv7",
        other => return Err(anyhow!("unsupported CPU architecture: {}", other)),
    };
    Ok((platform, arch))
}

/// Parse a `SHA256SUMS` file and return the hex digest for `artifact_name`.
///
/// Supports both `sha256sum` format (`<hash>  <name>`) and BSD format
/// (`<hash> *<name>`).
fn expected_checksum(sums: &str, artifact_name: &str) -> anyhow::Result<String> {
    for line in sums.lines() {
        if let Some((hash, name)) = line.split_once("  ") {
            if name.trim() == artifact_name {
                return Ok(hash.trim().to_string());
            }
        } else if let Some((hash, name)) = line.split_once(" *") {
            if name.trim() == artifact_name {
                return Ok(hash.trim().to_string());
            }
        }
    }
    Err(anyhow!(
        "artifact '{}' not found in SHA256SUMS",
        artifact_name
    ))
}

/// Download `url`, compute its SHA-256, and return the bytes only if the
/// digest matches `expected_hex`.
async fn download_verified(
    client: &reqwest::Client,
    url: &str,
    expected_hex: &str,
) -> anyhow::Result<Vec<u8>> {
    let bytes = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("download of {url} returned an error status"))?
        .bytes()
        .await
        .with_context(|| format!("failed to read body of {url}"))?;

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual = format!("{:x}", hasher.finalize());

    if actual != expected_hex {
        return Err(anyhow!(
            "SHA256 mismatch for {url}: expected {expected_hex}, got {actual}"
        ));
    }

    Ok(bytes.to_vec())
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_secs()
}

fn state_store(state_path: &Path) -> anyhow::Result<EncryptedJsonStore> {
    let parent = state_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = state_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("invalid update state path: {}", state_path.display()))?;
    EncryptedJsonStore::in_config_dir(parent, file_name)
}

#[cfg(test)]
mod tests {
    use super::{apply_update, check_for_updates, load_state, rollback_update};
    use std::fs;

    #[test]
    fn update_apply_and_rollback_success_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let state_path = tmp.path().join("state.json");

        let check =
            check_for_updates(&state_path, "0.1.0", Some("0.2.0")).expect("check should succeed");
        assert!(!check.up_to_date);

        let applied = apply_update(&state_path, "0.1.0", "0.2.0").expect("apply should succeed");
        assert_eq!(applied.from_version, "0.1.0");
        assert_eq!(applied.to_version, "0.2.0");

        let rolled = rollback_update(&state_path, "0.2.0").expect("rollback should succeed");
        assert_eq!(rolled.from_version, "0.2.0");
        assert_eq!(rolled.to_version, "0.1.0");

        let state = load_state(&state_path, "0.1.0").expect("state should load");
        assert_eq!(state.current_version, "0.1.0");
    }

    #[test]
    fn rollback_without_history_fails_negative_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let state_path = tmp.path().join("state.json");
        let err = rollback_update(&state_path, "0.1.0")
            .expect_err("rollback without history should fail");
        assert!(err.to_string().contains("no previous version"));
    }

    #[test]
    fn update_state_is_encrypted_at_rest_success_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let state_path = tmp.path().join("state.json");

        check_for_updates(&state_path, "0.1.0", Some("0.2.0")).expect("check should create state");
        apply_update(&state_path, "0.1.0", "0.2.0").expect("apply should update state");

        let on_disk = fs::read_to_string(&state_path).expect("state file should be readable");
        assert!(!on_disk.contains("0.2.0"));
        assert!(!on_disk.contains("current_version"));
    }
}
