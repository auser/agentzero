//! `brain review` — generate an end-of-day review prompt.

use crate::{validate_path, BrainConfig, BrainError, BrainFs};

/// Options for the review command.
pub struct ReviewOptions {
    /// Date to review (YYYY-MM-DD). Defaults to today.
    pub date: Option<String>,
    /// Save the generated prompt to `wiki/reports/`.
    pub save_prompt: bool,
    /// Show what would happen without writing.
    pub dry_run: bool,
}

/// Result of a review operation.
#[derive(Debug)]
pub struct ReviewResult {
    /// The generated prompt text.
    pub prompt: String,
    /// Path where the prompt was saved, if any.
    pub saved_to: Option<String>,
    /// The date that was reviewed.
    pub date: String,
}

/// Generate an end-of-day review prompt.
pub fn brain_review(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
    opts: &ReviewOptions,
) -> Result<ReviewResult, BrainError> {
    validate_path(root)?;

    // Resolve date
    let date = match &opts.date {
        Some(d) => {
            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .map_err(|e| BrainError::InvalidDate(format!("{d}: {e}")))?;
            d.clone()
        }
        None => {
            let now = fs.now();
            now.split('T').next().unwrap_or(&now).to_string()
        }
    };

    // Read today's daily note
    let daily_path = format!("{root}/{}/daily/{date}.md", config.vault.wiki_dir);
    let daily_content = fs.read_file(&daily_path).map_err(|_| {
        BrainError::Io(format!(
            "No daily note found for {date}. Run `az brain today --date {date}` first."
        ))
    })?;

    // Build the prompt
    let mut prompt = String::new();
    prompt.push_str("---\n");
    prompt.push_str("type: review-prompt\n");
    prompt.push_str(&format!("date: {date}\n"));
    prompt.push_str(&format!("generated: {}\n", fs.now()));
    prompt.push_str("---\n\n");

    prompt.push_str("# End-of-Day Review Prompt\n\n");
    prompt.push_str(&format!("## Date: {date}\n\n"));

    prompt.push_str("## Instructions\n\n");
    prompt.push_str("You are a knowledge-vault curator. Review the daily note below and:\n\n");
    prompt.push_str(
        "1. **Extract durable decisions** — create or update notes in `wiki/decisions/`.\n",
    );
    prompt
        .push_str("2. **Extract project updates** — update relevant notes in `wiki/projects/`.\n");
    prompt.push_str("3. **Extract area notes** — update relevant notes in `wiki/areas/`.\n");
    prompt.push_str("4. **Update indexes** — ensure `wiki/projects/index.md` and `wiki/index.md` link to new notes.\n");
    prompt.push_str("5. **Append to `wiki/log.md`** — summarize what was extracted.\n");
    prompt.push_str("6. **Never delete daily notes** — they are the source of truth.\n");
    prompt.push_str(
        "7. **Keep changes small and surgical** — one logical change per note update.\n\n",
    );

    prompt.push_str("## Daily Note Content\n\n");
    prompt.push_str(&format!(
        "**Path:** `{}/daily/{date}.md`\n\n",
        config.vault.wiki_dir
    ));
    prompt.push_str("```markdown\n");
    prompt.push_str(&daily_content);
    if !daily_content.ends_with('\n') {
        prompt.push('\n');
    }
    prompt.push_str("```\n");

    let mut saved_to = None;

    if !opts.dry_run && opts.save_prompt {
        let reports_dir = format!("{root}/{}/reports", config.vault.wiki_dir);
        let _ = fs.create_dir(&reports_dir);
        let report_path = format!("{reports_dir}/review-{date}.md");
        fs.write_file(&report_path, &prompt)
            .map_err(BrainError::Io)?;
        saved_to = Some(report_path);
    }

    Ok(ReviewResult {
        prompt,
        saved_to,
        date,
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
    fn errors_when_no_daily_note_exists() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        let opts = ReviewOptions {
            date: Some("2025-06-15".to_string()),
            save_prompt: false,
            dry_run: false,
        };
        let result = brain_review(&fs, "/vault", &config, &opts);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No daily note found"));
    }

    #[test]
    fn generates_prompt_containing_daily_note_content() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file(
            "/vault/wiki/daily/2025-06-15.md",
            "---\ntype: daily\n---\n\n# 2025-06-15\n\n## Capture\n\n- Decided to use Rust\n",
        );
        let opts = ReviewOptions {
            date: Some("2025-06-15".to_string()),
            save_prompt: false,
            dry_run: false,
        };
        let result = brain_review(&fs, "/vault", &config, &opts).expect("review");
        assert!(result.prompt.contains("Decided to use Rust"));
        assert!(result.prompt.contains("End-of-Day Review"));
        assert_eq!(result.date, "2025-06-15");
    }

    #[test]
    fn saves_prompt_when_save_prompt() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file(
            "/vault/wiki/daily/2025-06-15.md",
            "# 2025-06-15\n\n## Notes\n\nSome notes.\n",
        );
        let opts = ReviewOptions {
            date: Some("2025-06-15".to_string()),
            save_prompt: true,
            dry_run: false,
        };
        let result = brain_review(&fs, "/vault", &config, &opts).expect("review");
        assert!(result.saved_to.is_some());
        let saved = result.saved_to.as_ref().expect("saved");
        assert!(saved.contains("review-2025-06-15.md"));
        let content = fs.read_file(saved).expect("read saved");
        assert!(content.contains("Some notes."));
    }

    #[test]
    fn uses_today_when_no_date_given() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        // TestFs.now() returns "2025-06-15T14:30:00"
        fs.set_file(
            "/vault/wiki/daily/2025-06-15.md",
            "# 2025-06-15\n\n## Notes\n\nToday's content.\n",
        );
        let opts = ReviewOptions {
            date: None,
            save_prompt: false,
            dry_run: false,
        };
        let result = brain_review(&fs, "/vault", &config, &opts).expect("review");
        assert_eq!(result.date, "2025-06-15");
        assert!(result.prompt.contains("Today's content."));
    }
}
