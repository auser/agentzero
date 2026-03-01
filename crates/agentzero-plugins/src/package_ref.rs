use thiserror::Error;

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
}
