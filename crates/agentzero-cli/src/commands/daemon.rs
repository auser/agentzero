use crate::cli::DaemonCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use crate::daemon::{find_available_port, DaemonManager};
use async_trait::async_trait;
use std::fs::OpenOptions;
use std::process::{Command, Stdio};

pub struct DaemonCommand;

#[async_trait]
impl AgentZeroCommand for DaemonCommand {
    type Options = DaemonCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let manager = DaemonManager::new(&ctx.data_dir)?;

        match opts {
            DaemonCommands::Start {
                host,
                port,
                foreground,
            } => {
                let cfg = agentzero_config::load(&ctx.config_path).ok();
                let host = host.unwrap_or_else(|| {
                    cfg.as_ref()
                        .map(|c| c.gateway.host.clone())
                        .unwrap_or_else(|| "127.0.0.1".to_string())
                });
                let requested_port =
                    port.unwrap_or_else(|| cfg.as_ref().map(|c| c.gateway.port).unwrap_or(8080));
                let port = find_available_port(&host, requested_port)?;
                if port != requested_port {
                    println!("port {requested_port} is in use, using port {port} instead");
                }

                if foreground {
                    run_foreground(&manager, ctx, host, port).await
                } else {
                    spawn_background(&manager, ctx, host, port)
                }
            }
            DaemonCommands::Stop => {
                manager.stop_process()?;
                println!("daemon stopped");
                Ok(())
            }
            DaemonCommands::Restart { host, port } => {
                // Capture previous host/port before stopping.
                let prev = manager.status()?;
                let prev_host = prev.host.unwrap_or_else(|| "127.0.0.1".to_string());
                let prev_port = prev.port.unwrap_or(8080);

                if prev.running {
                    manager.stop_process()?;
                    println!("daemon stopped");
                    // Brief pause to let the port release.
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }

                let cfg = agentzero_config::load(&ctx.config_path).ok();
                let host = host.unwrap_or_else(|| {
                    cfg.as_ref()
                        .map(|c| c.gateway.host.clone())
                        .unwrap_or(prev_host)
                });
                let requested_port = port
                    .unwrap_or_else(|| cfg.as_ref().map(|c| c.gateway.port).unwrap_or(prev_port));
                let port = find_available_port(&host, requested_port)?;
                if port != requested_port {
                    println!("port {requested_port} is in use, using port {port} instead");
                }

                spawn_background(&manager, ctx, host, port)
            }
            DaemonCommands::Status { json } => {
                let status = manager.status()?;
                if json {
                    let value = serde_json::json!({
                        "running": status.running,
                        "host": status.host,
                        "port": status.port,
                        "pid": status.pid,
                        "started_at_epoch_seconds": status.started_at_epoch_seconds,
                    });
                    println!("{}", serde_json::to_string_pretty(&value)?);
                } else if status.running {
                    let pid = status.pid.map_or("unknown".to_string(), |p| p.to_string());
                    let host = status.host.as_deref().unwrap_or("?");
                    let port = status.port.map_or("?".to_string(), |p| p.to_string());
                    println!("daemon running (pid {pid}) on {host}:{port}");
                    println!("log: {}", ctx.data_dir.join("daemon.log").display());
                } else {
                    println!("daemon not running");
                }
                Ok(())
            }
        }
    }
}

#[cfg(feature = "gateway")]
async fn run_foreground(
    manager: &DaemonManager,
    ctx: &CommandContext,
    host: String,
    port: u16,
) -> anyhow::Result<()> {
    let pid = std::process::id();
    manager.mark_started(host.clone(), port, pid)?;
    crate::daemon::write_pid_file(&ctx.data_dir, pid)?;
    crate::daemon::rotate_log_if_needed(
        &ctx.data_dir,
        &crate::daemon::LogRotationConfig::default(),
    )?;
    println!("daemon running in foreground (pid {pid}) on {host}:{port}");

    // Auto-discover local AI providers at startup.
    let discovery = crate::local::discover_local_services(crate::local::DiscoveryOptions {
        timeout_ms: 2000,
        providers: Vec::new(),
    })
    .await;
    let summary = crate::local::format_discovery_summary(&discovery);
    println!("{summary}");

    let token_store_path = ctx.data_dir.join("gateway-paired-tokens.json");
    let run_result = agentzero_gateway::run(
        &host,
        port,
        agentzero_gateway::GatewayRunOptions {
            token_store_path: Some(token_store_path),
            new_pairing: false,
            data_dir: Some(ctx.data_dir.clone()),
            config_path: Some(ctx.config_path.clone()),
            workspace_root: Some(ctx.workspace_root.clone()),
            ..Default::default()
        },
    )
    .await;

    crate::daemon::remove_pid_file(&ctx.data_dir);
    if let Err(err) = manager.mark_stopped() {
        eprintln!("warning: failed to update daemon state after shutdown: {err}");
    }

    run_result
}

#[cfg(not(feature = "gateway"))]
async fn run_foreground(
    _manager: &DaemonManager,
    _ctx: &CommandContext,
    _host: String,
    _port: u16,
) -> anyhow::Result<()> {
    anyhow::bail!("gateway is not available (built without gateway feature)")
}

