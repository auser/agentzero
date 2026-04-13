use std::path::{Path, PathBuf};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Unified package reference (Plan 45 Phase 4)
// ---------------------------------------------------------------------------

/// A parsed package reference from user input.
///
/// Supports multiple source formats:
/// - `npm:@scope/name@version` — npm registry
/// - `git:github.com/user/repo#branch` — git clone
/// - `file:./local-path` — local directory
/// - `https://example.com/pkg.tar.gz` — URL download
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageRef {
    Npm {
        scope: Option<String>,
        name: String,
        version: Option<String>,
    },
    Git {
        url: String,
        branch: Option<String>,
    },
    File {
        path: PathBuf,
    },
    Url {
        url: String,
    },
}

/// What type of package was detected in a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageType {
    /// Has `skill.toml` — a skill bundle.
    SkillBundle,
    /// Has `manifest.json` — a WASM plugin.
    WasmPlugin,
    /// Could not determine type.
    Unknown,
}

impl PackageRef {
    /// Parse a package reference string.
    pub fn parse(input: &str) -> anyhow::Result<Self> {
        let input = input.trim();
        if input.is_empty() {
            anyhow::bail!("package reference cannot be empty");
        }

        if let Some(rest) = input.strip_prefix("npm:") {
            return Self::parse_npm(rest);
        }
        if let Some(rest) = input.strip_prefix("git:") {
            return Self::parse_git(rest);
        }
        if let Some(rest) = input.strip_prefix("file:") {
            return Ok(PackageRef::File {
                path: PathBuf::from(rest),
            });
        }
        if input.starts_with("https://") || input.starts_with("http://") {
            return Ok(PackageRef::Url {
                url: input.to_string(),
            });
        }

        // Try to interpret as a local path if it exists.
        let as_path = PathBuf::from(input);
        if as_path.exists() {
            return Ok(PackageRef::File { path: as_path });
        }

        anyhow::bail!(
            "unrecognized package reference: `{input}`\n\
             Expected: npm:<name>, git:<url>, file:<path>, or https://<url>"
        );
    }

    fn parse_npm(input: &str) -> anyhow::Result<Self> {
        if input.is_empty() {
            anyhow::bail!("npm package name cannot be empty");
        }

        if let Some(without_at) = input.strip_prefix('@') {
            // Scoped: @scope/name@version
            let parts: Vec<&str> = without_at.splitn(2, '/').collect();
            if parts.len() < 2 {
                anyhow::bail!("scoped npm package must be @scope/name, got: {input}");
            }
            let scope = parts[0].to_string();
            let (name, version) = split_at_version(parts[1]);
            Ok(PackageRef::Npm {
                scope: Some(scope),
                name: name.to_string(),
                version: version.map(String::from),
            })
        } else {
            let (name, version) = split_at_version(input);
            Ok(PackageRef::Npm {
                scope: None,
                name: name.to_string(),
                version: version.map(String::from),
            })
        }
    }

    fn parse_git(input: &str) -> anyhow::Result<Self> {
        if input.is_empty() {
            anyhow::bail!("git URL cannot be empty");
        }

        let (url, branch) = if let Some((url_part, branch_part)) = input.split_once('#') {
            (url_part.to_string(), Some(branch_part.to_string()))
        } else {
            (input.to_string(), None)
        };

        // Normalize GitHub shorthand.
        let url = if !url.contains("://") && url.contains('/') {
            format!("https://{url}")
        } else {
            url
        };

        Ok(PackageRef::Git { url, branch })
    }
}

/// Split `name@version` into `(name, Some(version))` or `(name, None)`.
fn split_at_version(s: &str) -> (&str, Option<&str>) {
    if let Some((name, version)) = s.rsplit_once('@') {
        if name.is_empty() {
            (s, None)
        } else {
            (name, Some(version))
        }
    } else {
        (s, None)
    }
}

/// Detect the package type by inspecting directory contents.
pub fn detect_package_type(dir: &Path) -> PackageType {
    if dir.join("skill.toml").exists() {
        PackageType::SkillBundle
    } else if dir.join("manifest.json").exists() {
        PackageType::WasmPlugin
    } else {
        PackageType::Unknown
    }
}

