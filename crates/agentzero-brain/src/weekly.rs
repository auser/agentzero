//! `brain weekly` — generate a weekly review prompt.

use crate::{validate_path, BrainConfig, BrainError, BrainFs};

/// Options for the weekly command.
pub struct WeeklyOptions {
    /// ISO week identifier, e.g., "2026-W20". Defaults to current week.
    pub week: Option<String>,
    /// Save the generated prompt to `wiki/reports/`.
    pub save_prompt: bool,
}

/// Result of a weekly review operation.
pub struct WeeklyResult {
    /// The generated prompt text.
    pub prompt: String,
    /// Path where the prompt was saved, if any.
    pub saved_to: Option<String>,
    /// The week identifier, e.g., "2025-W24".
    pub week: String,
    /// How many daily notes were found.
    pub found_count: usize,
    /// How many daily notes were missing.
    pub missing_count: usize,
}

/// Generate a weekly review prompt.
pub fn brain_weekly(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
    opts: &WeeklyOptions,
) -> Result<WeeklyResult, BrainError> {
    validate_path(root)?;

    let now = fs.now();
    let today = now.split('T').next().unwrap_or(&now);

    // Resolve the week and compute the 7 dates
    let (week_label, dates) = resolve_week_dates(today, &opts.week)?;

    // Collect daily notes
    let mut found_notes: Vec<(String, String)> = Vec::new();
    let mut missing_dates: Vec<String> = Vec::new();

    for date in &dates {
        let daily_path = format!("{root}/{}/daily/{date}.md", config.vault.wiki_dir);
        match fs.read_file(&daily_path) {
            Ok(content) => found_notes.push((date.clone(), content)),
            Err(_) => missing_dates.push(date.clone()),
        }
    }

    let found_count = found_notes.len();
    let missing_count = missing_dates.len();

    // Build the prompt
    let mut prompt = String::new();
    prompt.push_str("---\n");
    prompt.push_str("type: weekly-review-prompt\n");
    prompt.push_str(&format!("week: {week_label}\n"));
    prompt.push_str(&format!("generated: {now}\n"));
    prompt.push_str("---\n\n");

    prompt.push_str(&format!("# Weekly Review: {week_label}\n\n"));

    prompt.push_str("## Instructions\n\n");
    prompt.push_str("You are a knowledge-vault curator. Review the daily notes below and generate a weekly summary covering:\n\n");
    prompt.push_str("1. **Summary of the week** — key themes and accomplishments\n");
    prompt.push_str("2. **Progress on projects** — what moved forward\n");
    prompt.push_str("3. **Decisions made** — important choices and their rationale\n");
    prompt.push_str("4. **Active / blocked / stale projects** — current status\n");
    prompt.push_str("5. **Open questions** — unresolved items\n");
    prompt.push_str("6. **Next week focus** — priorities and planned actions\n");
    prompt.push_str("7. **Notes needing cleanup** — any daily notes that need wiki extraction\n\n");

    prompt.push_str(&format!(
        "Save the weekly summary to `wiki/weekly/{week_label}.md` with YAML frontmatter.\n\n"
    ));

    // Coverage
    prompt.push_str("## Coverage\n\n");
    prompt.push_str(&format!("- **Found:** {found_count} / 7 daily notes\n"));
    if !missing_dates.is_empty() {
        prompt.push_str(&format!("- **Missing:** {}\n", missing_dates.join(", ")));
    }
    prompt.push('\n');

    // Daily note contents
    prompt.push_str("## Daily Notes\n\n");
    if found_notes.is_empty() {
        prompt.push_str("_No daily notes found for this week._\n");
    } else {
        for (date, content) in &found_notes {
            prompt.push_str(&format!("### {date}\n\n"));
            prompt.push_str("```markdown\n");
            prompt.push_str(content);
            if !content.ends_with('\n') {
                prompt.push('\n');
            }
            prompt.push_str("```\n\n");
        }
    }

    let mut saved_to = None;

    if opts.save_prompt {
        let reports_dir = format!("{root}/{}/reports", config.vault.wiki_dir);
        let _ = fs.create_dir(&reports_dir);
        let report_path = format!("{reports_dir}/weekly-{week_label}.md");
        fs.write_file(&report_path, &prompt)
            .map_err(BrainError::Io)?;
        saved_to = Some(report_path);
    }

    Ok(WeeklyResult {
        prompt,
        saved_to,
        week: week_label,
        found_count,
        missing_count,
    })
}

