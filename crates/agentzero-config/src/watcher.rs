use crate::loader;
use crate::model::AgentZeroConfig;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::sync::watch;

/// Watches a config file for changes and broadcasts new configs via a watch channel.
///
/// Uses `tokio::fs::metadata()` polling to detect modification time changes.
/// When a change is detected, the file is re-read, parsed, and validated.
/// Only valid configs are broadcast; invalid files log a warning and are skipped.
pub struct ConfigWatcher {
    config_path: PathBuf,
    poll_interval: Duration,
    tx: watch::Sender<AgentZeroConfig>,
    rx: watch::Receiver<AgentZeroConfig>,
}

impl ConfigWatcher {
    /// Create a new watcher for the given config file.
    ///
    /// Loads the initial config immediately and makes it available via `subscribe()`.
    pub fn new(config_path: PathBuf, poll_interval: Duration) -> anyhow::Result<Self> {
        let initial = loader::load(&config_path)?;
        let (tx, rx) = watch::channel(initial);
        Ok(Self {
            config_path,
            poll_interval,
            tx,
            rx,
        })
    }

    /// Create a watcher from an already-loaded config (avoids double-load at startup).
    pub fn from_config(
        config_path: PathBuf,
        poll_interval: Duration,
        config: AgentZeroConfig,
    ) -> Self {
        let (tx, rx) = watch::channel(config);
        Self {
            config_path,
            poll_interval,
            tx,
            rx,
        }
    }

    /// Get a receiver to subscribe to config changes.
    pub fn subscribe(&self) -> watch::Receiver<AgentZeroConfig> {
        self.rx.clone()
    }

    /// Get the current config snapshot.
    pub fn current(&self) -> AgentZeroConfig {
        self.rx.borrow().clone()
    }

    /// Run the polling loop. This is a long-running task — spawn it with `tokio::spawn`.
    ///
    /// The loop runs until the sender is dropped (all receivers dropped) or the token
    /// signals cancellation.
    pub async fn run(self, cancel: tokio::sync::watch::Receiver<bool>) {
        let mut last_modified = file_mtime(&self.config_path).await;
        let mut cancel = cancel;

        loop {
            tokio::select! {
                _ = tokio::time::sleep(self.poll_interval) => {}
                result = cancel.changed() => {
                    if result.is_err() || *cancel.borrow() {
                        tracing::debug!("config watcher shutting down");
                        return;
                    }
                }
            }

            let current_mtime = file_mtime(&self.config_path).await;
            if current_mtime == last_modified {
                continue;
            }

            tracing::info!(path = %self.config_path.display(), "config file changed, reloading");
            last_modified = current_mtime;

            match loader::load(&self.config_path) {
                Ok(new_config) => {
                    if self.tx.send(new_config).is_err() {
                        tracing::debug!("all config subscribers dropped, stopping watcher");
                        return;
                    }
                    tracing::info!("config reloaded successfully");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "config reload failed, keeping previous config");
                }
            }
        }
    }
}

async fn file_mtime(path: &Path) -> Option<SystemTime> {
    tokio::fs::metadata(path)
        .await
        .ok()
        .and_then(|m| m.modified().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn minimal_config_toml() -> &'static str {
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n"
    }

    fn temp_dir() -> PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-watcher-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn from_config_provides_initial_value() {
        let config = AgentZeroConfig::default();
        let watcher = ConfigWatcher::from_config(
            PathBuf::from("/tmp/fake.toml"),
            Duration::from_secs(5),
            config.clone(),
        );
        let current = watcher.current();
        assert_eq!(current.provider.kind, config.provider.kind);
    }

    #[test]
    fn subscribe_returns_cloned_receiver() {
        let config = AgentZeroConfig::default();
        let watcher = ConfigWatcher::from_config(
            PathBuf::from("/tmp/fake.toml"),
            Duration::from_secs(5),
            config,
        );
        let rx1 = watcher.subscribe();
        let rx2 = watcher.subscribe();
        assert_eq!(rx1.borrow().provider.kind, rx2.borrow().provider.kind);
    }

    #[tokio::test]
    async fn detects_config_change() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, minimal_config_toml()).unwrap();

        let watcher = ConfigWatcher::new(config_path.clone(), Duration::from_millis(50)).unwrap();
        let mut rx = watcher.subscribe();
        let (cancel_tx, cancel_rx) = watch::channel(false);

        let handle = tokio::spawn(watcher.run(cancel_rx));

        // Modify the file
        tokio::time::sleep(Duration::from_millis(100)).await;
        fs::write(
            &config_path,
            "[provider]\nkind = \"anthropic\"\nmodel = \"claude-3\"\n\n[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n",
        )
        .unwrap();

        // Wait for change notification
        let changed = tokio::time::timeout(Duration::from_secs(2), rx.changed()).await;
        assert!(changed.is_ok(), "should receive change notification");
        assert_eq!(rx.borrow().provider.kind, "anthropic");

        // Shutdown
        let _ = cancel_tx.send(true);
        let _ = handle.await;
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn skips_invalid_config_change() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        // Start with a known provider kind
        fs::write(
            &config_path,
            "[provider]\nkind = \"openai\"\nmodel = \"gpt-4o\"\n\n[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n",
        )
        .unwrap();

        let watcher = ConfigWatcher::new(config_path.clone(), Duration::from_millis(50)).unwrap();
        let rx = watcher.subscribe();
        let (cancel_tx, cancel_rx) = watch::channel(false);

        let handle = tokio::spawn(watcher.run(cancel_rx));

        // Write syntactically invalid TOML that cannot be parsed at all.
        // Using broken TOML ensures no environment variable override can rescue it.
        tokio::time::sleep(Duration::from_millis(100)).await;
        fs::write(&config_path, "[provider\nkind = broken toml ~~~\n").unwrap();

        // Give it time to detect and skip
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Config should still have original provider kind
        assert_eq!(rx.borrow().provider.kind, "openai");

        let _ = cancel_tx.send(true);
        let _ = handle.await;
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn stops_on_cancel_signal() {
        let config = AgentZeroConfig::default();
        let watcher = ConfigWatcher::from_config(
            PathBuf::from("/tmp/nonexistent.toml"),
            Duration::from_millis(50),
            config,
        );
        let (cancel_tx, cancel_rx) = watch::channel(false);

        let handle = tokio::spawn(watcher.run(cancel_rx));

        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = cancel_tx.send(true);

        let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
        assert!(result.is_ok(), "watcher should stop within timeout");
    }
}
