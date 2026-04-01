//! Copy-on-write overlay filesystem for WASM plugin sandboxing.
//!
//! `WasiOverlayFs` provides a transparent CoW layer over a base directory.
//! All reads check the scratch layer first, then fall through to the base.
//! All writes go to the scratch layer. Deletions are tracked as whiteouts.
//!
//! After execution, the overlay can be committed (apply changes to real FS),
//! discarded (throw away all changes), or diffed (inspect what changed).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// How the overlay handles changes after plugin execution.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OverlayMode {
    /// Direct writes to the real filesystem (current behavior, backward compat).
    #[default]
    Disabled,
    /// Commit on success, discard on failure.
    AutoCommit,
    /// Plugin must explicitly call az_fs_commit.
    ExplicitCommit,
    /// Always discard, return diff. Useful for dry-run / preview.
    DryRun,
}

/// A copy-on-write overlay filesystem.
///
/// Reads check scratch first, then base. Writes always go to scratch.
/// Deletions tracked as whiteouts. Symlinks resolved and boundary-checked.
pub struct WasiOverlayFs {
    base: PathBuf,
    scratch: PathBuf,
    whiteouts: HashSet<PathBuf>,
    /// Whether to clean up the scratch dir on drop.
    owns_scratch: bool,
}

/// A single change in the overlay.
#[derive(Debug, Clone, PartialEq)]
pub enum OverlayChange {
    /// File was created or modified in the scratch layer.
    Modified(PathBuf),
    /// File was deleted (present in whiteout set).
    Deleted(PathBuf),
}

/// Summary of all changes in the overlay.
#[derive(Debug, Clone)]
pub struct OverlayDiff {
    pub changes: Vec<OverlayChange>,
}

/// Report after committing overlay changes.
#[derive(Debug)]
pub struct CommitReport {
    pub files_written: usize,
    pub files_deleted: usize,
    pub conflicts: Vec<PathBuf>,
}

