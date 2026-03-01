use crate::cli::TunnelCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_tunnel::{parse_tunnel_protocol, TunnelStore};
use async_trait::async_trait;

pub struct TunnelCommand;

#[async_trait]
impl AgentZeroCommand for TunnelCommand {
    type Options = TunnelCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let store = TunnelStore::new(&ctx.data_dir)?;

        match opts {
            TunnelCommands::Start {
                name,
                protocol,
                remote,
                local_port,
            } => {
                let protocol = parse_tunnel_protocol(&protocol)
                    .map_err(|err| anyhow::anyhow!(err.to_string()))?;
                let started = store.start(&name, protocol, &remote, local_port)?;
                println!(
                    "Started tunnel `{}` [{:?}] {} -> localhost:{}",
                    started.name, started.protocol, started.remote, started.local_port
                );
            }
            TunnelCommands::Stop { name } => {
                let stopped = store.stop(&name)?;
                println!("Stopped tunnel `{}`", stopped.name);
            }
            TunnelCommands::Status { name, json } => {
                let status = store.status(&name)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&status)?);
                } else {
                    println!(
                        "Tunnel `{}`: {:?} {} -> localhost:{} ({})",
                        status.name,
                        status.protocol,
                        status.remote,
                        status.local_port,
                        if status.active { "active" } else { "stopped" }
                    );
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::TunnelCommand;
    use crate::cli::TunnelCommands;
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
        let dir = std::env::temp_dir().join(format!("agentzero-tunnel-cmd-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn tunnel_start_and_status_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        TunnelCommand::run(
            &ctx,
            TunnelCommands::Start {
                name: "default".to_string(),
                protocol: "https".to_string(),
                remote: "example.com:443".to_string(),
                local_port: 9422,
            },
        )
        .await
        .expect("start should succeed");

        TunnelCommand::run(
            &ctx,
            TunnelCommands::Status {
                name: "default".to_string(),
                json: true,
            },
        )
        .await
        .expect("status should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn tunnel_start_with_invalid_protocol_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = TunnelCommand::run(
            &ctx,
            TunnelCommands::Start {
                name: "default".to_string(),
                protocol: "ftp".to_string(),
                remote: "example.com:443".to_string(),
                local_port: 9422,
            },
        )
        .await
        .expect_err("invalid protocol should fail");
        assert!(err.to_string().contains("unsupported protocol"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
