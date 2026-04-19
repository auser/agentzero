//! GGUF model download and cache management for the builtin provider.
//!
//! Downloads models from HuggingFace Hub into `~/.agentzero/models/` and
//! shows a progress bar during the first download.

use std::path::PathBuf;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use hf_hub::api::sync::ApiBuilder;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use tracing::{debug, info};

/// Default HuggingFace repo for the built-in coding model.
pub const DEFAULT_HF_REPO: &str = "Qwen/Qwen2.5-Coder-3B-Instruct-GGUF";

/// Default GGUF filename within the repo.
pub const DEFAULT_GGUF_FILE: &str = "qwen2.5-coder-3b-instruct-q4_k_m.gguf";

/// Default model identifier shown in CLI and logs.
pub const DEFAULT_BUILTIN_MODEL: &str = "qwen2.5-coder-3b";

/// Embedded model catalog JSON (shared with `models.rs`).
const CATALOG_JSON: &str = include_str!("../data/model_catalog.json");

/// Maps a short model ID to a HuggingFace repo and GGUF filename.
#[derive(Debug, Deserialize)]
pub struct GgufModelEntry {
    pub id: String,
    pub hf_repo: String,
    pub gguf_file: String,
}

#[derive(Deserialize)]
struct CatalogFragment {
    gguf_registry: Vec<GgufModelEntry>,
}

static GGUF_REGISTRY: LazyLock<Vec<GgufModelEntry>> = LazyLock::new(|| {
    let catalog: CatalogFragment =
        serde_json::from_str(CATALOG_JSON).expect("embedded model_catalog.json is valid");
    catalog.gguf_registry
});

/// Look up a GGUF model by its short ID.
pub fn resolve_model(id: &str) -> Option<&'static GgufModelEntry> {
    GGUF_REGISTRY.iter().find(|e| e.id == id)
}

/// Return the full GGUF registry (lazily parsed from embedded JSON).
pub fn gguf_registry() -> &'static [GgufModelEntry] {
    &GGUF_REGISTRY
}

/// Returns the models cache directory (`~/.agentzero/models/`).
pub fn models_dir() -> Result<PathBuf> {
    crate::bundle::models_dir()
}

/// Ensure a GGUF model file is available locally.
///
/// If the file is already cached, returns the path immediately.
/// Otherwise downloads from HuggingFace Hub with a progress bar.
pub fn ensure_model(repo: &str, filename: &str) -> Result<PathBuf> {
    let cache_dir = models_dir()?;
    let cached_path = cache_dir.join(filename);

    if cached_path.exists() {
        debug!(path = %cached_path.display(), "model already cached");
        return Ok(cached_path);
    }

    eprintln!(
        "\x1b[1;33m⟐ Downloading model {filename}\x1b[0m from {repo} (~2 GB, first run only)"
    );
    info!(repo, filename, "downloading model from HuggingFace Hub");

    let pb = ProgressBar::new(0);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})",
        )
        .expect("hardcoded progress bar template is known-valid")
        .progress_chars("=>-"),
    );
    pb.set_message(format!("Downloading {filename}"));

    // Use hf-hub's built-in caching but copy to our own models dir.
    let api = ApiBuilder::new()
        .with_progress(true)
        .build()
        .context("failed to create HuggingFace API client")?;

    let repo_handle = api.model(repo.to_string());
    let downloaded = repo_handle
        .get(filename)
        .with_context(|| format!("failed to download {filename} from {repo}"))?;

    // hf-hub caches files in its own directory structure. We symlink or copy
    // to our models dir for a cleaner path.
    std::fs::create_dir_all(&cache_dir)
        .with_context(|| format!("failed to create models dir {}", cache_dir.display()))?;

    // Prefer symlink on Unix, copy on Windows.
    #[cfg(unix)]
    {
        if !cached_path.exists() {
            std::os::unix::fs::symlink(&downloaded, &cached_path).with_context(|| {
                format!(
                    "failed to symlink {} -> {}",
                    downloaded.display(),
                    cached_path.display()
                )
            })?;
        }
    }
    #[cfg(not(unix))]
    {
        if !cached_path.exists() {
            std::fs::copy(&downloaded, &cached_path).with_context(|| {
                format!(
                    "failed to copy {} -> {}",
                    downloaded.display(),
                    cached_path.display()
                )
            })?;
        }
    }

    pb.finish_with_message(format!("Downloaded {filename}"));
    info!(path = %cached_path.display(), "model ready");
    Ok(cached_path)
}

/// Ensure the default built-in model is available.
pub fn ensure_default_model() -> Result<PathBuf> {
    ensure_model(DEFAULT_HF_REPO, DEFAULT_GGUF_FILE)
}

/// Load a model from a `.azb` bundle file.
///
/// Extracts the bundle into `~/.agentzero/models/{model_id}/{version}/` and
/// returns the path to the first file with role `"model"`. If no model-role
/// file is found, returns the first file in the bundle.
#[cfg(feature = "bundles")]
pub fn load_from_bundle(bundle_path: &std::path::Path) -> Result<PathBuf> {
    let bundle = crate::bundle::load_bundle(bundle_path)
        .with_context(|| format!("failed to load bundle: {}", bundle_path.display()))?;

    let install_dir = models_dir()?;
    let extracted =
        crate::bundle::extract_bundle(&bundle, &install_dir).context("failed to extract bundle")?;

    // Find the model file: prefer role="model", fall back to first file.
    let model_file = bundle
        .manifest
        .files
        .iter()
        .find(|f| f.role == "model")
        .or_else(|| bundle.manifest.files.first())
        .map(|f| extracted.join(&f.path))
        .ok_or_else(|| anyhow::anyhow!("bundle contains no files"))?;

    info!(
        model_id = bundle.manifest.model_id,
        path = %model_file.display(),
        "model loaded from bundle"
    );

    Ok(model_file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_dir_is_under_home() {
        let dir = models_dir().expect("should resolve models dir");
        assert!(dir.ends_with(".agentzero/models"));
    }

    #[test]
    fn default_constants_are_consistent() {
        assert!(!DEFAULT_HF_REPO.is_empty());
        assert!(DEFAULT_GGUF_FILE.ends_with(".gguf"));
        assert!(!DEFAULT_BUILTIN_MODEL.is_empty());
    }

    #[test]
    fn resolve_model_finds_known_model() {
        let entry = resolve_model("qwen2.5-coder-7b").expect("should find qwen2.5-coder-7b");
        assert_eq!(entry.hf_repo, "Qwen/Qwen2.5-Coder-7B-Instruct-GGUF");
        assert!(entry.gguf_file.ends_with(".gguf"));
    }

    #[test]
    fn resolve_model_returns_none_for_unknown() {
        assert!(resolve_model("nonexistent-model").is_none());
    }

    #[test]
    fn resolve_model_default_matches_constants() {
        let entry =
            resolve_model(DEFAULT_BUILTIN_MODEL).expect("default model should be in registry");
        assert_eq!(entry.hf_repo, DEFAULT_HF_REPO);
        assert_eq!(entry.gguf_file, DEFAULT_GGUF_FILE);
    }

    #[test]
    fn registry_has_no_duplicate_ids() {
        let registry = gguf_registry();
        let mut seen = std::collections::HashSet::new();
        for entry in registry {
            assert!(
                seen.insert(&entry.id),
                "duplicate registry entry: {}",
                entry.id
            );
        }
    }

    #[test]
    fn registry_parses_from_embedded_json() {
        let registry = gguf_registry();
        assert!(!registry.is_empty(), "registry should not be empty");
    }
}