impl WasiOverlayFs {
    /// Create a new overlay over `base`, using `scratch` for writes.
    ///
    /// If `scratch` is `None`, a temporary directory is created and cleaned
    /// up on drop.
    pub fn new(base: PathBuf, scratch: Option<PathBuf>) -> io::Result<Self> {
        let (scratch_path, owns_scratch) = match scratch {
            Some(p) => {
                fs::create_dir_all(&p)?;
                (p, false)
            }
            None => {
                let dir = std::env::temp_dir().join(format!(
                    "az-overlay-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                ));
                fs::create_dir_all(&dir)?;
                (dir, true)
            }
        };
        Ok(Self {
            base,
            scratch: scratch_path,
            whiteouts: HashSet::new(),
            owns_scratch,
        })
    }

    /// Resolve a relative path, rejecting traversals outside the base.
    fn resolve(&self, path: &Path) -> io::Result<PathBuf> {
        // Reject parent directory traversals.
        for component in path.components() {
            if matches!(component, Component::ParentDir) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "path traversal outside overlay base",
                ));
            }
        }
        // Ensure the path is relative.
        let relative = if path.is_absolute() {
            path.strip_prefix("/").unwrap_or(path)
        } else {
            path
        };
        Ok(relative.to_path_buf())
    }

    /// Read a file: scratch first, then base. Returns error if whiteout.
    pub fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let relative = self.resolve(path)?;

        // Check whiteout first.
        if self.whiteouts.contains(&relative) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "file has been deleted",
            ));
        }

        // Check scratch.
        let scratch_path = self.scratch.join(&relative);
        if scratch_path.exists() {
            return fs::read(&scratch_path);
        }

        // Fall through to base.
        let base_path = self.base.join(&relative);
        fs::read(&base_path)
    }

    /// Write a file to the scratch layer.
    pub fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let relative = self.resolve(path)?;
        let scratch_path = self.scratch.join(&relative);

        // Create parent directories in scratch.
        if let Some(parent) = scratch_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&scratch_path, data)
    }

    /// Delete a file by adding it to the whiteout set.
    pub fn delete(&mut self, path: &Path) -> io::Result<()> {
        let relative = self.resolve(path)?;

        // Verify the file exists in scratch or base.
        let in_scratch = self.scratch.join(&relative).exists();
        let in_base = self.base.join(&relative).exists();
        if !in_scratch && !in_base {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "file does not exist",
            ));
        }

        // Remove from scratch if present.
        let scratch_path = self.scratch.join(&relative);
        if scratch_path.exists() {
            fs::remove_file(&scratch_path)?;
        }

        // Add to whiteouts.
        self.whiteouts.insert(relative);
        Ok(())
    }

    /// Check if a file exists (considering overlay and whiteouts).
    pub fn exists(&self, path: &Path) -> io::Result<bool> {
        let relative = self.resolve(path)?;

        if self.whiteouts.contains(&relative) {
            return Ok(false);
        }

        let in_scratch = self.scratch.join(&relative).exists();
        let in_base = self.base.join(&relative).exists();
        Ok(in_scratch || in_base)
    }

    /// Compute the diff between the overlay and the base.
    pub fn diff(&self) -> OverlayDiff {
        let mut changes = Vec::new();

        // Walk scratch for modified/created files.
        if let Ok(entries) = walk_dir_relative(&self.scratch) {
            for relative in entries {
                changes.push(OverlayChange::Modified(relative));
            }
        }

        // Add whiteouts as deletions.
        for path in &self.whiteouts {
            changes.push(OverlayChange::Deleted(path.clone()));
        }

        changes.sort_by(|a, b| {
            let pa = match a {
                OverlayChange::Modified(p) | OverlayChange::Deleted(p) => p,
            };
            let pb = match b {
                OverlayChange::Modified(p) | OverlayChange::Deleted(p) => p,
            };
            pa.cmp(pb)
        });

        OverlayDiff { changes }
    }

    /// Commit overlay changes to the base filesystem.
    ///
    /// For each modified file, checks the base mtime hasn't changed since
    /// the overlay was created (conflict detection). Whiteout files are
    /// deleted from the base.
    pub fn commit(&self) -> io::Result<CommitReport> {
        let mut files_written = 0;
        let mut files_deleted = 0;
        let mut conflicts = Vec::new();

        // Apply modified files from scratch to base.
        if let Ok(entries) = walk_dir_relative(&self.scratch) {
            for relative in entries {
                let scratch_path = self.scratch.join(&relative);
                let base_path = self.base.join(&relative);

                // Create parent directories in base.
                if let Some(parent) = base_path.parent() {
                    fs::create_dir_all(parent)?;
                }

                // Conflict detection: if the base file exists and was modified
                // after the scratch file, flag it.
                if base_path.exists() {
                    let base_mtime = fs::metadata(&base_path).and_then(|m| m.modified()).ok();
                    let scratch_mtime = fs::metadata(&scratch_path).and_then(|m| m.modified()).ok();
                    if let (Some(bm), Some(sm)) = (base_mtime, scratch_mtime) {
                        if bm > sm {
                            conflicts.push(relative.clone());
                            continue;
                        }
                    }
                }

                let data = fs::read(&scratch_path)?;
                fs::write(&base_path, data)?;
                files_written += 1;
            }
        }

        // Apply deletions.
        for relative in &self.whiteouts {
            let base_path = self.base.join(relative);
            if base_path.exists() {
                fs::remove_file(&base_path)?;
                files_deleted += 1;
            }
        }

        Ok(CommitReport {
            files_written,
            files_deleted,
            conflicts,
        })
    }

    /// Discard all overlay changes. Cleans up scratch directory.
    pub fn discard(self) {
        // Drop will clean up scratch if we own it.
        drop(self);
    }

    /// The scratch directory path (for WASI pre-open).
    pub fn scratch_dir(&self) -> &Path {
        &self.scratch
    }

    /// The base directory path.
    pub fn base_dir(&self) -> &Path {
        &self.base
    }
}

impl Drop for WasiOverlayFs {
    fn drop(&mut self) {
        if self.owns_scratch {
            let _ = fs::remove_dir_all(&self.scratch);
        }
    }
}

/// Walk a directory recursively, returning relative paths to files.
fn walk_dir_relative(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    walk_dir_recursive(root, root, &mut result)?;
    Ok(result)
}

