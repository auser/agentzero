//! Hot-reload watcher for development plugins.
//!
//! Watches a directory (typically `$CWD/plugins/`) for `.wasm` file changes
//! and signals the caller via a channel when a plugin should be reloaded.
//! Only enabled behind the `plugin-dev` feature.

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

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
    pub fn start(dir: &Path) -> anyhow::Result<Self> {
        if !dir.exists() {
            anyhow::bail!("plugin watch directory does not exist: {}", dir.display());
        }

        let (tx, rx) = mpsc::channel::<PluginReloadEvent>();

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
                            let _ = tx.send(PluginReloadEvent {
                                wasm_path: path.clone(),
                            });
                        }
                    }
                }
                _ => {}
            }
        })?;

        watcher.watch(dir, RecursiveMode::Recursive)?;
        tracing::info!("plugin watcher started for {}", dir.display());

        Ok(Self {
            _watcher: watcher,
            receiver: rx,
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

        // Wait for event (with timeout)
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

        // Give it time to process
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
}
