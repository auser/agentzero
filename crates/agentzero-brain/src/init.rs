use crate::{validate_path, BrainConfig, BrainError, BrainFs};
use std::path::Path;

/// Result of an init operation.
#[derive(Debug, Default)]
pub struct InitResult {
    pub created: Vec<String>,
    pub skipped: Vec<String>,
    pub updated: Vec<String>,
}

impl InitResult {
    pub fn summary(&self) -> String {
        format!(
            "Created: {}, Skipped: {}, Updated: {}",
            self.created.len(),
            self.skipped.len(),
            self.updated.len()
        )
    }
}

/// Options for brain init.
#[derive(Default)]
pub struct InitOptions {
    pub force: bool,
    pub dry_run: bool,
}

/// Directories to create during init.
const DIRS: &[&str] = &[
    "raw/inbox",
    "raw/sources",
    "raw/assets",
    "wiki/daily",
    "wiki/weekly",
    "wiki/projects",
    "wiki/areas",
    "wiki/decisions",
    "wiki/people",
    "wiki/sources",
    "wiki/reports",
    "wiki/archive",
    "prompts/claude",
    "prompts/maintenance",
    "prompts/workflows",
    "templates",
];

/// Run brain init.
pub fn brain_init(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
    opts: &InitOptions,
) -> Result<InitResult, BrainError> {
    validate_path(root)?;
    let mut result = InitResult::default();

    // Create directories
    for dir in DIRS {
        let path = format!("{root}/{dir}");
        if opts.dry_run {
            result.created.push(format!("{dir}/"));
        } else {
            fs.create_dir(&path).map_err(BrainError::Io)?;
        }
    }

    // Get current date from the filesystem abstraction (supports WASM host clock)
    let now_raw = fs.now();
    let now_date = extract_date_from_iso(&now_raw, &config.daily.date_format);

    // Create starter files
    let files = starter_files(config, &now_date)?;
    for (rel_path, content) in &files {
        let full_path = format!("{root}/{rel_path}");
        let exists = if opts.dry_run {
            false
        } else {
            fs.file_exists(&full_path).map_err(BrainError::Io)?
        };

        if opts.dry_run {
            result.created.push(rel_path.to_string());
        } else if exists && !opts.force {
            result.skipped.push(rel_path.to_string());
        } else {
            // Ensure parent dir exists
            if let Some(parent) = Path::new(rel_path).parent() {
                if !parent.as_os_str().is_empty() {
                    let parent_path = format!("{root}/{}", parent.display());
                    fs.create_dir(&parent_path).map_err(BrainError::Io)?;
                }
            }
            fs.write_file(&full_path, content)
                .map_err(BrainError::Io)?;
            if exists {
                result.updated.push(rel_path.to_string());
            } else {
                result.created.push(rel_path.to_string());
            }
        }
    }

    Ok(result)
}

/// Extract a formatted date from an ISO 8601 string, using the config format.
fn extract_date_from_iso(iso: &str, format: &str) -> String {
    // Try parsing as full ISO 8601 datetime
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(
        iso.split_once('+')
            .or_else(|| iso.split_once('-').filter(|(pre, _)| pre.contains('T')))
            .map(|(pre, _)| pre)
            .unwrap_or(iso),
        "%Y-%m-%dT%H:%M:%S",
    ) {
        return dt.format(format).to_string();
    }
    // Fallback: take the date portion
    iso.split('T').next().unwrap_or(iso).to_string()
}

