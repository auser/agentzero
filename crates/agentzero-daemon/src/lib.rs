use agentzero_storage::EncryptedJsonStore;
use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const STATE_FILE: &str = "daemon_state.json";
const PID_FILE: &str = "daemon.pid";
const DEFAULT_LOG_FILE: &str = "daemon.log";

/// Maximum log file size before rotation (10 MB).
const DEFAULT_MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
/// Maximum number of rotated log files to keep.
const DEFAULT_MAX_LOG_FILES: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DaemonStatus {
    pub running: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub pid: Option<u32>,
    pub started_at_epoch_seconds: Option<u64>,
}

impl DaemonStatus {
    /// Uptime in seconds since the daemon started, or 0 if not running.
    pub fn uptime_secs(&self) -> u64 {
        if !self.running {
            return 0;
        }
        self.started_at_epoch_seconds
            .map(|started| current_epoch_seconds().saturating_sub(started))
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Log rotation
// ---------------------------------------------------------------------------

/// Configuration for daemon log rotation.
#[derive(Debug, Clone)]
pub struct LogRotationConfig {
    /// Maximum size of the log file before rotation.
    pub max_bytes: u64,
    /// Maximum number of rotated log files to keep.
    pub max_files: usize,
}

impl Default for LogRotationConfig {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_LOG_BYTES,
            max_files: DEFAULT_MAX_LOG_FILES,
        }
    }
}

/// Rotate the daemon log file if it exceeds the configured size.
///
/// Rotation scheme: `daemon.log` → `daemon.log.1` → `daemon.log.2` → ...
/// Oldest files beyond `max_files` are deleted.
pub fn rotate_log_if_needed(data_dir: &Path, config: &LogRotationConfig) -> anyhow::Result<bool> {
    let log_path = data_dir.join(DEFAULT_LOG_FILE);
    if !log_path.exists() {
        return Ok(false);
    }

    let metadata = std::fs::metadata(&log_path)?;
    if metadata.len() < config.max_bytes {
        return Ok(false);
    }

    // Delete oldest rotated file if at capacity.
    for i in (1..=config.max_files).rev() {
        let old = rotated_path(&log_path, i);
        if i == config.max_files {
            let _ = std::fs::remove_file(&old);
        } else {
            let new = rotated_path(&log_path, i + 1);
            if old.exists() {
                std::fs::rename(&old, &new)?;
            }
        }
    }

    // Rotate current log to .1
    let rotated = rotated_path(&log_path, 1);
    std::fs::rename(&log_path, &rotated)?;

    Ok(true)
}

fn rotated_path(base: &Path, index: usize) -> PathBuf {
    let mut name = base.as_os_str().to_os_string();
    name.push(format!(".{index}"));
    PathBuf::from(name)
}

/// Returns the path to the daemon log file.
pub fn log_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join(DEFAULT_LOG_FILE)
}

// ---------------------------------------------------------------------------
// PID file management
// ---------------------------------------------------------------------------

/// Write the daemon PID to a file for external monitoring tools.
pub fn write_pid_file(data_dir: &Path, pid: u32) -> anyhow::Result<PathBuf> {
    let path = data_dir.join(PID_FILE);
    std::fs::write(&path, pid.to_string())?;
    Ok(path)
}

/// Read the PID from the PID file, if it exists.
pub fn read_pid_file(data_dir: &Path) -> Option<u32> {
    let path = data_dir.join(PID_FILE);
    std::fs::read_to_string(&path).ok()?.trim().parse().ok()
}

