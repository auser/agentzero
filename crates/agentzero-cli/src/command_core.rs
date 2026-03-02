use agentzero_common::paths::{
    default_data_dir as common_default_data_dir, DEFAULT_CONFIG_FILE, ENV_CONFIG_PATH, ENV_DATA_DIR,
};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CommandContext {
    pub workspace_root: PathBuf,
    pub data_dir: PathBuf,
    pub config_path: PathBuf,
}

impl CommandContext {
    pub fn from_current_dir(
        config_override: Option<PathBuf>,
        data_dir_override: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let workspace_root = std::env::current_dir()?.canonicalize()?;
        let default_data_dir = default_data_dir()?;
        let preliminary_config_path = resolve_config_path(
            config_override.as_ref(),
            Some(&default_data_dir),
            &workspace_root,
        );

        let data_dir = resolve_data_dir(
            data_dir_override,
            &workspace_root,
            Some(&preliminary_config_path),
            default_data_dir,
        )?;
        let config_path =
            resolve_config_path(config_override.as_ref(), Some(&data_dir), &workspace_root);

        Ok(Self {
            workspace_root,
            data_dir,
            config_path,
        })
    }
}

fn resolve_data_dir(
    data_dir_override: Option<PathBuf>,
    workspace_root: &std::path::Path,
    config_path: Option<&PathBuf>,
    default_data_dir: PathBuf,
) -> anyhow::Result<PathBuf> {
    if let Some(path) = data_dir_override {
        return absolutize_path(path, workspace_root);
    }

    if let Ok(path) = std::env::var(ENV_DATA_DIR) {
        return absolutize_path(PathBuf::from(path), workspace_root);
    }

    if let Some(config_path) = config_path {
        if let Some(path) = read_data_dir_from_config(config_path, workspace_root)? {
            return Ok(path);
        }
    }

    Ok(default_data_dir)
}

fn resolve_config_path(
    config_override: Option<&PathBuf>,
    data_dir: Option<&PathBuf>,
    workspace_root: &std::path::Path,
) -> PathBuf {
    if let Some(path) = config_override {
        return absolutize_path_lossy(path.clone(), workspace_root);
    }

    if let Ok(path) = std::env::var(ENV_CONFIG_PATH) {
        return absolutize_path_lossy(PathBuf::from(path), workspace_root);
    }

    let data_dir = data_dir
        .cloned()
        .unwrap_or_else(|| workspace_root.to_path_buf());
    data_dir.join(DEFAULT_CONFIG_FILE)
}

fn read_data_dir_from_config(
    config_path: &std::path::Path,
    workspace_root: &std::path::Path,
) -> anyhow::Result<Option<PathBuf>> {
    if !config_path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let value: toml::Value = raw
        .parse()
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    let path = value
        .get("data_dir")
        .and_then(toml::Value::as_str)
        .or_else(|| {
            value
                .get("paths")
                .and_then(toml::Value::as_table)
                .and_then(|paths| paths.get("data_dir"))
                .and_then(toml::Value::as_str)
        });

    let Some(path) = path else {
        return Ok(None);
    };

    let base = config_path.parent().unwrap_or(workspace_root);
    absolutize_path(PathBuf::from(path), base).map(Some)
}

fn absolutize_path(path: PathBuf, base: &std::path::Path) -> anyhow::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    Ok(absolute)
}

fn absolutize_path_lossy(path: PathBuf, base: &std::path::Path) -> PathBuf {
    absolutize_path(path, base).unwrap_or_else(|_| base.join("agentzero.toml"))
}

fn default_data_dir() -> anyhow::Result<PathBuf> {
    common_default_data_dir().ok_or_else(|| {
        anyhow!("failed to determine home directory for default data dir; set --data-dir or AGENTZERO_DATA_DIR")
    })
}

#[async_trait]
pub trait AgentZeroCommand {
    type Options: Send;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::CommandContext;
    use agentzero_common::paths::{
        DEFAULT_CONFIG_FILE, DEFAULT_DATA_DIR_NAME, ENV_CONFIG_PATH, ENV_DATA_DIR,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    use temp_env::with_vars;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-cli-context-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn resolves_default_data_dir_from_home_success_path() {
        let home = temp_dir();
        with_vars(
            vec![
                ("HOME", Some(home.to_string_lossy().to_string())),
                (ENV_DATA_DIR, None::<String>),
                (ENV_CONFIG_PATH, None::<String>),
            ],
            || {
                let ctx = CommandContext::from_current_dir(None, None)
                    .expect("context should resolve with home default");
                assert_eq!(ctx.data_dir, home.join(DEFAULT_DATA_DIR_NAME));
                assert_eq!(
                    ctx.config_path,
                    home.join(DEFAULT_DATA_DIR_NAME).join(DEFAULT_CONFIG_FILE)
                );
            },
        );
        fs::remove_dir_all(home).expect("temp dir should be removed");
    }

    #[test]
    fn data_dir_flag_overrides_env_and_defaults_success_path() {
        let home = temp_dir();
        let flag_dir = temp_dir();
        let env_dir = temp_dir();
        with_vars(
            vec![
                ("HOME", Some(home.to_string_lossy().to_string())),
                (ENV_DATA_DIR, Some(env_dir.to_string_lossy().to_string())),
            ],
            || {
                let ctx = CommandContext::from_current_dir(None, Some(flag_dir.clone()))
                    .expect("context should resolve");
                assert_eq!(ctx.data_dir, flag_dir);
            },
        );
        fs::remove_dir_all(home).expect("temp dir should be removed");
        fs::remove_dir_all(env_dir).expect("temp dir should be removed");
        fs::remove_dir_all(flag_dir).expect("temp dir should be removed");
    }

    #[test]
    fn reads_data_dir_from_config_when_no_flag_or_env_success_path() {
        let home = temp_dir();
        let configured_dir = temp_dir();
        let config_dir = home.join(DEFAULT_DATA_DIR_NAME);
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("agentzero.toml"),
            format!("data_dir = \"{}\"\n", configured_dir.display()),
        )
        .expect("config file should be written");
        with_vars(
            vec![
                ("HOME", Some(home.to_string_lossy().to_string())),
                (ENV_DATA_DIR, None::<String>),
                (ENV_CONFIG_PATH, None::<String>),
            ],
            || {
                let ctx =
                    CommandContext::from_current_dir(None, None).expect("context should resolve");
                assert_eq!(ctx.data_dir, configured_dir);
            },
        );
        fs::remove_dir_all(home).expect("temp dir should be removed");
        fs::remove_dir_all(configured_dir).expect("temp dir should be removed");
    }

    #[test]
    fn invalid_config_toml_returns_error_negative_path() {
        let home = temp_dir();
        let config_dir = home.join(DEFAULT_DATA_DIR_NAME);
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(config_dir.join(DEFAULT_CONFIG_FILE), "not-toml = [")
            .expect("invalid config should be written");
        with_vars(
            vec![
                ("HOME", Some(home.to_string_lossy().to_string())),
                (ENV_DATA_DIR, None::<String>),
                (ENV_CONFIG_PATH, None::<String>),
            ],
            || {
                let err = CommandContext::from_current_dir(None, None)
                    .expect_err("invalid config should fail");
                assert!(err.to_string().contains("failed to parse"));
            },
        );
        fs::remove_dir_all(home).expect("temp dir should be removed");
    }
}
