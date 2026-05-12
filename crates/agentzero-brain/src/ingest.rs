//! `brain ingest <path>` — generate an ingest prompt for a raw file.

use crate::{validate_path, BrainConfig, BrainError, BrainFs};

/// Options for the ingest command.
pub struct IngestOptions {
    /// Save the generated prompt to `wiki/reports/`.
    pub save_prompt: bool,
    /// Show what would happen without writing.
    pub dry_run: bool,
}

/// Result of an ingest operation.
pub struct IngestResult {
    /// The generated prompt text.
    pub prompt: String,
    /// Path where the prompt was saved, if any.
    pub saved_to: Option<String>,
    /// Warnings emitted during generation.
    pub warnings: Vec<String>,
}

/// Generate an ingest prompt for a raw file.
pub fn brain_ingest(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
    source_path: &str,
    opts: &IngestOptions,
) -> Result<IngestResult, BrainError> {
    validate_path(root)?;
    validate_path(source_path)?;

    let mut warnings = Vec::new();

    // Validate source path exists
    let full_source = if source_path.starts_with('/') {
        source_path.to_string()
    } else {
        format!("{root}/{source_path}")
    };

    let exists = fs.file_exists(&full_source).map_err(BrainError::Io)?;
    if !exists {
        return Err(BrainError::Io(format!(
            "source file not found: {full_source}"
        )));
    }

    // Warn if source is outside raw/
    let raw_prefix = format!("{root}/{}/", config.vault.raw_dir);
    if !full_source.starts_with(&raw_prefix) {
        warnings.push(format!(
            "Warning: source file is outside {}/. Consider placing raw material there first.",
            config.vault.raw_dir
        ));
    }

    // Read the source file content
    let source_content = fs.read_file(&full_source).map_err(BrainError::Io)?;

    // Read AGENTS.md and CLAUDE.md for context (optional)
    let agents_md = fs
        .read_file(&format!("{root}/AGENTS.md"))
        .unwrap_or_default();
    let claude_md = fs
        .read_file(&format!("{root}/CLAUDE.md"))
        .unwrap_or_default();

    // Build the prompt
    let relative_source = full_source
        .strip_prefix(&format!("{root}/"))
        .unwrap_or(&full_source);

    let mut prompt = String::new();
    prompt.push_str("---\n");
    prompt.push_str("type: ingest-prompt\n");
    prompt.push_str(&format!("source: {relative_source}\n"));
    prompt.push_str(&format!("generated: {}\n", fs.now()));
    prompt.push_str("---\n\n");

    prompt.push_str("# Ingest Prompt\n\n");
    prompt.push_str("## Instructions\n\n");
    prompt.push_str("You are a knowledge-vault curator. Process the raw file below and:\n\n");
    prompt.push_str("1. **Classify** the raw file — what type of content is it? (article, notes, meeting log, reference, etc.)\n");
    prompt.push_str("2. **Create or update** cleaned wiki notes under `wiki/`. Use clear headings and YAML frontmatter.\n");
    prompt.push_str("3. **Preserve source references** — link back to the original raw file.\n");
    prompt.push_str("4. **Update `wiki/log.md`** with a summary of what was done.\n");
    prompt.push_str("5. **Do not invent facts** — only extract what is in the source material.\n");
    prompt.push_str("6. **Keep diffs small** — make targeted, surgical changes.\n\n");

    if !agents_md.is_empty() {
        prompt.push_str("## Vault Conventions (AGENTS.md)\n\n");
        prompt.push_str(&agents_md);
        prompt.push_str("\n\n");
    }

    if !claude_md.is_empty() {
        prompt.push_str("## Vault Rules (CLAUDE.md)\n\n");
        prompt.push_str(&claude_md);
        prompt.push_str("\n\n");
    }

    prompt.push_str("## Source File\n\n");
    prompt.push_str(&format!("**Path:** `{relative_source}`\n\n"));
    prompt.push_str("```\n");
    prompt.push_str(&source_content);
    if !source_content.ends_with('\n') {
        prompt.push('\n');
    }
    prompt.push_str("```\n");

    for w in &warnings {
        prompt.push_str(&format!("\n> {w}\n"));
    }

    let mut saved_to = None;

    if !opts.dry_run {
        // Append to wiki/log.md
        let log_path = format!("{root}/{}/log.md", config.vault.wiki_dir);
        let log_entry = format!(
            "\n- {} — ingest prompt generated for `{}`\n",
            fs.now(),
            relative_source
        );
        let _ = fs.append_file(&log_path, &log_entry);

        if opts.save_prompt {
            // Save prompt to wiki/reports/
            let reports_dir = format!("{root}/{}/reports", config.vault.wiki_dir);
            let _ = fs.create_dir(&reports_dir);
            let now = fs.now();
            let timestamp = now.replace([':', 'T'], "-");
            let report_path = format!("{reports_dir}/ingest-{timestamp}.md");
            fs.write_file(&report_path, &prompt)
                .map_err(BrainError::Io)?;
            saved_to = Some(report_path);
        }
    }

    Ok(IngestResult {
        prompt,
        saved_to,
        warnings,
    })
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

    #[test]
    fn generates_prompt_containing_source_content() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file(
            "/vault/raw/inbox/test.md",
            "# My Raw Notes\n\nSome content here.\n",
        );
        let opts = IngestOptions {
            save_prompt: false,
            dry_run: false,
        };
        let result =
            brain_ingest(&fs, "/vault", &config, "raw/inbox/test.md", &opts).expect("ingest");
        assert!(result.prompt.contains("My Raw Notes"));
        assert!(result.prompt.contains("Some content here."));
        assert!(result.prompt.contains("raw/inbox/test.md"));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn warns_about_files_outside_raw() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file("/vault/wiki/stray.md", "stray content");
        let opts = IngestOptions {
            save_prompt: false,
            dry_run: false,
        };
        let result = brain_ingest(&fs, "/vault", &config, "wiki/stray.md", &opts).expect("ingest");
        assert!(!result.warnings.is_empty());
        assert!(result.warnings[0].contains("outside"));
    }

    #[test]
    fn saves_prompt_to_wiki_reports() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file("/vault/raw/inbox/doc.md", "document content");
        let opts = IngestOptions {
            save_prompt: true,
            dry_run: false,
        };
        let result =
            brain_ingest(&fs, "/vault", &config, "raw/inbox/doc.md", &opts).expect("ingest");
        assert!(result.saved_to.is_some());
        let saved_path = result.saved_to.as_ref().expect("saved_to");
        assert!(saved_path.contains("wiki/reports/ingest-"));
        // Verify file was actually written
        let saved_content = fs.read_file(saved_path).expect("read saved");
        assert!(saved_content.contains("document content"));
    }

    #[test]
    fn appends_to_wiki_log() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file("/vault/raw/inbox/doc.md", "content");
        let opts = IngestOptions {
            save_prompt: false,
            dry_run: false,
        };
        brain_ingest(&fs, "/vault", &config, "raw/inbox/doc.md", &opts).expect("ingest");
        let log = fs.read_file("/vault/wiki/log.md").expect("read log");
        assert!(log.contains("ingest prompt generated"));
    }

    #[test]
    fn dry_run_does_not_write() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file("/vault/raw/inbox/doc.md", "content");
        let log_before = fs.read_file("/vault/wiki/log.md").unwrap_or_default();
        let opts = IngestOptions {
            save_prompt: true,
            dry_run: true,
        };
        let result =
            brain_ingest(&fs, "/vault", &config, "raw/inbox/doc.md", &opts).expect("ingest");
        assert!(result.saved_to.is_none());
        let log_after = fs.read_file("/vault/wiki/log.md").unwrap_or_default();
        assert_eq!(log_before, log_after);
    }
}