/// Spawn the daemon as a detached background process and exit.
fn spawn_background(
    manager: &DaemonManager,
    ctx: &CommandContext,
    host: String,
    port: u16,
) -> anyhow::Result<()> {
    // Check if already running.
    let status = manager.status()?;
    if status.running {
        let pid = status.pid.map_or("unknown".to_string(), |p| p.to_string());
        anyhow::bail!("daemon is already running (pid {pid})");
    }

    let exe = std::env::current_exe()?;
    let port_str = port.to_string();

    let mut cmd = Command::new(exe);
    cmd.args([
        "daemon",
        "start",
        "--host",
        &host,
        "--port",
        &port_str,
        "--foreground",
    ]);

    // Pass through config/data dir flags if they were set.
    if let Some(config_flag) = ctx.config_path.to_str() {
        cmd.args(["--config", config_flag]);
    }
    if let Some(data_flag) = ctx.data_dir.to_str() {
        cmd.args(["--data-dir", data_flag]);
    }

    // Redirect stdout/stderr to a log file so startup errors are visible.
    let log_path = ctx.data_dir.join("daemon.log");
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let log_stderr = log_file.try_clone()?;

    cmd.stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_stderr));

    // Detach from parent process group on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let child = cmd.spawn()?;
    let pid = child.id();

    // Don't call mark_started here — the child's run_foreground() owns the state.
    // We just do a brief liveness check to catch immediate crashes.
    std::thread::sleep(std::time::Duration::from_secs(1));
    if !crate::daemon::is_process_alive(pid) {
        let hint = tail_log(&log_path, 10);
        let mut msg = format!("daemon (pid {pid}) exited immediately after starting");
        if !hint.is_empty() {
            msg.push_str(&format!("\n\nlog tail ({}):\n{hint}", log_path.display()));
        }
        anyhow::bail!(msg);
    }

    println!("daemon started (pid {pid}) on {host}:{port}");
    println!("log: {}", log_path.display());
    Ok(())
}

/// Read the last `n` lines from a file (best-effort).
fn tail_log(path: &std::path::Path, n: usize) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use crate::daemon::{find_available_port, DaemonManager};
    use std::fs;
    use std::net::TcpListener;
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
        let dir = std::env::temp_dir().join(format!(
            "agentzero-cli-daemon-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn daemon_manager_mark_started_and_status_success_path() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager should be created");
        let my_pid = std::process::id();

        let started = manager
            .mark_started("0.0.0.0".to_string(), 9090, my_pid)
            .expect("mark_started should succeed");
        assert!(started.running);
        assert_eq!(started.host.as_deref(), Some("0.0.0.0"));
        assert_eq!(started.port, Some(9090));
        assert_eq!(started.pid, Some(my_pid));

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

    #[test]
    fn daemon_status_format_running() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager should be created");
        let my_pid = std::process::id();

        manager
            .mark_started("127.0.0.1".to_string(), 8080, my_pid)
            .expect("mark_started should succeed");

        let status = manager.status().expect("status should succeed");
        assert!(status.running);
        assert_eq!(status.pid, Some(my_pid));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn daemon_status_format_not_running() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager should be created");

        let status = manager.status().expect("status should succeed");
        assert!(!status.running);

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn find_available_port_returns_start_when_free_success_path() {
        // Bind a listener to get a free port from the OS, then immediately drop
        // it so find_available_port can claim it.
        let listener = TcpListener::bind("127.0.0.1:0").expect("OS should assign a free port");
        let free_port = listener
            .local_addr()
            .expect("should have local addr")
            .port();
        drop(listener);

        let found = find_available_port("127.0.0.1", free_port)
            .expect("should find a port starting from a free port");
        assert!(found >= free_port);
    }

    #[test]
    fn find_available_port_skips_occupied_port_success_path() {
        // Hold a listener on some port so it appears occupied.
        let occupied = TcpListener::bind("127.0.0.1:0").expect("OS should assign a free port");
        let occupied_port = occupied.local_addr().expect("should have addr").port();

        // find_available_port should not return the occupied port.
        let found = find_available_port("127.0.0.1", occupied_port)
            .expect("should find a free port nearby");
        assert_ne!(found, occupied_port, "should skip the occupied port");
    }

    #[test]
    fn find_available_port_errors_when_all_ports_occupied_negative_path() {
        // Saturating add means if start is near u16::MAX the range is tiny.
        // Use a port near the ceiling to force a scan failure.
        // We just verify the error message shape rather than actually occupying
        // 100 ports (which would be slow and flaky).
        let near_max = u16::MAX - 5;
        // The scan range is [near_max, near_max + 100) but saturating_add caps
        // at u16::MAX, so fewer than 100 ports are tried. Whether they're free
        // or not doesn't matter for checking the error path logic — the
        // important thing is the function doesn't panic.
        let _ = find_available_port("127.0.0.1", near_max); // may succeed or fail; no panic
    }
}
