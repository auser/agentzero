//! Reusable path validator ensuring all paths stay within a root directory.
//!
//! Extracts the shared logic from `ToolExecutor::validate_path()` so that
//! both the session-layer tool executor and the brain plugin host callbacks
//! can enforce the same path-safety rules.

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors returned by [`PathValidator`] when a path fails validation.
#[derive(Debug, Error)]
pub enum PathError {
    /// The resolved path escapes the configured root directory.
    #[error("path outside root: {0}")]
    OutsideRoot(String),
    /// The path touches a sensitive location (e.g. `.ssh`, `.gnupg`).
    #[error("access to sensitive path denied: {0}")]
    SensitivePath(String),
    /// A write was attempted to a path whose final component is a symlink.
    #[error("symlink write blocked: {0}")]
    SymlinkBlocked(String),
    /// The path could not be resolved or is otherwise malformed.
    #[error("invalid path: {0}")]
    InvalidPath(String),
}

/// Paths containing any of these substrings (after canonicalization) are
/// rejected as sensitive.  Matches the blocklist in
/// `agentzero-session/src/tool_exec.rs`.
///
/// Note: The check uses substring matching, so `.agentzero` matches both
/// the `.agentzero/` directory and files like `.agentzero-brain.toml`.
/// Callers that need to access `.agentzero-*` files should use
/// [`PathValidator::with_sensitive`] to customize the blocklist.
const SENSITIVE: &[&str] = &[".ssh", ".gnupg", ".aws/credentials", ".env", ".agentzero"];

/// A validator that ensures every path stays within a canonicalized root.
pub struct PathValidator {
    root: PathBuf,
    sensitive: Vec<String>,
}

impl PathValidator {
    /// Create a new validator anchored at `root` using the default
    /// sensitive-path blocklist.
    ///
    /// The root is canonicalized immediately; returns an error if the
    /// directory does not exist or cannot be resolved.
    pub fn new(root: &Path) -> Result<Self, PathError> {
        Self::with_sensitive(root, SENSITIVE)
    }

    /// Create a new validator with a custom sensitive-path blocklist.
    ///
    /// Use this when the default blocklist (which includes `.agentzero`)
    /// is too broad — for example, the brain plugin needs to access
    /// `.agentzero-brain.toml` inside the vault root.
    pub fn with_sensitive(root: &Path, sensitive: &[&str]) -> Result<Self, PathError> {
        let root = std::fs::canonicalize(root)
            .map_err(|e| PathError::InvalidPath(format!("{}: {e}", root.display())))?;
        Ok(Self {
            root,
            sensitive: sensitive.iter().map(|s| s.to_string()).collect(),
        })
    }

    /// Validate a path for **reading**.
    ///
    /// Canonicalizes the path, checks that it starts with `self.root`, and
    /// rejects paths matching the sensitive blocklist.
    pub fn validate_read(&self, path: &str) -> Result<PathBuf, PathError> {
        let resolved = self.resolve(path)?;
        self.check_bounds(&resolved, path)?;
        self.check_sensitive(&resolved)?;
        Ok(resolved)
    }

    /// Validate a path for **writing** (overwrite of existing file).
    ///
    /// Same checks as [`validate_read`](Self::validate_read), plus blocks
    /// writes when the final path component is a symlink (TOCTOU mitigation).
    pub fn validate_write(&self, path: &str) -> Result<PathBuf, PathError> {
        let resolved = self.resolve(path)?;
        self.check_bounds(&resolved, path)?;
        self.check_sensitive(&resolved)?;
        self.check_symlink(path)?;
        Ok(resolved)
    }

    /// Validate a path for **creation** (the file/directory may not exist yet).
    ///
    /// Walks up from the given path to the first existing ancestor,
    /// canonicalizes that ancestor, appends the remaining components, and
    /// runs the standard bounds + sensitivity checks.
    pub fn validate_create(&self, path: &str) -> Result<PathBuf, PathError> {
        let full = self.root.join(path);

        // If it already exists, delegate to validate_write
        if full.exists() {
            return self.validate_write(&full.to_string_lossy());
        }

        // Walk up to find the first existing ancestor
        let mut suffix_components: Vec<&std::ffi::OsStr> = Vec::new();

        if let Some(name) = full.file_name() {
            suffix_components.push(name);
        }

        let mut ancestor = full.parent();
        loop {
            match ancestor {
                Some(a) if a.exists() => {
                    let canonical_ancestor = std::fs::canonicalize(a)
                        .map_err(|e| PathError::InvalidPath(format!("{}: {e}", a.display())))?;
                    let mut canonical = canonical_ancestor;
                    for component in suffix_components.iter().rev() {
                        canonical = canonical.join(component);
                    }
                    self.check_bounds(&canonical, path)?;
                    self.check_sensitive(&canonical)?;
                    return Ok(canonical);
                }
                Some(a) => {
                    if let Some(name) = a.file_name() {
                        suffix_components.push(name);
                    }
                    ancestor = a.parent();
                }
                None => {
                    return Err(PathError::InvalidPath(format!(
                        "{path}: no existing ancestor directory"
                    )));
                }
            }
        }
    }

