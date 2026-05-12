use agentzero_core::path_validator::{PathError, PathValidator};

use crate::BrainFs;

/// Sensitive paths for the brain plugin.
///
/// Excludes `.agentzero` from the default blocklist because the brain vault
/// legitimately contains `.agentzero-brain.toml` as its config file.
const BRAIN_SENSITIVE: &[&str] = &[".ssh", ".gnupg", ".aws/credentials", ".env"];

/// Real filesystem implementation of `BrainFs` with path validation.
///
/// All I/O operations are validated against a [`PathValidator`] anchored
/// at the vault root, preventing path traversal, access to sensitive
/// locations, and symlink-based TOCTOU attacks.
pub struct RealBrainFs {
    validator: PathValidator,
}

impl RealBrainFs {
    /// Create a new `RealBrainFs` anchored at the given root directory.
    ///
    /// The root is canonicalized immediately; returns an error if the
    /// directory does not exist or cannot be resolved.
    pub fn new(root: &std::path::Path) -> Result<Self, PathError> {
        Ok(Self {
            validator: PathValidator::with_sensitive(root, BRAIN_SENSITIVE)?,
        })
    }
}

impl BrainFs for RealBrainFs {
    fn read_file(&self, path: &str) -> Result<String, String> {
        let canonical = self
            .validator
            .validate_read(path)
            .map_err(|e| e.to_string())?;
        std::fs::read_to_string(canonical).map_err(|e| format!("read {path}: {e}"))
    }

    fn write_file(&self, path: &str, content: &str) -> Result<bool, String> {
        // Use validate_write for existing files (includes symlink check),
        // fall back to validate_create for new files.
        let canonical = match self.validator.validate_write(path) {
            Ok(c) => c,
            Err(PathError::InvalidPath(_)) => self
                .validator
                .validate_create(path)
                .map_err(|e| e.to_string())?,
            Err(e) => return Err(e.to_string()),
        };
        std::fs::write(canonical, content).map_err(|e| format!("write {path}: {e}"))?;
        Ok(true)
    }

    fn append_file(&self, path: &str, content: &str) -> Result<bool, String> {
        let canonical = self
            .validator
            .validate_create(path)
            .map_err(|e| e.to_string())?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(canonical)
            .map_err(|e| format!("append {path}: {e}"))?;
        file.write_all(content.as_bytes())
            .map_err(|e| format!("append write {path}: {e}"))?;
        Ok(true)
    }

    fn list_dir(&self, path: &str) -> Result<Vec<String>, String> {
        let canonical = self
            .validator
            .validate_read(path)
            .map_err(|e| e.to_string())?;
        let entries = std::fs::read_dir(canonical).map_err(|e| format!("list_dir {path}: {e}"))?;
        let mut result = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| format!("read entry: {e}"))?;
            if let Some(name) = entry.file_name().to_str() {
                result.push(name.to_string());
            }
        }
        result.sort();
        Ok(result)
    }

    fn create_dir(&self, path: &str) -> Result<bool, String> {
        let canonical = self
            .validator
            .validate_create(path)
            .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(canonical).map_err(|e| format!("create_dir {path}: {e}"))?;
        Ok(true)
    }

    fn file_exists(&self, path: &str) -> Result<bool, String> {
        // Try validate_read first (works for existing paths).
        // If that fails due to InvalidPath (non-existent), fall back to
        // validate_create which handles non-existent paths, then return false.
        // Security errors (OutsideRoot, SensitivePath) propagate as errors.
        match self.validator.validate_read(path) {
            Ok(canonical) => Ok(canonical.exists()),
            Err(PathError::InvalidPath(_)) => {
                // Path doesn't exist — validate bounds via create check
                let _ = self
                    .validator
                    .validate_create(path)
                    .map_err(|e| e.to_string())?;
                Ok(false)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn now(&self) -> String {
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
    }
}
