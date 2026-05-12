//! `brain health` — vault health diagnostics.
//! `brain status` — quick vault status summary.

use crate::{validate_path, BrainConfig, BrainError, BrainFs};
use serde::Serialize;

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/// Options for the health command.
pub struct HealthOptions {
    /// Output as JSON.
    pub json: bool,
    /// Attempt to fix issues (not yet implemented).
    pub fix: bool,
}

/// Severity level for diagnostics.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A single diagnostic finding.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub category: String,
    pub file: Option<String>,
    pub message: String,
}

/// Result of a health check.
#[derive(Debug, Serialize)]
pub struct HealthReport {
    pub diagnostics: Vec<Diagnostic>,
}

impl HealthReport {
    /// Format the report for display.
    pub fn display(&self) -> String {
        if self.diagnostics.is_empty() {
            return "All checks passed. Vault is healthy.".to_string();
        }

        let mut out = String::new();
        let errors = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        let warnings = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count();
        let infos = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Info)
            .count();

        out.push_str(&format!(
            "Health: {} error(s), {} warning(s), {} info(s)\n\n",
            errors, warnings, infos
        ));

        for d in &self.diagnostics {
            let icon = match d.severity {
                Severity::Error => "ERROR",
                Severity::Warning => "WARN ",
                Severity::Info => "INFO ",
            };
            let file_str = d
                .file
                .as_ref()
                .map(|f| format!(" [{f}]"))
                .unwrap_or_default();
            out.push_str(&format!(
                "  {icon} [{}]{file_str} {}\n",
                d.category, d.message
            ));
        }

        out
    }
}

/// Run vault health diagnostics.
pub fn brain_health(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
    opts: &HealthOptions,
) -> Result<HealthReport, BrainError> {
    validate_path(root)?;

    if opts.fix {
        // Accepted but not implemented
        // The message will be included as an info diagnostic
    }

    let mut diagnostics = Vec::new();

    if opts.fix {
        diagnostics.push(Diagnostic {
            severity: Severity::Info,
            category: "fix-mode".to_string(),
            file: None,
            message: "fix mode not yet implemented".to_string(),
        });
    }

    // Check: missing frontmatter in wiki/ files
    check_frontmatter(fs, root, config, &mut diagnostics);

    // Check: raw inbox files not yet ingested
    check_raw_inbox(fs, root, config, &mut diagnostics);

    // Check: missing index links
    check_index_links(fs, root, config, &mut diagnostics);

    // Check: empty sections in daily notes
    check_empty_sections(fs, root, config, &mut diagnostics);

    // Check: oversized notes
    check_oversized(fs, root, config, &mut diagnostics);

    Ok(HealthReport { diagnostics })
}

/// Check for wiki markdown files missing YAML frontmatter.
fn check_frontmatter(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let wiki_dir = format!("{root}/{}", config.vault.wiki_dir);
    check_frontmatter_recursive(fs, &wiki_dir, root, diagnostics);
}

fn check_frontmatter_recursive(
    fs: &dyn BrainFs,
    dir: &str,
    root: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let entries = match fs.list_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries {
        let path = format!("{dir}/{entry}");

        // Check subdirectories
        if fs.list_dir(&path).is_ok() {
            check_frontmatter_recursive(fs, &path, root, diagnostics);
            continue;
        }

        if !entry.ends_with(".md") {
            continue;
        }

        let content = match fs.read_file(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if !content.starts_with("---") {
            let rel = path.strip_prefix(&format!("{root}/")).unwrap_or(&path);
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                category: "missing-frontmatter".to_string(),
                file: Some(rel.to_string()),
                message: "file is missing YAML frontmatter".to_string(),
            });
        }
    }
}