    // ---- internal helpers ----

    /// Resolve `path` to an absolute, canonical path.
    ///
    /// If `path` is relative it is joined to `self.root` before
    /// canonicalization.
    fn resolve(&self, path: &str) -> Result<PathBuf, PathError> {
        let p = Path::new(path);
        let target = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        };
        std::fs::canonicalize(&target).map_err(|e| PathError::InvalidPath(format!("{path}: {e}")))
    }

    /// Ensure `canonical` starts with `self.root`.
    fn check_bounds(&self, canonical: &Path, original: &str) -> Result<(), PathError> {
        if !canonical.starts_with(&self.root) {
            return Err(PathError::OutsideRoot(original.to_string()));
        }
        Ok(())
    }

    /// Reject paths that contain sensitive substrings.
    fn check_sensitive(&self, canonical: &Path) -> Result<(), PathError> {
        let path_str = canonical.to_string_lossy();
        for s in &self.sensitive {
            if path_str.contains(s.as_str()) {
                return Err(PathError::SensitivePath(s.clone()));
            }
        }
        Ok(())
    }

    /// Block writes when the final component is a symlink.
    fn check_symlink(&self, path: &str) -> Result<(), PathError> {
        let p = Path::new(path);
        let target = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        };
        let meta = std::fs::symlink_metadata(&target);
        if let Ok(m) = meta {
            if m.file_type().is_symlink() {
                return Err(PathError::SymlinkBlocked(path.to_string()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agentzero-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    fn cleanup(dir: &Path) {
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn blocks_traversal_outside_root() {
        let dir = temp_dir("path-validator");
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let v = PathValidator::new(&dir).expect("new validator");
        assert!(
            matches!(
                v.validate_read("/etc/passwd"),
                Err(PathError::OutsideRoot(_))
            ),
            "expected OutsideRoot error"
        );
        cleanup(&dir);
    }

    #[test]
    fn blocks_sensitive_ssh() {
        let dir = temp_dir("path-validator-ssh");
        std::fs::create_dir_all(dir.join(".ssh")).expect("create .ssh");
        std::fs::write(dir.join(".ssh/id_rsa"), "key").expect("write id_rsa");
        let v = PathValidator::new(&dir).expect("new validator");
        assert!(
            matches!(
                v.validate_read(".ssh/id_rsa"),
                Err(PathError::SensitivePath(_))
            ),
            "expected SensitivePath error"
        );
        cleanup(&dir);
    }

    #[test]
    fn allows_valid_path_inside_root() {
        let dir = temp_dir("path-validator-valid");
        std::fs::create_dir_all(&dir).expect("create temp dir");
        std::fs::write(dir.join("test.md"), "hello").expect("write test file");
        let v = PathValidator::new(&dir).expect("new validator");
        assert!(v.validate_read("test.md").is_ok());
        cleanup(&dir);
    }

    #[test]
    fn create_validates_parent_chain() {
        let dir = temp_dir("path-validator-create");
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let v = PathValidator::new(&dir).expect("new validator");
        // Creating a nested path that doesn't exist yet should succeed
        assert!(v.validate_create("wiki/daily/2025-01-01.md").is_ok());
        cleanup(&dir);
    }

    #[test]
    fn blocks_dotdot_traversal() {
        let dir = temp_dir("path-validator-dotdot");
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let v = PathValidator::new(&dir).expect("new validator");
        assert!(v.validate_read("../../../etc/passwd").is_err());
        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn validate_write_blocks_symlink() {
        let dir = temp_dir("path-validator-symlink");
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let target = dir.join("real.txt");
        std::fs::write(&target, "real content").expect("write target");
        let link = dir.join("link.txt");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");
        let v = PathValidator::new(&dir).expect("new validator");
        assert!(
            matches!(
                v.validate_write("link.txt"),
                Err(PathError::SymlinkBlocked(_))
            ),
            "expected SymlinkBlocked error"
        );
        cleanup(&dir);
    }

    #[test]
    fn blocks_sensitive_env() {
        let dir = temp_dir("path-validator-env");
        std::fs::create_dir_all(&dir).expect("create temp dir");
        std::fs::write(dir.join(".env"), "SECRET=x").expect("write .env");
        let v = PathValidator::new(&dir).expect("new validator");
        assert!(
            matches!(v.validate_read(".env"), Err(PathError::SensitivePath(_))),
            "expected SensitivePath error"
        );
        cleanup(&dir);
    }
}
