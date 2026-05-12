use serde::{Deserialize, Serialize};

/// Brain vault configuration, loaded from `.agentzero-brain.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainConfig {
    #[serde(default = "default_vault")]
    pub vault: VaultSection,
    #[serde(default = "default_daily")]
    pub daily: DailySection,
    #[serde(default = "default_safety")]
    pub safety: SafetySection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultSection {
    #[serde(default = "default_root")]
    pub root: String,
    #[serde(default = "default_raw_dir")]
    pub raw_dir: String,
    #[serde(default = "default_wiki_dir")]
    pub wiki_dir: String,
    #[serde(default = "default_templates_dir")]
    pub templates_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailySection {
    #[serde(default = "default_date_format")]
    pub date_format: String,
    #[serde(default = "default_time_format")]
    pub time_format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetySection {
    #[serde(default = "default_raw_is_immutable")]
    pub raw_is_immutable: bool,
    #[serde(default = "default_allow_destructive")]
    pub allow_destructive: bool,
}

fn default_root() -> String {
    ".".to_string()
}
fn default_raw_dir() -> String {
    "raw".to_string()
}
fn default_wiki_dir() -> String {
    "wiki".to_string()
}
fn default_templates_dir() -> String {
    "templates".to_string()
}
fn default_date_format() -> String {
    "%Y-%m-%d".to_string()
}
fn default_time_format() -> String {
    "%H:%M".to_string()
}
fn default_raw_is_immutable() -> bool {
    true
}
fn default_allow_destructive() -> bool {
    false
}
fn default_vault() -> VaultSection {
    VaultSection {
        root: default_root(),
        raw_dir: default_raw_dir(),
        wiki_dir: default_wiki_dir(),
        templates_dir: default_templates_dir(),
    }
}
fn default_daily() -> DailySection {
    DailySection {
        date_format: default_date_format(),
        time_format: default_time_format(),
    }
}
fn default_safety() -> SafetySection {
    SafetySection {
        raw_is_immutable: default_raw_is_immutable(),
        allow_destructive: default_allow_destructive(),
    }
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            vault: default_vault(),
            daily: default_daily(),
            safety: default_safety(),
        }
    }
}

impl BrainConfig {
    /// Load config from a TOML string, merging with defaults.
    pub fn from_toml(toml_str: &str) -> Result<Self, crate::BrainError> {
        let config: Self =
            toml::from_str(toml_str).map_err(|e| crate::BrainError::ConfigError(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Validate config directory values are safe.
    fn validate(&self) -> Result<(), crate::BrainError> {
        for (name, value) in [
            ("raw_dir", &self.vault.raw_dir),
            ("wiki_dir", &self.vault.wiki_dir),
            ("templates_dir", &self.vault.templates_dir),
        ] {
            if value.contains("..") {
                return Err(crate::BrainError::ConfigError(format!(
                    "{name} contains path traversal: {value}"
                )));
            }
            if value.starts_with('/') {
                return Err(crate::BrainError::ConfigError(format!(
                    "{name} must be relative, not absolute: {value}"
                )));
            }
            if value.contains('\0') {
                return Err(crate::BrainError::ConfigError(format!(
                    "{name} contains null byte"
                )));
            }
        }
        Ok(())
    }

    /// Serialize config to TOML string.
    pub fn to_toml(&self) -> Result<String, crate::BrainError> {
        toml::to_string_pretty(self).map_err(|e| crate::BrainError::ConfigError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_when_missing() {
        let cfg = BrainConfig::default();
        assert_eq!(cfg.vault.root, ".");
        assert_eq!(cfg.vault.raw_dir, "raw");
        assert_eq!(cfg.vault.wiki_dir, "wiki");
        assert_eq!(cfg.daily.date_format, "%Y-%m-%d");
        assert!(cfg.safety.raw_is_immutable);
        assert!(!cfg.safety.allow_destructive);
    }

    #[test]
    fn test_loads_from_toml() {
        let toml_str = r#"
[vault]
root = "/my/vault"
raw_dir = "inbox"

[daily]
date_format = "%d-%m-%Y"
"#;
        let cfg = BrainConfig::from_toml(toml_str).expect("parse");
        assert_eq!(cfg.vault.root, "/my/vault");
        assert_eq!(cfg.vault.raw_dir, "inbox");
        // defaults for missing fields
        assert_eq!(cfg.vault.wiki_dir, "wiki");
        assert_eq!(cfg.daily.date_format, "%d-%m-%Y");
        assert_eq!(cfg.daily.time_format, "%H:%M");
    }

    #[test]
    fn test_rejects_traversal_in_config_dirs() {
        let toml_str = "[vault]\nwiki_dir = \"../../etc\"\n";
        let result = BrainConfig::from_toml(toml_str);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("path traversal"), "got: {err}");
    }

    #[test]
    fn test_rejects_absolute_config_dirs() {
        let toml_str = "[vault]\nraw_dir = \"/etc/passwd\"\n";
        let result = BrainConfig::from_toml(toml_str);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("relative"), "got: {err}");
    }

    #[test]
    fn test_merges_with_defaults() {
        let toml_str = r#"
[safety]
allow_destructive = true
"#;
        let cfg = BrainConfig::from_toml(toml_str).expect("parse");
        assert!(cfg.safety.allow_destructive);
        assert!(cfg.safety.raw_is_immutable); // default
        assert_eq!(cfg.vault.root, "."); // default
    }
}
