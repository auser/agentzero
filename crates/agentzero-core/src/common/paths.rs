use std::path::{Path, PathBuf};

pub const ENV_DATA_DIR: &str = "AGENTZERO_DATA_DIR";
pub const ENV_CONFIG_PATH: &str = "AGENTZERO_CONFIG";
pub const DEFAULT_DATA_DIR_NAME: &str = ".agentzero";
pub const DEFAULT_CONFIG_FILE: &str = "agentzero.toml";
pub const DEFAULT_SQLITE_FILE: &str = "agentzero.db";
pub const MCP_CONFIG_FILE: &str = "mcp.json";

pub fn data_dir_for_home(home_dir: &Path) -> PathBuf {
    home_dir.join(DEFAULT_DATA_DIR_NAME)
}

pub fn default_data_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| data_dir_for_home(&home))
}

pub fn default_config_path() -> Option<PathBuf> {
    default_data_dir().map(|dir| dir.join(DEFAULT_CONFIG_FILE))
}

pub fn default_sqlite_path() -> Option<PathBuf> {
    default_data_dir().map(|dir| dir.join(DEFAULT_SQLITE_FILE))
}

#[cfg(test)]
mod tests {
    use super::{
        data_dir_for_home, default_config_path, default_data_dir, default_sqlite_path,
        DEFAULT_CONFIG_FILE, DEFAULT_DATA_DIR_NAME, DEFAULT_SQLITE_FILE,
    };
    use std::path::PathBuf;

    #[test]
    fn data_dir_for_home_joins_expected_segment_success_path() {
        let home = PathBuf::from("/tmp/home");
        let data_dir = data_dir_for_home(&home);
        assert_eq!(data_dir, home.join(DEFAULT_DATA_DIR_NAME));
    }

    #[test]
    fn default_paths_share_same_data_dir_root_success_path() {
        if let (Some(data_dir), Some(config_path), Some(sqlite_path)) = (
            default_data_dir(),
            default_config_path(),
            default_sqlite_path(),
        ) {
            assert_eq!(config_path, data_dir.join(DEFAULT_CONFIG_FILE));
            assert_eq!(sqlite_path, data_dir.join(DEFAULT_SQLITE_FILE));
        }
    }
}