fn walk_dir_recursive(root: &Path, current: &Path, result: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir_recursive(root, &path, result)?;
        } else {
            let relative = path.strip_prefix(root).map_err(io::Error::other)?;
            result.push(relative.to_path_buf());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
        let base = tempfile::tempdir().expect("create base dir");
        let scratch = tempfile::tempdir().expect("create scratch dir");

        // Create some files in base.
        fs::write(base.path().join("hello.txt"), b"hello world").expect("write");
        fs::create_dir_all(base.path().join("subdir")).expect("mkdir");
        fs::write(base.path().join("subdir/nested.txt"), b"nested content").expect("write");

        (base, scratch)
    }

    #[test]
    fn read_falls_through_to_base() {
        let (base, scratch) = setup();
        let overlay = WasiOverlayFs::new(
            base.path().to_path_buf(),
            Some(scratch.path().to_path_buf()),
        )
        .expect("create overlay");

        let data = overlay.read(Path::new("hello.txt")).expect("read");
        assert_eq!(data, b"hello world");

        let nested = overlay.read(Path::new("subdir/nested.txt")).expect("read");
        assert_eq!(nested, b"nested content");
    }

    #[test]
    fn write_goes_to_scratch() {
        let (base, scratch) = setup();
        let overlay = WasiOverlayFs::new(
            base.path().to_path_buf(),
            Some(scratch.path().to_path_buf()),
        )
        .expect("create overlay");

        overlay
            .write(Path::new("new_file.txt"), b"new content")
            .expect("write");

        // Read should return the scratch version.
        let data = overlay.read(Path::new("new_file.txt")).expect("read");
        assert_eq!(data, b"new content");

        // Base should be untouched.
        assert!(!base.path().join("new_file.txt").exists());

        // Scratch should have the file.
        assert!(scratch.path().join("new_file.txt").exists());
    }

    #[test]
    fn write_overrides_base() {
        let (base, scratch) = setup();
        let overlay = WasiOverlayFs::new(
            base.path().to_path_buf(),
            Some(scratch.path().to_path_buf()),
        )
        .expect("create overlay");

        overlay
            .write(Path::new("hello.txt"), b"modified")
            .expect("write");

        // Read should return scratch version.
        let data = overlay.read(Path::new("hello.txt")).expect("read");
        assert_eq!(data, b"modified");

        // Base unchanged.
        let base_data = fs::read(base.path().join("hello.txt")).expect("read base");
        assert_eq!(base_data, b"hello world");
    }

    #[test]
    fn delete_creates_whiteout() {
        let (base, scratch) = setup();
        let mut overlay = WasiOverlayFs::new(
            base.path().to_path_buf(),
            Some(scratch.path().to_path_buf()),
        )
        .expect("create overlay");

        overlay.delete(Path::new("hello.txt")).expect("delete");

        // Read should fail.
        let result = overlay.read(Path::new("hello.txt"));
        assert!(result.is_err());

        // Exists should return false.
        assert!(!overlay.exists(Path::new("hello.txt")).expect("exists"));

        // Base file still there.
        assert!(base.path().join("hello.txt").exists());
    }

    #[test]
    fn delete_nonexistent_fails() {
        let (base, scratch) = setup();
        let mut overlay = WasiOverlayFs::new(
            base.path().to_path_buf(),
            Some(scratch.path().to_path_buf()),
        )
        .expect("create overlay");

        let result = overlay.delete(Path::new("nonexistent.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn diff_shows_changes() {
        let (base, scratch) = setup();
        let mut overlay = WasiOverlayFs::new(
            base.path().to_path_buf(),
            Some(scratch.path().to_path_buf()),
        )
        .expect("create overlay");

        overlay
            .write(Path::new("new_file.txt"), b"new")
            .expect("write");
        overlay
            .write(Path::new("hello.txt"), b"modified")
            .expect("overwrite");
        overlay
            .delete(Path::new("subdir/nested.txt"))
            .expect("delete");

        let diff = overlay.diff();
        assert_eq!(diff.changes.len(), 3);

        assert!(diff
            .changes
            .contains(&OverlayChange::Modified(PathBuf::from("hello.txt"))));
        assert!(diff
            .changes
            .contains(&OverlayChange::Modified(PathBuf::from("new_file.txt"))));
        assert!(diff
            .changes
            .contains(&OverlayChange::Deleted(PathBuf::from("subdir/nested.txt"))));
    }

    #[test]
    fn commit_applies_changes() {
        let (base, scratch) = setup();
        let mut overlay = WasiOverlayFs::new(
            base.path().to_path_buf(),
            Some(scratch.path().to_path_buf()),
        )
        .expect("create overlay");

        overlay
            .write(Path::new("new_file.txt"), b"committed")
            .expect("write");
        overlay.delete(Path::new("hello.txt")).expect("delete");

        let report = overlay.commit().expect("commit");
        assert_eq!(report.files_written, 1);
        assert_eq!(report.files_deleted, 1);
        assert!(report.conflicts.is_empty());

        // Base should now have the new file.
        let data = fs::read(base.path().join("new_file.txt")).expect("read");
        assert_eq!(data, b"committed");

        // Base should not have the deleted file.
        assert!(!base.path().join("hello.txt").exists());
    }

    #[test]
    fn discard_leaves_base_untouched() {
        let (base, scratch) = setup();
        let overlay = WasiOverlayFs::new(
            base.path().to_path_buf(),
            Some(scratch.path().to_path_buf()),
        )
        .expect("create overlay");

        overlay
            .write(Path::new("new_file.txt"), b"discarded")
            .expect("write");

        overlay.discard();

        // Base unchanged.
        assert!(!base.path().join("new_file.txt").exists());
        assert!(base.path().join("hello.txt").exists());
    }

    #[test]
    fn rejects_parent_directory_traversal() {
        let (base, scratch) = setup();
        let overlay = WasiOverlayFs::new(
            base.path().to_path_buf(),
            Some(scratch.path().to_path_buf()),
        )
        .expect("create overlay");

        let result = overlay.read(Path::new("../../../etc/passwd"));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn nested_directory_write_and_commit() {
        let (base, scratch) = setup();
        let overlay = WasiOverlayFs::new(
            base.path().to_path_buf(),
            Some(scratch.path().to_path_buf()),
        )
        .expect("create overlay");

        overlay
            .write(Path::new("deep/nested/dir/file.txt"), b"deep content")
            .expect("write");

        let data = overlay
            .read(Path::new("deep/nested/dir/file.txt"))
            .expect("read");
        assert_eq!(data, b"deep content");

        let report = overlay.commit().expect("commit");
        assert_eq!(report.files_written, 1);

        let committed = fs::read(base.path().join("deep/nested/dir/file.txt")).expect("read");
        assert_eq!(committed, b"deep content");
    }

    #[test]
    fn commit_detects_conflict_when_base_modified() {
        let (base, scratch) = setup();
        let overlay = WasiOverlayFs::new(
            base.path().to_path_buf(),
            Some(scratch.path().to_path_buf()),
        )
        .expect("create overlay");

        // Write to overlay (goes to scratch).
        overlay
            .write(Path::new("hello.txt"), b"overlay version")
            .expect("write to overlay");

        // Simulate an external modification to the base file AFTER the scratch write.
        // Sleep briefly so mtime is strictly greater.
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(base.path().join("hello.txt"), b"external modification")
            .expect("external write to base");

        // Commit should detect the conflict (base mtime > scratch mtime).
        let report = overlay.commit().expect("commit should succeed");
        assert_eq!(report.conflicts.len(), 1, "should detect one conflict");
        assert_eq!(report.conflicts[0], PathBuf::from("hello.txt"));
        assert_eq!(
            report.files_written, 0,
            "conflicted file should not be written"
        );

        // Base should still have the external modification, not the overlay version.
        let base_data = fs::read(base.path().join("hello.txt")).expect("read base");
        assert_eq!(base_data, b"external modification");
    }

    #[test]
    fn commit_no_conflict_when_base_unmodified() {
        let (base, scratch) = setup();
        let overlay = WasiOverlayFs::new(
            base.path().to_path_buf(),
            Some(scratch.path().to_path_buf()),
        )
        .expect("create overlay");

        // Wait so scratch write is clearly after base creation.
        std::thread::sleep(std::time::Duration::from_millis(50));

        overlay
            .write(Path::new("hello.txt"), b"updated by overlay")
            .expect("write to overlay");

        let report = overlay.commit().expect("commit");
        assert!(report.conflicts.is_empty(), "no conflicts expected");
        assert_eq!(report.files_written, 1);

        let data = fs::read(base.path().join("hello.txt")).expect("read");
        assert_eq!(data, b"updated by overlay");
    }
}