// ---------------------------------------------------------------------------
// Legacy plugin-specific reference (pre-existing)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPackageRef {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PluginRefError {
    #[error("plugin reference must be formatted as <name>@<version>")]
    InvalidFormat,
    #[error("plugin name contains invalid characters: {0}")]
    InvalidName(String),
    #[error("plugin version cannot be empty")]
    EmptyVersion,
}

pub fn parse_plugin_package_ref(input: &str) -> Result<PluginPackageRef, PluginRefError> {
    let (name, version) = input.split_once('@').ok_or(PluginRefError::InvalidFormat)?;

    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(PluginRefError::InvalidName(name.to_string()));
    }

    if version.trim().is_empty() {
        return Err(PluginRefError::EmptyVersion);
    }

    Ok(PluginPackageRef {
        name: name.to_string(),
        version: version.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plugin_package_ref_accepts_valid_ref() {
        let parsed = parse_plugin_package_ref("my_plugin@1.2.3").expect("valid ref");
        assert_eq!(parsed.name, "my_plugin");
        assert_eq!(parsed.version, "1.2.3");
    }

    #[test]
    fn parse_plugin_package_ref_rejects_missing_separator() {
        let err = parse_plugin_package_ref("my_plugin").expect_err("missing @ should fail");
        assert_eq!(err, PluginRefError::InvalidFormat);
    }

    // ── PackageRef tests ────────────────────────────────────────────────

    #[test]
    fn package_ref_npm_simple() {
        let pkg = PackageRef::parse("npm:my-tool").expect("parse");
        assert_eq!(
            pkg,
            PackageRef::Npm {
                scope: None,
                name: "my-tool".into(),
                version: None,
            }
        );
    }

    #[test]
    fn package_ref_npm_scoped_with_version() {
        let pkg = PackageRef::parse("npm:@myorg/my-tool@2.0.0").expect("parse");
        assert_eq!(
            pkg,
            PackageRef::Npm {
                scope: Some("myorg".into()),
                name: "my-tool".into(),
                version: Some("2.0.0".into()),
            }
        );
    }

    #[test]
    fn package_ref_git_shorthand() {
        let pkg = PackageRef::parse("git:github.com/user/repo").expect("parse");
        assert_eq!(
            pkg,
            PackageRef::Git {
                url: "https://github.com/user/repo".into(),
                branch: None,
            }
        );
    }

    #[test]
    fn package_ref_git_with_branch() {
        let pkg = PackageRef::parse("git:github.com/user/repo#main").expect("parse");
        assert_eq!(
            pkg,
            PackageRef::Git {
                url: "https://github.com/user/repo".into(),
                branch: Some("main".into()),
            }
        );
    }

    #[test]
    fn package_ref_file() {
        let pkg = PackageRef::parse("file:./my-plugin").expect("parse");
        assert_eq!(
            pkg,
            PackageRef::File {
                path: PathBuf::from("./my-plugin"),
            }
        );
    }

    #[test]
    fn package_ref_url() {
        let pkg = PackageRef::parse("https://example.com/plugin.tar.gz").expect("parse");
        assert_eq!(
            pkg,
            PackageRef::Url {
                url: "https://example.com/plugin.tar.gz".into(),
            }
        );
    }

    #[test]
    fn package_ref_empty_fails() {
        PackageRef::parse("").expect_err("should fail");
    }

    #[test]
    fn detect_skill_bundle_type() {
        let dir = std::env::temp_dir().join("detect-skill-type-test");
        std::fs::create_dir_all(&dir).ok();
        std::fs::write(dir.join("skill.toml"), "name = \"test\"").ok();
        assert_eq!(detect_package_type(&dir), PackageType::SkillBundle);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn detect_wasm_plugin_type() {
        let dir = std::env::temp_dir().join("detect-wasm-type-test");
        std::fs::create_dir_all(&dir).ok();
        std::fs::write(dir.join("manifest.json"), "{}").ok();
        assert_eq!(detect_package_type(&dir), PackageType::WasmPlugin);
        std::fs::remove_dir_all(dir).ok();
    }
}