/// Count raw inbox files.
fn check_raw_inbox(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let inbox_dir = format!("{root}/{}/inbox", config.vault.raw_dir);
    let entries = match fs.list_dir(&inbox_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let count = entries.len();
    if count > 0 {
        diagnostics.push(Diagnostic {
            severity: Severity::Info,
            category: "raw-inbox".to_string(),
            file: None,
            message: format!("{count} file(s) in raw/inbox/ awaiting ingestion"),
        });
    }
}

/// Check for project entries not linked from the projects index.
fn check_index_links(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let projects_dir = format!("{root}/{}/projects", config.vault.wiki_dir);
    let index_path = format!("{projects_dir}/index.md");

    let index_content = match fs.read_file(&index_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let entries = match fs.list_dir(&projects_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries {
        if entry == "index.md" {
            continue;
        }
        if !entry.ends_with(".md") {
            continue;
        }
        // Check if the file is referenced in the index
        if !index_content.contains(&entry) {
            let rel = format!("{}/projects/{entry}", config.vault.wiki_dir);
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                category: "orphan-note".to_string(),
                file: Some(rel),
                message: format!("{entry} not linked from projects/index.md"),
            });
        }
    }
}

/// Check for empty sections in daily notes (## heading with no content below).
fn check_empty_sections(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let daily_dir = format!("{root}/{}/daily", config.vault.wiki_dir);
    let entries = match fs.list_dir(&daily_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries {
        if !entry.ends_with(".md") {
            continue;
        }
        let path = format!("{daily_dir}/{entry}");
        let content = match fs.read_file(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let lines: Vec<&str> = content.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.starts_with("## ") {
                continue;
            }
            // Check if the section is empty: next non-blank line is another heading or EOF
            let mut has_content = false;
            for next_line in &lines[i + 1..] {
                let trimmed = next_line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.starts_with("## ") || trimmed.starts_with("# ") {
                    break;
                }
                has_content = true;
                break;
            }
            if !has_content {
                let rel = format!("{}/daily/{entry}", config.vault.wiki_dir);
                diagnostics.push(Diagnostic {
                    severity: Severity::Info,
                    category: "empty-section".to_string(),
                    file: Some(rel),
                    message: format!("empty section: {line}"),
                });
            }
        }
    }
}

/// Check for oversized notes (> 50KB).
fn check_oversized(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let wiki_dir = format!("{root}/{}", config.vault.wiki_dir);
    check_oversized_recursive(fs, &wiki_dir, root, diagnostics);
}

fn check_oversized_recursive(
    fs: &dyn BrainFs,
    dir: &str,
    root: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let entries = match fs.list_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries {
        let path = format!("{dir}/{entry}");

        if fs.list_dir(&path).is_ok() {
            check_oversized_recursive(fs, &path, root, diagnostics);
            continue;
        }

        if !entry.ends_with(".md") {
            continue;
        }

        let content = match fs.read_file(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if content.len() > 50 * 1024 {
            let rel = path.strip_prefix(&format!("{root}/")).unwrap_or(&path);
            let size_kb = content.len() / 1024;
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                category: "oversized-note".to_string(),
                file: Some(rel.to_string()),
                message: format!("note is {size_kb}KB (> 50KB threshold)"),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Result of a status check.
pub struct StatusResult {
    /// Vault root path.
    pub root: String,
    /// Number of wiki notes.
    pub wiki_count: usize,
    /// Number of daily notes.
    pub daily_count: usize,
    /// Number of raw files.
    pub raw_count: usize,
    /// Config date format.
    pub date_format: String,
    /// Whether raw is immutable.
    pub raw_immutable: bool,
    /// Whether .git exists.
    pub has_git: bool,
}

impl StatusResult {
    /// Format for display.
    pub fn display(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Vault root:      {}\n", self.root));
        out.push_str(&format!("Wiki notes:      {}\n", self.wiki_count));
        out.push_str(&format!("Daily notes:     {}\n", self.daily_count));
        out.push_str(&format!("Raw files:       {}\n", self.raw_count));
        out.push_str(&format!("Date format:     {}\n", self.date_format));
        out.push_str(&format!("Raw immutable:   {}\n", self.raw_immutable));
        out.push_str(&format!(
            "Git repo:        {}\n",
            if self.has_git { "yes" } else { "no" }
        ));
        out
    }
}

/// Show vault status summary.
pub fn brain_status(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
) -> Result<StatusResult, BrainError> {
    validate_path(root)?;

    let wiki_count = count_md_files(fs, &format!("{root}/{}", config.vault.wiki_dir));
    let daily_count = count_md_files(fs, &format!("{root}/{}/daily", config.vault.wiki_dir));
    let raw_count = count_all_files(fs, &format!("{root}/{}", config.vault.raw_dir));
    let has_git = fs.file_exists(&format!("{root}/.git")).unwrap_or(false);

    Ok(StatusResult {
        root: root.to_string(),
        wiki_count,
        daily_count,
        raw_count,
        date_format: config.daily.date_format.clone(),
        raw_immutable: config.safety.raw_is_immutable,
        has_git,
    })
}

/// Count markdown files recursively.
fn count_md_files(fs: &dyn BrainFs, dir: &str) -> usize {
    let entries = match fs.list_dir(dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };

    let mut count = 0;
    for entry in entries {
        let path = format!("{dir}/{entry}");
        if fs.list_dir(&path).is_ok() {
            count += count_md_files(fs, &path);
        } else if entry.ends_with(".md") {
            count += 1;
        }
    }
    count
}

/// Count all files recursively (for raw/).
fn count_all_files(fs: &dyn BrainFs, dir: &str) -> usize {
    let entries = match fs.list_dir(dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };

    let mut count = 0;
    for entry in entries {
        let path = format!("{dir}/{entry}");
        if fs.list_dir(&path).is_ok() {
            count += count_all_files(fs, &path);
        } else {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::{brain_init, InitOptions};
    use crate::tests::TestFs;

    fn setup_vault(fs: &TestFs) -> BrainConfig {
        let config = BrainConfig::default();
        let opts = InitOptions::default();
        brain_init(fs, "/vault", &config, &opts).expect("init");
        config
    }

    // --- health tests ---

    #[test]
    fn detects_missing_frontmatter() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        // This file has no frontmatter
        fs.set_file(
            "/vault/wiki/projects/myproject.md",
            "# My Project\n\nSome content.\n",
        );
        let opts = HealthOptions {
            json: false,
            fix: false,
        };
        let report = brain_health(&fs, "/vault", &config, &opts).expect("health");
        let fm_diags: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|d| d.category == "missing-frontmatter")
            .collect();
        assert!(!fm_diags.is_empty());
        assert!(fm_diags[0].message.contains("frontmatter"));
    }

    #[test]
    fn counts_raw_inbox_files() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file("/vault/raw/inbox/file1.md", "raw content 1");
        fs.set_file("/vault/raw/inbox/file2.md", "raw content 2");
        let opts = HealthOptions {
            json: false,
            fix: false,
        };
        let report = brain_health(&fs, "/vault", &config, &opts).expect("health");
        let inbox_diags: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|d| d.category == "raw-inbox")
            .collect();
        assert_eq!(inbox_diags.len(), 1);
        assert!(inbox_diags[0].message.contains("2 file(s)"));
    }

    #[test]
    fn reports_empty_sections() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file(
            "/vault/wiki/daily/2025-06-15.md",
            "---\ntype: daily\n---\n\n# 2025-06-15\n\n## Plan\n\n## Capture\n\n- did something\n\n## Notes\n\n## End of Day\n",
        );
        let opts = HealthOptions {
            json: false,
            fix: false,
        };
        let report = brain_health(&fs, "/vault", &config, &opts).expect("health");
        let empty_diags: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|d| d.category == "empty-section")
            .collect();
        // Plan, Notes, and End of Day are empty
        assert!(
            empty_diags.len() >= 2,
            "expected at least 2 empty sections, got {}",
            empty_diags.len()
        );
    }

    #[test]
    fn json_output_structure() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file("/vault/wiki/projects/test.md", "no frontmatter");
        let opts = HealthOptions {
            json: true,
            fix: false,
        };
        let report = brain_health(&fs, "/vault", &config, &opts).expect("health");
        let json_str = serde_json::to_string_pretty(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("parse");
        assert!(parsed.get("diagnostics").is_some());
        let diags = parsed["diagnostics"].as_array().expect("array");
        assert!(!diags.is_empty());
        // Each diagnostic should have severity, category, message
        assert!(diags[0].get("severity").is_some());
        assert!(diags[0].get("category").is_some());
        assert!(diags[0].get("message").is_some());
    }

    #[test]
    fn does_not_modify_files_by_default() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file("/vault/wiki/projects/test.md", "no frontmatter");
        let files_before = fs.files();
        let opts = HealthOptions {
            json: false,
            fix: false,
        };
        brain_health(&fs, "/vault", &config, &opts).expect("health");
        let files_after = fs.files();
        assert_eq!(files_before, files_after);
    }

    #[test]
    fn fix_mode_prints_not_implemented() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        let opts = HealthOptions {
            json: false,
            fix: true,
        };
        let report = brain_health(&fs, "/vault", &config, &opts).expect("health");
        let fix_diags: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|d| d.category == "fix-mode")
            .collect();
        assert_eq!(fix_diags.len(), 1);
        assert!(fix_diags[0].message.contains("not yet implemented"));
    }

    // --- status tests ---

    #[test]
    fn reports_file_counts() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file("/vault/wiki/daily/2025-06-15.md", "# daily");
        fs.set_file("/vault/wiki/daily/2025-06-14.md", "# daily 2");
        fs.set_file("/vault/raw/inbox/file.md", "raw");
        let result = brain_status(&fs, "/vault", &config).expect("status");
        assert_eq!(result.daily_count, 2);
        assert!(result.wiki_count >= 2); // at least the 2 daily notes + index/log
        assert!(result.raw_count >= 1);
        assert_eq!(result.root, "/vault");
    }

    #[test]
    fn detects_git_presence() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        let result = brain_status(&fs, "/vault", &config).expect("status");
        assert!(!result.has_git);

        // Now create .git
        fs.set_file("/vault/.git", "");
        let result = brain_status(&fs, "/vault", &config).expect("status");
        assert!(result.has_git);
    }

    #[test]
    fn status_display_includes_all_fields() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        let result = brain_status(&fs, "/vault", &config).expect("status");
        let display = result.display();
        assert!(display.contains("Vault root:"));
        assert!(display.contains("Wiki notes:"));
        assert!(display.contains("Daily notes:"));
        assert!(display.contains("Raw files:"));
        assert!(display.contains("Date format:"));
        assert!(display.contains("Raw immutable:"));
        assert!(display.contains("Git repo:"));
    }
}
