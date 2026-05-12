use crate::{check_raw_immutable, validate_path, BrainConfig, BrainError, BrainFs};

/// Ensure today's (or a given date's) daily note exists.
/// Returns the relative path to the daily note within the vault.
pub fn brain_today(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
    date_override: Option<&str>,
) -> Result<String, BrainError> {
    validate_root(fs, root)?;
    let date = resolve_date(fs, config, date_override)?;
    let rel_path = format!("{}/daily/{date}.md", config.vault.wiki_dir);
    let full_path = format!("{root}/{rel_path}");

    // Ensure we're not writing into the raw directory
    check_raw_immutable(root, &full_path, config)?;

    let exists = fs.file_exists(&full_path).map_err(BrainError::Io)?;
    if !exists {
        // Ensure parent dir
        let parent = format!("{root}/{}/daily", config.vault.wiki_dir);
        fs.create_dir(&parent).map_err(BrainError::Io)?;

        // Load template
        let template_path = format!("{root}/{}/daily.md", config.vault.templates_dir);
        let template = if fs.file_exists(&template_path).unwrap_or(false) {
            fs.read_file(&template_path).map_err(BrainError::Io)?
        } else {
            default_daily_template()
        };

        let content = template.replace("{{date}}", &date);
        fs.write_file(&full_path, &content)
            .map_err(BrainError::Io)?;
    }

    Ok(rel_path)
}

/// Append a capture entry to today's (or a given date's) daily note.
pub fn brain_capture(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
    message: &str,
    date_override: Option<&str>,
    section: Option<&str>,
) -> Result<(String, String), BrainError> {
    // Ensure the daily note exists
    let rel_path = brain_today(fs, root, config, date_override)?;
    let full_path = format!("{root}/{rel_path}");

    // Read current content
    let content = fs
        .read_file(&full_path)
        .map_err(BrainError::Io)?;

    let heading = section.unwrap_or("Capture");
    let heading_marker = format!("## {heading}");

    // Get current time
    let now_raw = fs.now();
    let time = extract_time(&now_raw, config);

    let entry = format!("- {time} -- {message}");

    let new_content = insert_under_heading(&content, &heading_marker, &entry);

    fs.write_file(&full_path, &new_content)
        .map_err(BrainError::Io)?;

    Ok((rel_path, entry))
}

/// Extract time from ISO datetime using config format.
fn extract_time(now_raw: &str, config: &BrainConfig) -> String {
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(now_raw, "%Y-%m-%dT%H:%M:%S") {
        dt.format(&config.daily.time_format).to_string()
    } else {
        // Fallback: just use the raw time portion
        now_raw
            .split('T')
            .nth(1)
            .unwrap_or(now_raw)
            .chars()
            .take(5)
            .collect()
    }
}

/// Insert a line under a heading. If the heading doesn't exist, append it.
fn insert_under_heading(content: &str, heading: &str, entry: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let heading_idx = lines.iter().position(|line| line.trim() == heading);

    match heading_idx {
        Some(idx) => {
            // Find the end of this section: next ## heading or EOF
            let section_end = lines
                .iter()
                .enumerate()
                .skip(idx + 1)
                .find(|(_, line)| line.starts_with("## "))
                .map(|(i, _)| i)
                .unwrap_or(lines.len());

            // Find last non-empty line in the section to insert after content
            let mut insert_at = idx + 1;
            for i in (idx + 1..section_end).rev() {
                if !lines[i].trim().is_empty() {
                    insert_at = i + 1;
                    break;
                }
            }

            // Build result: lines before insert point, entry, remaining lines
            let mut result: Vec<&str> = lines[..insert_at].to_vec();
            result.push(entry);
            result.extend_from_slice(&lines[insert_at..]);

            let mut out = result.join("\n");
            if content.ends_with('\n') && !out.ends_with('\n') {
                out.push('\n');
            }
            out
        }
        None => {
            // Heading not found — append heading + entry at end
            let mut out = content.to_string();
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
            out.push_str(heading);
            out.push('\n');
            out.push_str(entry);
            out.push('\n');
            out
        }
    }
}

fn resolve_date(
    fs: &dyn BrainFs,
    _config: &BrainConfig,
    date_override: Option<&str>,
) -> Result<String, BrainError> {
    match date_override {
        Some(d) => {
            // Validate the date parses
            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .map_err(|e| BrainError::InvalidDate(format!("{d}: {e}")))?;
            Ok(d.to_string())
        }
        None => {
            // Get current time from the filesystem abstraction
            let now_raw = fs.now();
            // Extract just the date portion from ISO 8601
            Ok(now_raw
                .split('T')
                .next()
                .unwrap_or(&now_raw)
                .to_string())
        }
    }
}

