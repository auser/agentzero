use agentzero_storage::EncryptedJsonStore;
use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const STATE_FILE: &str = "daemon_state.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DaemonStatus {
    pub running: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub pid: Option<u32>,
    pub started_at_epoch_seconds: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DaemonManager {
    store: EncryptedJsonStore,
}

impl DaemonManager {
    pub fn new(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let store = EncryptedJsonStore::in_config_dir(data_dir.as_ref(), STATE_FILE)?;
        Ok(Self { store })
    }

    pub fn mark_started(&self, host: String, port: u16, pid: u32) -> anyhow::Result<DaemonStatus> {
        let mut status = self.store.load_or_default::<DaemonStatus>()?;
        // If state says running, verify the process is actually alive.
        if status.running {
            if let Some(old_pid) = status.pid {
                if is_process_alive(old_pid) {
                    bail!("daemon is already running (pid {old_pid})");
                }
                // Stale state from a crash — allow re-start.
            } else {
                bail!("daemon is already running");
            }
        }

        status.running = true;
        status.host = Some(host);
        status.port = Some(port);
        status.pid = Some(pid);
        status.started_at_epoch_seconds = Some(current_epoch_seconds());
        self.store.save(&status)?;
        Ok(status)
    }

    pub fn mark_stopped(&self) -> anyhow::Result<DaemonStatus> {
        let mut status = self.store.load_or_default::<DaemonStatus>()?;
        if !status.running {
            bail!("daemon is not running");
        }

        status.running = false;
        self.store.save(&status)?;
        Ok(status)
    }

    /// Returns the current daemon status, auto-correcting stale state if the
    /// process has died without marking itself as stopped.
    pub fn status(&self) -> anyhow::Result<DaemonStatus> {
        let mut status = self.store.load_or_default::<DaemonStatus>()?;
        if status.running {
            let alive = status.pid.is_some_and(is_process_alive);
            if !alive {
                status.running = false;
                // Best-effort persist the correction; ignore errors.
                let _ = self.store.save(&status);
            }
        }
        Ok(status)
    }

    /// Send SIGTERM to the daemon process and wait for it to exit (up to 5 seconds).
    pub fn stop_process(&self) -> anyhow::Result<()> {
        let status = self.store.load_or_default::<DaemonStatus>()?;
        if !status.running {
            bail!("daemon is not running");
        }
        let pid = status
            .pid
            .ok_or_else(|| anyhow::anyhow!("daemon state has no PID — stop it manually"))?;

        if !is_process_alive(pid) {
            // Already dead; just update state.
            self.mark_stopped()?;
            return Ok(());
        }

        // Send SIGTERM.
        send_signal(pid, libc::SIGTERM)?;

        // Wait up to 5 seconds for the process to exit.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if !is_process_alive(pid) {
                self.mark_stopped()?;
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        // Still alive — force kill.
        send_signal(pid, libc::SIGKILL)?;
        std::thread::sleep(Duration::from_millis(200));
        self.mark_stopped()?;
        Ok(())
    }
}

/// Check if a process with the given PID is alive using `kill(pid, 0)`.
pub fn is_process_alive(pid: u32) -> bool {
    // SAFETY: kill with signal 0 doesn't send a signal — it just checks existence.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn send_signal(pid: u32, signal: libc::c_int) -> anyhow::Result<()> {
    // SAFETY: Sending a signal to a known PID.
    let result = unsafe { libc::kill(pid as libc::pid_t, signal) };
    if result != 0 {
        let err = std::io::Error::last_os_error();
        bail!("failed to send signal {signal} to pid {pid}: {err}");
    }
    Ok(())
}

fn current_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{is_process_alive, DaemonManager};
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
        let dir = std::env::temp_dir().join(format!("agentzero-daemon-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn mark_started_then_stopped_success_path() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager should be created");
        let my_pid = std::process::id();

        let started = manager
            .mark_started("127.0.0.1".to_string(), 8080, my_pid)
            .expect("mark_started should succeed");
        assert!(started.running);
        assert_eq!(started.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(started.port, Some(8080));
        assert_eq!(started.pid, Some(my_pid));
        assert!(started.started_at_epoch_seconds.is_some());

        let stopped = manager.mark_stopped().expect("mark_stopped should succeed");
        assert!(!stopped.running);
        assert_eq!(stopped.host.as_deref(), Some("127.0.0.1"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn mark_started_rejects_double_start_negative_path() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager should be created");
        let my_pid = std::process::id();

        manager
            .mark_started("127.0.0.1".to_string(), 8080, my_pid)
            .expect("first start should succeed");

        let err = manager
            .mark_started("127.0.0.1".to_string(), 8080, my_pid)
            .expect_err("second start should fail");
        assert!(err.to_string().contains("already running"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn status_auto_corrects_stale_state() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager should be created");

        // Use a PID that almost certainly doesn't exist.
        let dead_pid = 4_000_000;
        manager
            .mark_started("127.0.0.1".to_string(), 8080, dead_pid)
            .expect("mark_started should succeed");

        // Status should detect the dead process and auto-correct.
        let status = manager.status().expect("status should succeed");
        assert!(
            !status.running,
            "stale state should be auto-corrected to not running"
        );

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn stale_state_allows_restart() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager should be created");
        let my_pid = std::process::id();

        // Start with a dead PID.
        let dead_pid = 4_000_000;
        manager
            .mark_started("127.0.0.1".to_string(), 8080, dead_pid)
            .expect("mark_started should succeed");

        // Should allow re-start because the old process is dead.
        manager
            .mark_started("127.0.0.1".to_string(), 9090, my_pid)
            .expect("restart after stale state should succeed");

        let status = manager.status().expect("status should succeed");
        assert!(status.running);
        assert_eq!(status.port, Some(9090));
        assert_eq!(status.pid, Some(my_pid));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn is_process_alive_for_current_process() {
        assert!(is_process_alive(std::process::id()));
    }

    #[test]
    fn is_process_alive_returns_false_for_dead_pid() {
        assert!(!is_process_alive(4_000_000));
    }
}
