//! AgentZero Brain: personal LLM wiki plugin.
//!
//! Provides commands for managing a personal knowledge vault:
//! - `brain init` — initialize vault structure
//! - `brain today` — open/create today's daily note
//! - `brain capture` — append a thought to today's note
//! - `brain query` — search the vault

pub mod config;
pub mod daily;
pub mod error;
pub mod fs;
pub mod init;
pub mod query;

pub use config::BrainConfig;
pub use daily::{brain_capture, brain_today};
pub use error::BrainError;
pub use fs::RealBrainFs;
pub use init::{brain_init, InitOptions, InitResult};
pub use query::{brain_query, format_results, QueryMatch, QueryOptions};

/// Validate a path is safe (no traversal, no null bytes).
pub fn validate_path(path: &str) -> Result<(), BrainError> {
    if path.contains("..") {
        return Err(BrainError::PathTraversal(path.to_string()));
    }
    if path.contains('\0') {
        return Err(BrainError::PathTraversal(format!(
            "null byte in path: {path}"
        )));
    }
    Ok(())
}

/// Check whether a write path falls under the raw directory.
/// Returns an error if raw is immutable and the path is under raw_dir.
pub fn check_raw_immutable(
    root: &str,
    path: &str,
    config: &BrainConfig,
) -> Result<(), BrainError> {
    if !config.safety.raw_is_immutable {
        return Ok(());
    }
    let raw_prefix = format!("{root}/{}/", config.vault.raw_dir);
    let raw_exact = format!("{root}/{}", config.vault.raw_dir);
    if path.starts_with(&raw_prefix) || path == raw_exact {
        return Err(BrainError::RawImmutable(path.to_string()));
    }
    Ok(())
}

/// Trait abstracting filesystem operations for testability and future WASM extraction.
///
/// Partially mirrors the `WasmHostCallbacks` pattern from `agentzero-sandbox`
/// (filesystem subset only, without `log` and `get_secret`), enabling the
/// brain logic to compile to WASM with a thin adapter when the component
/// model is adopted.
pub trait BrainFs {
    /// Read the entire contents of a file as a UTF-8 string.
    fn read_file(&self, path: &str) -> Result<String, String>;

    /// Write content to a file, creating it if necessary and overwriting if it exists.
    fn write_file(&self, path: &str, content: &str) -> Result<bool, String>;

    /// Append content to a file, creating it if necessary.
    fn append_file(&self, path: &str, content: &str) -> Result<bool, String>;

    /// List entries in a directory, returning entry names (not full paths).
    fn list_dir(&self, path: &str) -> Result<Vec<String>, String>;

    /// Create a directory and all parent directories.
    fn create_dir(&self, path: &str) -> Result<bool, String>;

    /// Check if a file or directory exists.
    fn file_exists(&self, path: &str) -> Result<bool, String>;

    /// Return the current local time as an ISO 8601 string.
    fn now(&self) -> String;
}

