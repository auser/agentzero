//! Hot-reload watcher for development plugins.
//!
//! Watches a directory (typically `$CWD/plugins/`) for `.wasm` file changes
//! and signals the caller via a channel when a plugin should be reloaded.
//! Only enabled behind the `plugin-dev` feature.
//!
//! Events are debounced: rapid filesystem events for the same `.wasm` file
//! are coalesced into a single reload event after a quiet period.

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Default debounce window — events for the same path within this window
/// are coalesced into a single reload event.
const DEFAULT_DEBOUNCE_MS: u64 = 200;

/// Describes a hot-reload event for a changed plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginReloadEvent {
    /// Path to the `.wasm` file that changed.
    pub wasm_path: PathBuf,
}

/// A handle to a running plugin watcher.
///
/// When dropped, the watcher thread is stopped.
pub struct PluginWatcher {
    _watcher: RecommendedWatcher,
    receiver: mpsc::Receiver<PluginReloadEvent>,
}

impl std::fmt::Debug for PluginWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginWatcher")
            .field("active", &true)
            .finish()
    }
}

impl PluginWatcher {
    /// Start watching `dir` for `.wasm` file create/modify events.
    ///
    /// Returns a `PluginWatcher` whose `recv()` method yields reload events.
    /// The watcher runs on a background OS thread managed by the `notify` crate.
    /// Events are debounced with `debounce_ms` (defaults to 200ms).
    pub fn start(dir: &Path) -> anyhow::Result<Self> {
        Self::start_with_debounce(dir, Duration::from_millis(DEFAULT_DEBOUNCE_MS))
    }