/// Resolve the ISO week and return (label, [7 dates]).
fn resolve_week_dates(
    today: &str,
    week_opt: &Option<String>,
) -> Result<(String, Vec<String>), BrainError> {
    use chrono::{Datelike, NaiveDate};

    match week_opt {
        Some(w) => {
            // Parse "2026-W20" format
            let parts: Vec<&str> = w.split("-W").collect();
            if parts.len() != 2 {
                return Err(BrainError::InvalidDate(format!(
                    "invalid week format: {w} (expected YYYY-Www)"
                )));
            }
            let year: i32 = parts[0]
                .parse()
                .map_err(|_| BrainError::InvalidDate(format!("invalid year in {w}")))?;
            let week: u32 = parts[1]
                .parse()
                .map_err(|_| BrainError::InvalidDate(format!("invalid week number in {w}")))?;
            if week == 0 || week > 53 {
                return Err(BrainError::InvalidDate(format!(
                    "week number out of range: {week}"
                )));
            }

            // Find the Monday of the given ISO week
            let jan4 = NaiveDate::from_ymd_opt(year, 1, 4)
                .ok_or_else(|| BrainError::InvalidDate(format!("invalid year: {year}")))?;
            let jan4_weekday = jan4.weekday().num_days_from_monday();
            let week1_monday = jan4 - chrono::Duration::days(jan4_weekday as i64);
            let target_monday = week1_monday + chrono::Duration::weeks((week - 1) as i64);

            let dates: Vec<String> = (0..7)
                .map(|i| {
                    let d = target_monday + chrono::Duration::days(i);
                    d.format("%Y-%m-%d").to_string()
                })
                .collect();

            Ok((w.clone(), dates))
        }
        None => {
            // Use the current week
            let today_date = NaiveDate::parse_from_str(today, "%Y-%m-%d")
                .map_err(|e| BrainError::InvalidDate(format!("{today}: {e}")))?;
            let iso_week = today_date.iso_week();
            let week_label = format!("{}-W{:02}", iso_week.year(), iso_week.week());

            // Find Monday of this week
            let days_since_monday = today_date.weekday().num_days_from_monday();
            let monday = today_date - chrono::Duration::days(days_since_monday as i64);

            let dates: Vec<String> = (0..7)
                .map(|i| {
                    let d = monday + chrono::Duration::days(i);
                    d.format("%Y-%m-%d").to_string()
                })
                .collect();

            Ok((week_label, dates))
        }
    }
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
    fn collects_available_daily_notes() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        // TestFs.now() => "2025-06-15T14:30:00" which is a Sunday
        // Week 2025-W24: Mon 2025-06-09 to Sun 2025-06-15
        fs.set_file(
            "/vault/wiki/daily/2025-06-09.md",
            "# Monday\n\n## Capture\n\n- did stuff\n",
        );
        fs.set_file(
            "/vault/wiki/daily/2025-06-11.md",
            "# Wednesday\n\n## Capture\n\n- more stuff\n",
        );
        let opts = WeeklyOptions {
            week: None,
            save_prompt: false,
        };
        let result = brain_weekly(&fs, "/vault", &config, &opts).expect("weekly");
        assert_eq!(result.found_count, 2);
        assert_eq!(result.missing_count, 5);
        assert!(result.prompt.contains("Monday"));
        assert!(result.prompt.contains("Wednesday"));
    }

    #[test]
    fn handles_missing_daily_notes_gracefully() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        // No daily notes at all
        let opts = WeeklyOptions {
            week: Some("2025-W24".to_string()),
            save_prompt: false,
        };
        let result = brain_weekly(&fs, "/vault", &config, &opts).expect("weekly");
        assert_eq!(result.found_count, 0);
        assert_eq!(result.missing_count, 7);
        assert!(result.prompt.contains("No daily notes found"));
    }

    #[test]
    fn generates_prompt_with_week_summary() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file(
            "/vault/wiki/daily/2025-06-09.md",
            "# 2025-06-09\n\n## Capture\n\n- Weekly kickoff meeting\n",
        );
        let opts = WeeklyOptions {
            week: Some("2025-W24".to_string()),
            save_prompt: false,
        };
        let result = brain_weekly(&fs, "/vault", &config, &opts).expect("weekly");
        assert!(result.prompt.contains("Weekly Review: 2025-W24"));
        assert!(result.prompt.contains("Weekly kickoff meeting"));
        assert!(result.prompt.contains("Progress on projects"));
        assert!(result.prompt.contains("wiki/weekly/2025-W24.md"));
    }

    #[test]
    fn saves_prompt_when_requested() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file("/vault/wiki/daily/2025-06-09.md", "# Monday\n");
        let opts = WeeklyOptions {
            week: Some("2025-W24".to_string()),
            save_prompt: true,
        };
        let result = brain_weekly(&fs, "/vault", &config, &opts).expect("weekly");
        assert!(result.saved_to.is_some());
        let saved = result.saved_to.as_ref().expect("saved");
        assert!(saved.contains("weekly-2025-W24.md"));
    }

    #[test]
    fn rejects_invalid_week_format() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        let opts = WeeklyOptions {
            week: Some("bad-format".to_string()),
            save_prompt: false,
        };
        let result = brain_weekly(&fs, "/vault", &config, &opts);
        assert!(result.is_err());
    }
}
