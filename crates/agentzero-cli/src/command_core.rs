use async_trait::async_trait;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CommandContext {
    pub workspace_root: PathBuf,
    pub config_path: PathBuf,
}

impl CommandContext {
    pub fn from_current_dir(config_override: Option<PathBuf>) -> anyhow::Result<Self> {
        let workspace_root = std::env::current_dir()?.canonicalize()?;
        let config_path = if let Some(path) = config_override {
            if path.is_absolute() {
                path
            } else {
                workspace_root.join(path)
            }
        } else {
            std::env::var("AGENTZERO_CONFIG")
                .map(PathBuf::from)
                .unwrap_or_else(|_| workspace_root.join("agentzero.toml"))
        };

        Ok(Self {
            workspace_root,
            config_path,
        })
    }
}

#[async_trait]
pub trait AgentZeroCommand {
    type Options: Send;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()>;
}