fn starter_files(config: &BrainConfig, now_date: &str) -> Result<Vec<(String, String)>, BrainError> {
    let config_toml = config.to_toml()?;

    Ok(vec![
        (".agentzero-brain.toml".to_string(), config_toml),
        (
            format!("{}/index.md", config.vault.wiki_dir),
            format!(
                r#"---
type: index
status: active
created: {now_date}
tags: []
---

# Vault Index

Welcome to your AgentZero Brain vault.

## Areas

- [Projects](projects/index.md)
- [Decisions](decisions/)
- [Daily Notes](daily/)
"#
            ),
        ),
        (
            format!("{}/log.md", config.vault.wiki_dir),
            format!(
                r#"---
type: log
status: active
created: {now_date}
tags: []
---

# Operation Log

All vault operations are logged here.
"#
            ),
        ),
        (
            format!("{}/projects/index.md", config.vault.wiki_dir),
            format!(
                r#"---
type: index
status: active
created: {now_date}
tags: [projects]
---

# Projects Index

Active projects are listed here.
"#
            ),
        ),
        (
            format!("{}/daily.md", config.vault.templates_dir),
            r#"---
type: daily
created: {{date}}
tags: []
---

# {{date}}

## Plan

## Capture

## Notes

## End of Day
"#
            .to_string(),
        ),
        (
            format!("{}/project.md", config.vault.templates_dir),
            r#"---
type: project
status: active
created: {{date}}
updated: {{date}}
tags: []
---

# Project Name

## Goal

## Status

## Next Actions

## Decisions

## Notes
"#
            .to_string(),
        ),
        (
            format!("{}/decision.md", config.vault.templates_dir),
            r#"---
type: decision
status: draft
created: {{date}}
tags: []
---

# Decision: Title

## Context

## Options

## Decision

## Consequences
"#
            .to_string(),
        ),
        (
            "AGENTS.md".to_string(),
            format!(
                r#"---
type: meta
created: {now_date}
---

# Agent Instructions

This vault is managed by AgentZero Brain.

## Conventions

- All notes use YAML frontmatter
- Daily notes live in {wiki}/daily/
- Templates live in {templates}/
- Raw ingestion goes to {raw}/ (immutable by default)
- Links use standard Markdown syntax
"#,
                wiki = config.vault.wiki_dir,
                templates = config.vault.templates_dir,
                raw = config.vault.raw_dir,
            ),
        ),
        (
            "CLAUDE.md".to_string(),
            format!(
                r#"---
type: meta
created: {now_date}
---

# Claude Code Instructions

This is an AgentZero Brain vault.

## Structure

- `{wiki}/` — curated wiki notes
- `{raw}/` — raw ingested material (immutable)
- `{templates}/` — note templates
- `prompts/` — prompt templates

## Rules

- Never modify files in `{raw}/` directly
- Always use YAML frontmatter
- Use standard Markdown links (not wikilinks)
"#,
                wiki = config.vault.wiki_dir,
                raw = config.vault.raw_dir,
                templates = config.vault.templates_dir,
            ),
        ),
        (
            "README.md".to_string(),
            format!(
                r#"# Brain Vault

Personal knowledge vault powered by AgentZero Brain.

## Quick Start

```bash
az brain today          # Open today's daily note
az brain capture "idea" # Capture a thought
az brain query "term"   # Search the vault
```

Created: {now_date}
"#
            ),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::TestFs;

    #[test]
    fn test_creates_canonical_dirs() {
        let fs = TestFs::new();
        let config = BrainConfig::default();
        let opts = InitOptions::default();
        let result = brain_init(&fs, "/vault", &config, &opts).expect("init");
        assert!(!result.created.is_empty());
        // Check some key dirs were created
        let dirs = fs.dirs();
        assert!(dirs.contains(&"/vault/raw/inbox".to_string()));
        assert!(dirs.contains(&"/vault/wiki/daily".to_string()));
        assert!(dirs.contains(&"/vault/templates".to_string()));
    }

    #[test]
    fn test_creates_starter_files() {
        let fs = TestFs::new();
        let config = BrainConfig::default();
        let opts = InitOptions::default();
        brain_init(&fs, "/vault", &config, &opts).expect("init");
        let files = fs.files();
        assert!(files.contains_key("/vault/.agentzero-brain.toml"));
        assert!(files.contains_key("/vault/wiki/index.md"));
        assert!(files.contains_key("/vault/wiki/log.md"));
        assert!(files.contains_key("/vault/templates/daily.md"));
    }

    #[test]
    fn test_idempotent() {
        let fs = TestFs::new();
        let config = BrainConfig::default();
        let opts = InitOptions::default();
        brain_init(&fs, "/vault", &config, &opts).expect("init first");
        let result = brain_init(&fs, "/vault", &config, &opts).expect("init second");
        // Second run should skip existing files
        assert!(!result.skipped.is_empty());
    }

    #[test]
    fn test_force_overwrites() {
        let fs = TestFs::new();
        let config = BrainConfig::default();
        let opts = InitOptions::default();
        brain_init(&fs, "/vault", &config, &opts).expect("init");
        let force_opts = InitOptions {
            force: true,
            dry_run: false,
        };
        let result = brain_init(&fs, "/vault", &config, &force_opts).expect("force init");
        // With force, files should be updated, not skipped
        assert!(result.skipped.is_empty());
        assert!(!result.updated.is_empty());
    }

    #[test]
    fn test_dry_run_no_writes() {
        let fs = TestFs::new();
        let config = BrainConfig::default();
        let opts = InitOptions {
            force: false,
            dry_run: true,
        };
        let result = brain_init(&fs, "/vault", &config, &opts).expect("dry run");
        assert!(!result.created.is_empty());
        // dry run should not actually create files
        assert!(fs.files().is_empty());
    }
}