/// Remove the PID file on shutdown.
pub fn remove_pid_file(data_dir: &Path) {
    let path = data_dir.join(PID_FILE);
    let _ = std::fs::remove_file(path);
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

    /// Internal liveness check: returns structured health information about the
    /// daemon process including whether the PID is alive, uptime, and log size.
    pub fn health_check(&self, data_dir: &Path) -> anyhow::Result<DaemonHealth> {
        let status = self.status()?;
        let alive = status.running && status.pid.is_some_and(is_process_alive);
        let log_size = std::fs::metadata(log_file_path(data_dir))
            .ok()
            .map(|m| m.len());

        Ok(DaemonHealth {
            alive,
            pid: status.pid,
            uptime_secs: status.uptime_secs(),
            host: status.host,
            port: status.port,
            log_file_bytes: log_size,
        })
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

/// Structured health information returned by the daemon liveness check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonHealth {
    pub alive: bool,
    pub pid: Option<u32>,
    pub uptime_secs: u64,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub log_file_bytes: Option<u64>,
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
    use super::*;
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
            "agentzero-daemon-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
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

    // --- PID file tests ---

    #[test]
    fn pid_file_write_read_remove() {
        let dir = temp_dir();
        let pid = std::process::id();

        let path = write_pid_file(&dir, pid).expect("write should succeed");
        assert!(path.exists());
        assert_eq!(read_pid_file(&dir), Some(pid));

        remove_pid_file(&dir);
        assert!(!path.exists());
        assert_eq!(read_pid_file(&dir), None);

        fs::remove_dir_all(dir).ok();
    }

    // --- Log rotation tests ---

    #[test]
    fn rotate_log_noop_when_small() {
        let dir = temp_dir();
        let log = dir.join("daemon.log");
        fs::write(&log, "small log").expect("write should succeed");

        let config = LogRotationConfig {
            max_bytes: 1024,
            max_files: 3,
        };
        let rotated = rotate_log_if_needed(&dir, &config).expect("rotate should succeed");
        assert!(!rotated);
        assert!(log.exists());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rotate_log_rotates_large_file() {
        let dir = temp_dir();
        let log = dir.join("daemon.log");

        // Write a file larger than the threshold.
        let big_content = "x".repeat(200);
        fs::write(&log, &big_content).expect("write should succeed");

        let config = LogRotationConfig {
            max_bytes: 100,
            max_files: 3,
        };
        let rotated = rotate_log_if_needed(&dir, &config).expect("rotate should succeed");
        assert!(rotated);

        // Original should be gone; .1 should exist.
        assert!(!log.exists());
        let rotated_1 = dir.join("daemon.log.1");
        assert!(rotated_1.exists());
        assert_eq!(fs::read_to_string(&rotated_1).unwrap(), big_content);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rotate_log_cascades_existing_rotations() {
        let dir = temp_dir();
        let log = dir.join("daemon.log");
        let config = LogRotationConfig {
            max_bytes: 10,
            max_files: 3,
        };

        // Simulate existing rotated files.
        fs::write(dir.join("daemon.log.1"), "old-1").expect("write");
        fs::write(dir.join("daemon.log.2"), "old-2").expect("write");

        // Write current log exceeding threshold.
        fs::write(&log, "current-log-exceeds").expect("write");
        let rotated = rotate_log_if_needed(&dir, &config).expect("rotate should succeed");
        assert!(rotated);

        // .1 = current, .2 = old-1, .3 = old-2
        assert!(!log.exists());
        assert!(dir.join("daemon.log.1").exists());
        assert!(dir.join("daemon.log.2").exists());
        assert!(dir.join("daemon.log.3").exists());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rotate_log_deletes_oldest_beyond_max_files() {
        let dir = temp_dir();
        let log = dir.join("daemon.log");
        let config = LogRotationConfig {
            max_bytes: 10,
            max_files: 2,
        };

        // Create existing rotated files up to max.
        fs::write(dir.join("daemon.log.1"), "old-1").expect("write");
        fs::write(dir.join("daemon.log.2"), "should-be-deleted").expect("write");

        fs::write(&log, "current-big-enough").expect("write");
        rotate_log_if_needed(&dir, &config).expect("rotate should succeed");

        // .2 (the oldest at max_files) should have been deleted, then .1 renamed to .2
        assert!(dir.join("daemon.log.1").exists());
        assert!(dir.join("daemon.log.2").exists());
        let content = fs::read_to_string(dir.join("daemon.log.2")).unwrap();
        assert_eq!(content, "old-1"); // cascaded from .1

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rotate_log_noop_when_no_file() {
        let dir = temp_dir();
        let config = LogRotationConfig::default();
        let rotated = rotate_log_if_needed(&dir, &config).expect("rotate should succeed");
        assert!(!rotated);
        fs::remove_dir_all(dir).ok();
    }

    // --- DaemonStatus tests ---

    #[test]
    fn uptime_secs_when_running() {
        let status = DaemonStatus {
            running: true,
            started_at_epoch_seconds: Some(current_epoch_seconds().saturating_sub(120)),
            ..Default::default()
        };
        let uptime = status.uptime_secs();
        assert!((119..=121).contains(&uptime));
    }

    #[test]
    fn uptime_secs_when_not_running() {
        let status = DaemonStatus::default();
        assert_eq!(status.uptime_secs(), 0);
    }

    // --- Health check tests ---

    #[test]
    fn health_check_running_process() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager");
        let my_pid = std::process::id();

        manager
            .mark_started("0.0.0.0".to_string(), 8080, my_pid)
            .expect("start");

        // Create a log file so log_file_bytes is populated.
        fs::write(dir.join("daemon.log"), "some log output").expect("write log");

        let health = manager.health_check(&dir).expect("health_check");
        assert!(health.alive);
        assert_eq!(health.pid, Some(my_pid));
        assert!(health.uptime_secs < 5);
        assert_eq!(health.host.as_deref(), Some("0.0.0.0"));
        assert_eq!(health.port, Some(8080));
        assert!(health.log_file_bytes.is_some());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn health_check_dead_process() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager");

        let dead_pid = 4_000_000;
        manager
            .mark_started("127.0.0.1".to_string(), 9090, dead_pid)
            .expect("start");

        let health = manager.health_check(&dir).expect("health_check");
        assert!(!health.alive);
        assert_eq!(health.uptime_secs, 0);
        assert!(health.log_file_bytes.is_none());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn health_check_never_started() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager");

        let health = manager.health_check(&dir).expect("health_check");
        assert!(!health.alive);
        assert_eq!(health.pid, None);
        assert_eq!(health.uptime_secs, 0);

        fs::remove_dir_all(dir).ok();
    }
}
