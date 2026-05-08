use std::path::Path;

/// File extensions treated as indexable text.
const TEXT_EXTENSIONS: &[&str] = &[
    "txt",
    "md",
    "markdown",
    "rst",
    "org",
    "adoc", // Prose
    "rs",
    "py",
    "js",
    "ts",
    "jsx",
    "tsx",
    "go",
    "java",
    "c",
    "h",
    "cpp",
    "hpp",
    "cs",
    "rb",
    "php",
    "swift",
    "kt",
    "scala",
    "lua",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1", // Code
    "toml",
    "yaml",
    "yml",
    "json",
    "xml",
    "csv",
    "ini",
    "cfg",
    "conf", // Config
    "sql",
    "graphql",
    "proto", // Data/schema
    "dockerfile",
    "makefile", // Build
    "html",
    "css",
    "scss",
    "less",
    "sass", // Web
    "el",
    "clj",
    "cljs",
    "edn",
    "ex",
    "exs",
    "erl",
    "hrl",
    "hs",
    "ml",
    "mli",
    "nim",
    "zig",
    "v",
    "sv", // More languages
];

/// Directories to always skip when walking.
const SKIP_DIRS: &[&str] = &[
    ".agentzero",
    ".git",
    ".hg",
    ".svn",
    "target",
    "node_modules",
    "__pycache__",
    ".tox",
    ".venv",
    "venv",
    "dist",
    "build",
    ".next",
    ".cache",
];

/// Determine whether a file should be indexed based on its extension.
pub fn is_indexable(path: &Path) -> bool {
    // Files with no extension: check common extensionless files
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return is_known_extensionless(path);
    };

    TEXT_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// Check for known extensionless files like Makefile, Dockerfile, etc.
fn is_known_extensionless(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "makefile" | "dockerfile" | "rakefile" | "gemfile" | "procfile" | "justfile"
    )
}

/// Whether a directory should be skipped during traversal.
pub fn should_skip_dir(name: &str) -> bool {
    name.starts_with('.') || SKIP_DIRS.contains(&name)
}

/// Read a file as UTF-8 text, returning `None` for binary/unreadable files.
pub fn read_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;

    // Quick binary check: look for null bytes in first 8KB
    let check_len = bytes.len().min(8192);
    if bytes[..check_len].contains(&0) {
        return None;
    }

    String::from_utf8(bytes).ok()
}

/// Walk a directory tree, yielding paths of indexable files.
pub fn walk_indexable(root: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    walk_recursive(root, &mut files);
    files.sort();
    files
}

fn walk_recursive(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if path.is_dir() {
            if !should_skip_dir(name) {
                walk_recursive(&path, out);
            }
        } else if path.is_file() && is_indexable(&path) {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_code_files() {
        assert!(is_indexable(Path::new("src/main.rs")));
        assert!(is_indexable(Path::new("app.py")));
        assert!(is_indexable(Path::new("index.ts")));
    }

    #[test]
    fn recognizes_config_files() {
        assert!(is_indexable(Path::new("Cargo.toml")));
        assert!(is_indexable(Path::new("config.yaml")));
        assert!(is_indexable(Path::new("data.json")));
    }

    #[test]
    fn rejects_binary_extensions() {
        assert!(!is_indexable(Path::new("image.png")));
        assert!(!is_indexable(Path::new("archive.zip")));
        assert!(!is_indexable(Path::new("binary.exe")));
    }

    #[test]
    fn recognizes_extensionless_files() {
        assert!(is_indexable(Path::new("Makefile")));
        assert!(is_indexable(Path::new("Dockerfile")));
    }

    #[test]
    fn skips_known_dirs() {
        assert!(should_skip_dir(".git"));
        assert!(should_skip_dir("target"));
        assert!(should_skip_dir("node_modules"));
        assert!(!should_skip_dir("src"));
    }
}
