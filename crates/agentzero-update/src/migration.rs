use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const MIGRATION_FILES: &[&str] = &[
    "agentzero.toml",
    "agentzero.db",
    "auth_profiles.json",
    "daemon_state.json",
    "gateway-paired-tokens.json",
    "heartbeat_state.json",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MigrationInspectResult {
    pub source: PathBuf,
    pub found_files: Vec<String>,
    pub missing_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MigrationImportResult {
    pub source: PathBuf,
    pub target: PathBuf,
    pub dry_run: bool,
    pub copied_files: Vec<String>,
    pub skipped_files: Vec<String>,
}

pub fn inspect_source(source: impl AsRef<Path>) -> anyhow::Result<MigrationInspectResult> {
    let source = source.as_ref();
    if !source.exists() {
        return Err(anyhow!(
            "migration source does not exist: {}",
            source.display()
        ));
    }
    if !source.is_dir() {
        return Err(anyhow!(
            "migration source must be a directory: {}",
            source.display()
        ));
    }

    let mut found_files = Vec::new();
    let mut missing_files = Vec::new();
    for file in MIGRATION_FILES {
        let path = source.join(file);
        if path.exists() {
            found_files.push((*file).to_string());
        } else {
            missing_files.push((*file).to_string());
        }
    }

    Ok(MigrationInspectResult {
        source: source.to_path_buf(),
        found_files,
        missing_files,
    })
}

pub fn import_from_source(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    dry_run: bool,
) -> anyhow::Result<MigrationImportResult> {
    let source = source.as_ref();
    let target = target.as_ref();

    let inspected = inspect_source(source)?;
    if inspected.found_files.is_empty() {
        return Err(anyhow!(
            "no known migration files found in source: {}",
            source.display()
        ));
    }

    if !dry_run {
        fs::create_dir_all(target)
            .with_context(|| format!("failed to create target directory {}", target.display()))?;
    }

    let mut copied_files = Vec::new();
    let mut skipped_files = Vec::new();

    for file in &inspected.found_files {
        let src = source.join(file);
        let dst = target.join(file);

        if dst.exists() {
            skipped_files.push(file.clone());
            continue;
        }

        if !dry_run {
            fs::copy(&src, &dst).with_context(|| {
                format!(
                    "failed to copy migration file {} -> {}",
                    src.display(),
                    dst.display()
                )
            })?;
        }
        copied_files.push(file.clone());
    }

    Ok(MigrationImportResult {
        source: source.to_path_buf(),
        target: target.to_path_buf(),
        dry_run,
        copied_files,
        skipped_files,
    })
}

#[cfg(test)]
mod tests {
    use super::{import_from_source, inspect_source};
    use std::fs;

    #[test]
    fn inspect_and_import_round_trip_success_path() {
        let src = tempfile::tempdir().expect("source dir should be created");
        let dst = tempfile::tempdir().expect("target dir should be created");

        fs::write(src.path().join("agentzero.toml"), "provider = \"openai\"\n")
            .expect("config should be written");
        fs::write(src.path().join("agentzero.db"), "db").expect("db should be written");

        let inspected = inspect_source(src.path()).expect("inspect should succeed");
        assert!(inspected.found_files.iter().any(|f| f == "agentzero.toml"));

        let dry_run_result =
            import_from_source(src.path(), dst.path(), true).expect("dry-run should succeed");
        assert!(dry_run_result
            .copied_files
            .iter()
            .any(|f| f == "agentzero.toml"));
        assert!(
            !dst.path().join("agentzero.toml").exists(),
            "dry-run should not write files"
        );

        let import_result =
            import_from_source(src.path(), dst.path(), false).expect("import should succeed");
        assert!(import_result
            .copied_files
            .iter()
            .any(|f| f == "agentzero.toml"));
        assert!(dst.path().join("agentzero.toml").exists());
        assert!(dst.path().join("agentzero.db").exists());
    }

    #[test]
    fn import_fails_for_missing_source_negative_path() {
        let dst = tempfile::tempdir().expect("target dir should be created");
        let missing = dst.path().join("missing-source");
        let err = import_from_source(&missing, dst.path(), false)
            .expect_err("missing source should fail");
        assert!(err.to_string().contains("does not exist"));
    }
}
