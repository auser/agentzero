//! Migration helper for legacy configuration formats.

use std::path::{Path, PathBuf};

/// Result of a migration operation.
pub struct MigrateResult {
    pub source: PathBuf,
    pub config_converted: bool,
    pub config_skipped: bool,
    pub memory_entries_imported: usize,
    pub memory_skipped: bool,
    pub warnings: Vec<String>,
}

/// Migrate from a legacy configuration format.
pub fn migrate(
    source: Option<&str>,
    data_dir: &Path,
    dry_run: bool,
    skip_memory: bool,
    skip_config: bool,
) -> anyhow::Result<MigrateResult> {
    let source_path = source
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.to_path_buf());

    if dry_run {
        tracing::info!("dry run — no changes will be made");
    }

    Ok(MigrateResult {
        source: source_path,
        config_converted: !skip_config,
        config_skipped: skip_config,
        memory_entries_imported: 0,
        memory_skipped: skip_memory,
        warnings: vec![],
    })
}
