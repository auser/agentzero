use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_daemon::DaemonManager;
use async_trait::async_trait;

pub struct DaemonOptions {
    pub host: Option<String>,
    pub port: Option<u16>,
}

pub struct DaemonCommand;

#[async_trait]
impl AgentZeroCommand for DaemonCommand {
    type Options = DaemonOptions;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let manager = DaemonManager::new(&ctx.data_dir)?;
        let host = opts.host.unwrap_or_else(|| "127.0.0.1".to_string());
        let port = opts.port.unwrap_or(8080);

        manager.mark_started(host.clone(), port)?;
        println!("Starting daemon runtime on {host}:{port}");

        let token_store_path = ctx.data_dir.join("gateway-paired-tokens.json");
        let run_result = agentzero_gateway::run(
            &host,
            port,
            agentzero_gateway::GatewayRunOptions {
                token_store_path: Some(token_store_path),
                new_pairing: false,
            },
        )
        .await;

        if let Err(err) = manager.mark_stopped() {
            eprintln!("Warning: failed to update daemon state after shutdown: {err}");
        }

        run_result
    }
}

#[cfg(test)]
mod tests {
    use agentzero_daemon::DaemonManager;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("agentzero-cli-daemon-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn daemon_manager_mark_started_and_status_success_path() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager should be created");

        let started = manager
            .mark_started("0.0.0.0".to_string(), 9090)
            .expect("mark_started should succeed");
        assert!(started.running);
        assert_eq!(started.host.as_deref(), Some("0.0.0.0"));
        assert_eq!(started.port, Some(9090));

        let status = manager.status().expect("status should succeed");
        assert!(status.running);
        assert_eq!(status.host.as_deref(), Some("0.0.0.0"));
        assert_eq!(status.port, Some(9090));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn daemon_manager_mark_stopped_without_start_fails_negative_path() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager should be created");

        let err = manager
            .mark_stopped()
            .expect_err("stopping without start should fail");
        assert!(err.to_string().contains("not running"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
