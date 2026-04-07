//! GGUF model download and cache management for the builtin provider.
//!
//! Downloads models from HuggingFace Hub into `~/.agentzero/models/` and
//! shows a progress bar during the first download.

use std::path::PathBuf;

use anyhow::{Context, Result};
use hf_hub::api::sync::ApiBuilder;
use indicatif::{ProgressBar, ProgressStyle};
use tracing::{debug, info};

/// Default HuggingFace repo for the built-in coding model.
pub const DEFAULT_HF_REPO: &str = "Qwen/Qwen2.5-Coder-3B-Instruct-GGUF";

/// Default GGUF filename within the repo.
pub const DEFAULT_GGUF_FILE: &str = "qwen2.5-coder-3b-instruct-q4_k_m.gguf";

/// Default model identifier shown in CLI and logs.
pub const DEFAULT_BUILTIN_MODEL: &str = "qwen2.5-coder-3b";

/// Returns the models cache directory (`~/.agentzero/models/`).
pub fn models_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    let dir = home.join(".agentzero").join("models");
    Ok(dir)
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
}