/// Load a `BrainConfig` from a vault root directory.
/// Returns defaults if the config file doesn't exist.
pub fn load_config(fs: &dyn BrainFs, root: &str) -> Result<BrainConfig, BrainError> {
    let config_path = format!("{root}/.agentzero-brain.toml");
    match fs.read_file(&config_path) {
        Ok(content) => BrainConfig::from_toml(&content),
        Err(_) => Ok(BrainConfig::default()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};

    /// In-memory filesystem for testing.
    pub struct TestFs {
        files: RefCell<BTreeMap<String, String>>,
        dirs: RefCell<BTreeSet<String>>,
    }

    impl TestFs {
        pub fn new() -> Self {
            Self {
                files: RefCell::new(BTreeMap::new()),
                dirs: RefCell::new(BTreeSet::new()),
            }
        }

        pub fn files(&self) -> BTreeMap<String, String> {
            self.files.borrow().clone()
        }

        pub fn dirs(&self) -> BTreeSet<String> {
            self.dirs.borrow().clone()
        }

        pub fn set_file(&self, path: &str, content: &str) {
            self.files
                .borrow_mut()
                .insert(path.to_string(), content.to_string());
        }
    }

    impl BrainFs for TestFs {
        fn read_file(&self, path: &str) -> Result<String, String> {
            self.files
                .borrow()
                .get(path)
                .cloned()
                .ok_or_else(|| format!("not found: {path}"))
        }

        fn write_file(&self, path: &str, content: &str) -> Result<bool, String> {
            self.files
                .borrow_mut()
                .insert(path.to_string(), content.to_string());
            Ok(true)
        }

        fn append_file(&self, path: &str, content: &str) -> Result<bool, String> {
            let mut files = self.files.borrow_mut();
            let entry = files.entry(path.to_string()).or_default();
            entry.push_str(content);
            Ok(true)
        }

        fn list_dir(&self, path: &str) -> Result<Vec<String>, String> {
            let files = self.files.borrow();
            let dirs = self.dirs.borrow();
            let prefix = if path.ends_with('/') {
                path.to_string()
            } else {
                format!("{path}/")
            };

            let mut entries = BTreeSet::new();

            // Collect file entries
            for key in files.keys() {
                if let Some(rest) = key.strip_prefix(&prefix) {
                    if let Some(name) = rest.split('/').next() {
                        if !name.is_empty() {
                            entries.insert(name.to_string());
                        }
                    }
                }
            }

            // Collect directory entries
            for dir in dirs.iter() {
                if let Some(rest) = dir.strip_prefix(&prefix) {
                    if let Some(name) = rest.split('/').next() {
                        if !name.is_empty() {
                            entries.insert(name.to_string());
                        }
                    }
                }
            }

            if entries.is_empty() && !dirs.contains(path) && !dirs.contains(&prefix) {
                return Err(format!("dir not found: {path}"));
            }

            Ok(entries.into_iter().collect())
        }

        fn create_dir(&self, path: &str) -> Result<bool, String> {
            // Create this and all parents
            let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
            let prefix = if path.starts_with('/') { "/" } else { "" };
            let mut current = String::new();
            for part in &parts {
                if current.is_empty() {
                    current = format!("{prefix}{part}");
                } else {
                    current = format!("{current}/{part}");
                }
                self.dirs.borrow_mut().insert(current.clone());
            }
            Ok(true)
        }

        fn file_exists(&self, path: &str) -> Result<bool, String> {
            let files = self.files.borrow();
            let dirs = self.dirs.borrow();
            Ok(files.contains_key(path) || dirs.contains(path))
        }

        fn now(&self) -> String {
            "2025-06-15T14:30:00".to_string()
        }
    }

    #[test]
    fn test_load_config_defaults() {
        let fs = TestFs::new();
        let config = load_config(&fs, "/vault").expect("load");
        assert_eq!(config.vault.wiki_dir, "wiki");
    }

    #[test]
    fn test_load_config_from_file() {
        let fs = TestFs::new();
        fs.set_file(
            "/vault/.agentzero-brain.toml",
            "[vault]\nwiki_dir = \"notes\"\n",
        );
        let config = load_config(&fs, "/vault").expect("load");
        assert_eq!(config.vault.wiki_dir, "notes");
    }

    // Safety tests

    #[test]
    fn test_path_traversal_init() {
        let fs = TestFs::new();
        let config = BrainConfig::default();
        let opts = InitOptions::default();
        let result = init::brain_init(&fs, "/vault/../etc", &config, &opts);
        assert!(result.is_err());
        assert!(
            matches!(result, Err(BrainError::PathTraversal(_))),
            "expected PathTraversal error"
        );
    }

    #[test]
    fn test_path_traversal_query() {
        let fs = TestFs::new();
        let config = BrainConfig::default();
        let result = query::brain_query(
            &fs,
            "/vault/../etc",
            &config,
            "test",
            &QueryOptions::default(),
        );
        assert!(matches!(result, Err(BrainError::PathTraversal(_))));
    }

    #[test]
    fn test_raw_immutable_flag_default() {
        let config = BrainConfig::default();
        assert!(config.safety.raw_is_immutable);
    }

    #[test]
    fn test_raw_immutable_blocks_writes() {
        let config = BrainConfig::default();
        // check_raw_immutable should reject writes under raw/
        let result = check_raw_immutable("/vault", "/vault/raw/inbox/file.md", &config);
        assert!(matches!(result, Err(BrainError::RawImmutable(_))));

        // But allow writes under wiki/
        let result = check_raw_immutable("/vault", "/vault/wiki/daily/2025-01-01.md", &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_raw_immutable_disabled() {
        let toml_str = "[safety]\nraw_is_immutable = false\n";
        let config = BrainConfig::from_toml(toml_str).expect("parse");
        // When disabled, writes to raw/ should be allowed
        let result = check_raw_immutable("/vault", "/vault/raw/inbox/file.md", &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_capture_into_raw_dir_rejected() {
        let fs = TestFs::new();
        // Set up a vault where wiki_dir points to raw
        let mut config = BrainConfig::default();
        config.vault.wiki_dir = "raw".to_string();
        // Init the vault so config file exists
        fs.set_file("/vault/.agentzero-brain.toml", "");
        fs.set_file("/vault/raw/daily/2025-06-15.md", "## Capture\n");
        let result = daily::brain_capture(
            &fs,
            "/vault",
            &config,
            "should fail",
            Some("2025-06-15"),
            None,
        );
        assert!(
            matches!(result, Err(BrainError::RawImmutable(_))),
            "expected RawImmutable, got: {result:?}"
        );
    }
}
