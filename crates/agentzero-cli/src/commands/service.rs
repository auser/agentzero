use crate::cli::ServiceCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_service::ServiceManager;
use async_trait::async_trait;

pub struct ServiceCommand;

#[async_trait]
impl AgentZeroCommand for ServiceCommand {
    type Options = ServiceCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let manager = ServiceManager::new(&ctx.data_dir)?;

        match opts {
            ServiceCommands::Install => {
                manager.install()?;
                println!("Service installed");
            }
            ServiceCommands::Restart => {
                manager.restart()?;
                println!("Service restarted");
            }
            ServiceCommands::Start => {
                manager.start()?;
                println!("Service started");
            }
            ServiceCommands::Stop => {
                manager.stop()?;
                println!("Service stopped");
            }
            ServiceCommands::Uninstall => {
                manager.uninstall()?;
                println!("Service uninstalled");
            }
            ServiceCommands::Status => {
                let status = manager.status()?;
                println!("Service status");
                println!("  installed: {}", status.installed);
                println!("  running: {}", status.running);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceCommand;
    use crate::cli::ServiceCommands;
    use crate::command_core::{AgentZeroCommand, CommandContext};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-cli-service-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn service_restart_and_uninstall_success_path() {
        let data_dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: data_dir.clone(),
            data_dir: data_dir.clone(),
            config_path: data_dir.join("agentzero.toml"),
        };

        ServiceCommand::run(&ctx, ServiceCommands::Install)
            .await
            .expect("install should succeed");
        ServiceCommand::run(&ctx, ServiceCommands::Restart)
            .await
            .expect("restart should succeed");
        ServiceCommand::run(&ctx, ServiceCommands::Uninstall)
            .await
            .expect("uninstall should succeed");

        fs::remove_dir_all(data_dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn service_restart_without_install_fails_negative_path() {
        let data_dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: data_dir.clone(),
            data_dir: data_dir.clone(),
            config_path: data_dir.join("agentzero.toml"),
        };

        let err = ServiceCommand::run(&ctx, ServiceCommands::Restart)
            .await
            .expect_err("restart should fail without install");
        assert!(err.to_string().contains("service is not installed"));

        fs::remove_dir_all(data_dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn service_start_stop_lifecycle_success_path() {
        let data_dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: data_dir.clone(),
            data_dir: data_dir.clone(),
            config_path: data_dir.join("agentzero.toml"),
        };

        ServiceCommand::run(&ctx, ServiceCommands::Install)
            .await
            .expect("install should succeed");
        ServiceCommand::run(&ctx, ServiceCommands::Start)
            .await
            .expect("start should succeed");
        ServiceCommand::run(&ctx, ServiceCommands::Stop)
            .await
            .expect("stop should succeed");
        ServiceCommand::run(&ctx, ServiceCommands::Uninstall)
            .await
            .expect("uninstall should succeed");

        fs::remove_dir_all(data_dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn service_stop_without_install_fails_negative_path() {
        let data_dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: data_dir.clone(),
            data_dir: data_dir.clone(),
            config_path: data_dir.join("agentzero.toml"),
        };

        let err = ServiceCommand::run(&ctx, ServiceCommands::Stop)
            .await
            .expect_err("stop without install should fail");
        assert!(err.to_string().contains("service is not installed"));

        fs::remove_dir_all(data_dir).expect("temp dir should be removed");
    }
}