    /// Start watching with a custom debounce duration.
    pub fn start_with_debounce(dir: &Path, debounce: Duration) -> anyhow::Result<Self> {
        if !dir.exists() {
            anyhow::bail!("plugin watch directory does not exist: {}", dir.display());
        }

        // Raw events from notify go into this channel.
        let (raw_tx, raw_rx) = mpsc::channel::<PathBuf>();
        // Debounced events go to the consumer.
        let (debounced_tx, debounced_rx) = mpsc::channel::<PluginReloadEvent>();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
            let event = match res {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("plugin watcher error: {e}");
                    return;
                }
            };

            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    for path in &event.paths {
                        if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
                            let _ = raw_tx.send(path.clone());
                        }
                    }
                }
                _ => {}
            }
        })?;

        watcher.watch(dir, RecursiveMode::Recursive)?;
        tracing::info!("plugin watcher started for {}", dir.display());

        // Debounce thread: collects raw events and emits debounced events.
        std::thread::Builder::new()
            .name("plugin-watcher-debounce".into())
            .spawn(move || {
                let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
                loop {
                    // Wait for new raw events or check pending timers.
                    let next_deadline = pending.values().min().copied();
                    let wait = next_deadline
                        .map(|deadline| {
                            deadline
                                .checked_duration_since(Instant::now())
                                .unwrap_or(Duration::ZERO)
                        })
                        .unwrap_or(Duration::from_secs(1));

                    match raw_rx.recv_timeout(wait) {
                        Ok(path) => {
                            // Record/reset the debounce timer for this path.
                            pending.insert(path, Instant::now() + debounce);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            // Check for expired timers.
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            // Watcher dropped — flush any pending and exit.
                            for (path, _) in pending.drain() {
                                let _ = debounced_tx.send(PluginReloadEvent { wasm_path: path });
                            }
                            return;
                        }
                    }

                    // Emit events whose debounce window has elapsed.
                    let now = Instant::now();
                    let expired: Vec<PathBuf> = pending
                        .iter()
                        .filter(|(_, deadline)| now >= **deadline)
                        .map(|(path, _)| path.clone())
                        .collect();
                    for path in expired {
                        pending.remove(&path);
                        if debounced_tx
                            .send(PluginReloadEvent { wasm_path: path })
                            .is_err()
                        {
                            return; // Consumer dropped
                        }
                    }
                }
            })?;

        Ok(Self {
            _watcher: watcher,
            receiver: debounced_rx,
        })
    }

    /// Try to receive a reload event without blocking.
    pub fn try_recv(&self) -> Option<PluginReloadEvent> {
        self.receiver.try_recv().ok()
    }

    /// Block until a reload event arrives or the timeout expires.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<PluginReloadEvent> {
        self.receiver.recv_timeout(timeout).ok()
    }

    /// Drain all pending reload events.
    pub fn drain(&self) -> Vec<PluginReloadEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.receiver.try_recv() {
            events.push(event);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn watcher_rejects_missing_directory() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let missing = tmp.path().join("nonexistent");
        let err = PluginWatcher::start(&missing).expect_err("missing dir should fail");
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn watcher_detects_wasm_file_creation() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let watch_dir = tmp.path().join("plugins");
        fs::create_dir_all(&watch_dir).expect("dir should be created");

        let watcher = PluginWatcher::start(&watch_dir).expect("watcher should start");

        // Write a .wasm file
        let wasm_path = watch_dir.join("test.wasm");
        fs::write(&wasm_path, b"\0asm\x01\0\0\0").expect("write wasm");

        // Wait for event (with timeout, accounting for debounce window)
        let event = watcher.recv_timeout(Duration::from_secs(5));
        assert!(event.is_some(), "watcher should detect .wasm creation");
        // Canonicalize both paths to handle macOS /private/var vs /var symlink
        let expected = wasm_path.canonicalize().unwrap_or(wasm_path);
        let actual_path = event.unwrap().wasm_path;
        let actual = actual_path.canonicalize().unwrap_or(actual_path);
        assert_eq!(actual, expected);
    }

    #[test]
    fn watcher_ignores_non_wasm_files() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let watch_dir = tmp.path().join("plugins");
        fs::create_dir_all(&watch_dir).expect("dir should be created");

        let watcher = PluginWatcher::start(&watch_dir).expect("watcher should start");

        // Write a non-wasm file
        fs::write(watch_dir.join("readme.txt"), b"hello").expect("write txt");

        // Give it time to process (longer than debounce window)
        std::thread::sleep(Duration::from_millis(500));
        let events = watcher.drain();
        assert!(
            events.is_empty(),
            "watcher should not report non-wasm files"
        );
    }

    #[test]
    fn watcher_detects_wasm_modification() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let watch_dir = tmp.path().join("plugins");
        fs::create_dir_all(&watch_dir).expect("dir should be created");

        let wasm_path = watch_dir.join("test.wasm");
        fs::write(&wasm_path, b"\0asm\x01\0\0\0").expect("initial write");

        // Start watching after initial file exists
        let watcher = PluginWatcher::start(&watch_dir).expect("watcher should start");

        // Small delay then modify
        std::thread::sleep(Duration::from_millis(100));
        fs::write(&wasm_path, b"\0asm\x01\0\0\0\x01").expect("modify wasm");

        let event = watcher.recv_timeout(Duration::from_secs(5));
        assert!(event.is_some(), "watcher should detect .wasm modification");
    }

    #[test]
    fn watcher_debounces_rapid_writes() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let watch_dir = tmp.path().join("plugins");
        fs::create_dir_all(&watch_dir).expect("dir should be created");

        // Use a short debounce for faster testing
        let watcher = PluginWatcher::start_with_debounce(&watch_dir, Duration::from_millis(300))
            .expect("watcher should start");

        let wasm_path = watch_dir.join("rapid.wasm");

        // Write the same file 5 times in quick succession
        for i in 0..5 {
            let content = format!("\0asm\x01\0\0\0{}", i);
            fs::write(&wasm_path, content.as_bytes()).expect("write wasm");
            std::thread::sleep(Duration::from_millis(20));
        }

        // Wait for debounce window to close plus processing time
        std::thread::sleep(Duration::from_millis(600));

        // Should produce exactly 1 debounced event, not 5
        let events = watcher.drain();
        assert_eq!(
            events.len(),
            1,
            "rapid writes should be debounced into a single event, got {}",
            events.len()
        );
    }
}
