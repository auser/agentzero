//! Regression detection for multi-agent file modification conflicts.
//!
//! Tracks which files each agent modifies within a delegation tree (identified
//! by `correlation_id`) and detects when two different agents modify the same
//! file, which may indicate one agent undoing another's work.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single recorded file modification.
#[derive(Debug, Clone)]
pub struct FileModEntry {
    pub agent_id: String,
    pub run_id: String,
    pub correlation_id: String,
    pub timestamp_ms: u64,
}

/// Warning raised when a file conflict is detected.
#[derive(Debug, Clone)]
pub struct RegressionWarning {
    pub file_path: String,
    pub conflicting_entries: Vec<(String, String)>, // (agent_id, run_id)
    pub correlation_id: String,
}

/// Tracks file modifications across agents and detects conflicts.
///
/// A conflict occurs when two different agents modify the same file within the
/// same correlation tree (delegation chain) within a configurable time window.
pub struct FileModificationTracker {
    /// Maps canonical file paths to modification entries.
    modifications: Mutex<HashMap<String, Vec<FileModEntry>>>,
    /// Conflict detection window in milliseconds.
    window_ms: u64,
}

impl FileModificationTracker {
    /// Create a new tracker with the given conflict detection window.
    pub fn new(window_ms: u64) -> Self {
        Self {
            modifications: Mutex::new(HashMap::new()),
            window_ms,
        }
    }

    /// Record a file modification and check for conflicts.
    ///
    /// Returns `Some(RegressionWarning)` if another agent in the same
    /// correlation tree modified this file within the detection window.
    pub fn record_modification(
        &self,
        file_path: &str,
        agent_id: &str,
        run_id: &str,
        correlation_id: &str,
    ) -> Option<RegressionWarning> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let entry = FileModEntry {
            agent_id: agent_id.to_string(),
            run_id: run_id.to_string(),
            correlation_id: correlation_id.to_string(),
            timestamp_ms: now_ms,
        };

        let mut mods = self.modifications.lock().expect("regression tracker lock");
        let entries = mods.entry(file_path.to_string()).or_default();

        // Check for conflicts: same file, same correlation tree, different agent,
        // within the time window.
        let cutoff = now_ms.saturating_sub(self.window_ms);
        let conflicting: Vec<(String, String)> = entries
            .iter()
            .filter(|e| {
                e.correlation_id == correlation_id
                    && e.agent_id != agent_id
                    && e.timestamp_ms >= cutoff
            })
            .map(|e| (e.agent_id.clone(), e.run_id.clone()))
            .collect();

        entries.push(entry);

        if conflicting.is_empty() {
            None
        } else {
            Some(RegressionWarning {
                file_path: file_path.to_string(),
                conflicting_entries: conflicting,
                correlation_id: correlation_id.to_string(),
            })
        }
    }

    /// Remove entries older than the detection window.
    pub fn gc(&self) {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let cutoff = now_ms.saturating_sub(self.window_ms);

        let mut mods = self.modifications.lock().expect("regression tracker lock");
        mods.retain(|_, entries| {
            entries.retain(|e| e.timestamp_ms >= cutoff);
            !entries.is_empty()
        });
    }
}

impl std::fmt::Debug for FileModificationTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileModificationTracker")
            .field("window_ms", &self.window_ms)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_warning_for_different_files() {
        let tracker = FileModificationTracker::new(60_000);
        let result = tracker.record_modification("src/a.rs", "agent-1", "run-1", "corr-1");
        assert!(result.is_none());
        let result = tracker.record_modification("src/b.rs", "agent-2", "run-2", "corr-1");
        assert!(result.is_none());
    }

    #[test]
    fn warning_for_same_file_same_correlation() {
        let tracker = FileModificationTracker::new(60_000);
        let result = tracker.record_modification("src/main.rs", "agent-1", "run-1", "corr-1");
        assert!(result.is_none());
        let result = tracker.record_modification("src/main.rs", "agent-2", "run-2", "corr-1");
        assert!(result.is_some());
        let warning = result.expect("should have warning");
        assert_eq!(warning.file_path, "src/main.rs");
        assert_eq!(warning.correlation_id, "corr-1");
        assert_eq!(warning.conflicting_entries.len(), 1);
        assert_eq!(warning.conflicting_entries[0].0, "agent-1");
    }

    #[test]
    fn no_warning_for_different_correlation() {
        let tracker = FileModificationTracker::new(60_000);
        let result = tracker.record_modification("src/main.rs", "agent-1", "run-1", "corr-1");
        assert!(result.is_none());
        let result = tracker.record_modification("src/main.rs", "agent-2", "run-2", "corr-2");
        assert!(result.is_none());
    }

    #[test]
    fn no_warning_for_same_agent() {
        let tracker = FileModificationTracker::new(60_000);
        let result = tracker.record_modification("src/main.rs", "agent-1", "run-1", "corr-1");
        assert!(result.is_none());
        let result = tracker.record_modification("src/main.rs", "agent-1", "run-2", "corr-1");
        assert!(result.is_none());
    }

    #[test]
    fn gc_removes_expired_entries() {
        let tracker = FileModificationTracker::new(1); // 1ms window
        tracker.record_modification("src/a.rs", "agent-1", "run-1", "corr-1");
        // Sleep long enough for the entry to expire.
        std::thread::sleep(std::time::Duration::from_millis(5));
        tracker.gc();
        // After GC, recording same file from different agent should NOT conflict
        // (the old entry was cleaned up).
        let result = tracker.record_modification("src/a.rs", "agent-2", "run-2", "corr-1");
        assert!(result.is_none());
    }
}