fn validate_root(fs: &dyn BrainFs, root: &str) -> Result<(), BrainError> {
    validate_path(root)?;
    // Check if vault is initialized by looking for config
    let config_path = format!("{root}/.agentzero-brain.toml");
    let exists = fs.file_exists(&config_path).map_err(BrainError::Io)?;
    if !exists {
        return Err(BrainError::VaultNotInitialized(root.to_string()));
    }
    Ok(())
}

fn default_daily_template() -> String {
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
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::TestFs;
    use crate::init::{brain_init, InitOptions};

    fn setup_vault(fs: &TestFs) -> BrainConfig {
        let config = BrainConfig::default();
        let opts = InitOptions::default();
        brain_init(fs, "/vault", &config, &opts).expect("init");
        config
    }

    #[test]
    fn test_correct_daily_path() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        let path = brain_today(&fs, "/vault", &config, Some("2025-06-15")).expect("today");
        assert_eq!(path, "wiki/daily/2025-06-15.md");
    }

    #[test]
    fn test_creates_from_template() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        brain_today(&fs, "/vault", &config, Some("2025-06-15")).expect("today");
        let content = fs.files().get("/vault/wiki/daily/2025-06-15.md").cloned().expect("file");
        assert!(content.contains("# 2025-06-15"));
        assert!(content.contains("## Capture"));
        assert!(content.contains("created: 2025-06-15"));
    }

    #[test]
    fn test_does_not_overwrite() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        brain_today(&fs, "/vault", &config, Some("2025-06-15")).expect("today first");
        // Manually modify
        fs.set_file("/vault/wiki/daily/2025-06-15.md", "custom content");
        brain_today(&fs, "/vault", &config, Some("2025-06-15")).expect("today second");
        let content = fs.files().get("/vault/wiki/daily/2025-06-15.md").cloned().expect("file");
        assert_eq!(content, "custom content");
    }

    #[test]
    fn test_respects_date_override() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        let path = brain_today(&fs, "/vault", &config, Some("2024-01-01")).expect("today");
        assert_eq!(path, "wiki/daily/2024-01-01.md");
    }

    #[test]
    fn test_capture_creates_note_if_missing() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        let (path, _) =
            brain_capture(&fs, "/vault", &config, "test message", Some("2025-06-15"), None)
                .expect("capture");
        assert_eq!(path, "wiki/daily/2025-06-15.md");
        let content = fs.files().get("/vault/wiki/daily/2025-06-15.md").cloned().expect("file");
        assert!(content.contains("test message"));
    }

    #[test]
    fn test_capture_appends_under_heading() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        brain_today(&fs, "/vault", &config, Some("2025-06-15")).expect("today");
        brain_capture(&fs, "/vault", &config, "first", Some("2025-06-15"), None).expect("cap 1");
        brain_capture(&fs, "/vault", &config, "second", Some("2025-06-15"), None).expect("cap 2");
        let content = fs.files().get("/vault/wiki/daily/2025-06-15.md").cloned().expect("file");
        assert!(content.contains("first"));
        assert!(content.contains("second"));
    }

    #[test]
    fn test_capture_creates_heading_if_missing() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        // Write a note without Capture heading
        fs.set_file("/vault/.agentzero-brain.toml", "");
        fs.set_file(
            "/vault/wiki/daily/2025-06-15.md",
            "---\ntype: daily\n---\n\n# 2025-06-15\n\n## Plan\n",
        );
        let (_, entry) = brain_capture(
            &fs,
            "/vault",
            &config,
            "new idea",
            Some("2025-06-15"),
            None,
        )
        .expect("capture");
        let content = fs.files().get("/vault/wiki/daily/2025-06-15.md").cloned().expect("file");
        assert!(content.contains("## Capture"));
        assert!(content.contains(&entry));
    }

    #[test]
    fn test_capture_handles_unicode() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        brain_today(&fs, "/vault", &config, Some("2025-06-15")).expect("today");
        let msg = "Idee: Architektur-Review fur das Projekt";
        brain_capture(&fs, "/vault", &config, msg, Some("2025-06-15"), None).expect("cap");
        let content = fs.files().get("/vault/wiki/daily/2025-06-15.md").cloned().expect("file");
        assert!(content.contains(msg));
    }

    #[test]
    fn test_capture_preserves_existing_content() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        brain_today(&fs, "/vault", &config, Some("2025-06-15")).expect("today");
        let content_before = fs.files().get("/vault/wiki/daily/2025-06-15.md").cloned().expect("file");
        brain_capture(&fs, "/vault", &config, "thought", Some("2025-06-15"), None).expect("cap");
        let content_after = fs.files().get("/vault/wiki/daily/2025-06-15.md").cloned().expect("file");
        // Original content should still be present
        assert!(content_after.contains("## Plan"));
        assert!(content_after.contains("## Notes"));
        assert!(content_after.contains("## End of Day"));
        // Plus the new content
        assert!(content_after.contains("thought"));
        assert!(content_after.len() > content_before.len());
    }
}
