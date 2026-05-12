use crate::{validate_path, BrainConfig, BrainError, BrainFs};
use serde::Serialize;

/// A single search match.
#[derive(Debug, Clone, Serialize)]
pub struct QueryMatch {
    pub file: String,
    pub line: usize,
    pub content: String,
}

/// Options for brain query.
pub struct QueryOptions {
    pub include_raw: bool,
    pub json: bool,
    pub limit: usize,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            include_raw: false,
            json: false,
            limit: 50,
        }
    }
}

/// Run brain query: search for a term across vault markdown files.
pub fn brain_query(
    fs: &dyn BrainFs,
    root: &str,
    config: &BrainConfig,
    term: &str,
    opts: &QueryOptions,
) -> Result<Vec<QueryMatch>, BrainError> {
    validate_path(root)?;

    let mut matches = Vec::new();
    let term_lower = term.to_lowercase();

    // Search wiki directory
    let wiki_path = format!("{root}/{}", config.vault.wiki_dir);
    collect_matches(fs, &wiki_path, root, &term_lower, &mut matches, opts.limit)?;

    // Optionally search raw directory
    if opts.include_raw && matches.len() < opts.limit {
        let raw_path = format!("{root}/{}", config.vault.raw_dir);
        collect_matches(fs, &raw_path, root, &term_lower, &mut matches, opts.limit)?;
    }

    // Sort by file path
    matches.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));

    // Apply limit
    matches.truncate(opts.limit);

    Ok(matches)
}

/// Format query results for display.
pub fn format_results(matches: &[QueryMatch], json: bool) -> String {
    if json {
        return serde_json::to_string_pretty(matches).unwrap_or_else(|_| "[]".to_string());
    }

    if matches.is_empty() {
        return "No results found.".to_string();
    }

    let mut out = String::new();
    let mut current_file = "";

    for m in matches {
        if m.file != current_file {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&m.file);
            out.push('\n');
            current_file = &m.file;
        }
        out.push_str(&format!("  {}:{}\n", m.line, m.content));
    }

    out
}

/// Recursively collect matches from a directory.
fn collect_matches(
    fs: &dyn BrainFs,
    dir: &str,
    root: &str,
    term: &str,
    matches: &mut Vec<QueryMatch>,
    limit: usize,
) -> Result<(), BrainError> {
    if matches.len() >= limit {
        return Ok(());
    }

    let entries = match fs.list_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()), // Directory may not exist
    };

    for entry in entries {
        if matches.len() >= limit {
            break;
        }

        let path = format!("{dir}/{entry}");

        // Check if it's a directory by trying to list it
        if let Ok(sub_entries) = fs.list_dir(&path) {
            if !sub_entries.is_empty() || entry != "." {
                collect_matches(fs, &path, root, term, matches, limit)?;
                continue;
            }
        }

        // Only search .md files
        if !entry.ends_with(".md") {
            continue;
        }

        let content = match fs.read_file(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Make path relative to root
        let rel_path = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .trim_start_matches('/');

        for (i, line) in content.lines().enumerate() {
            if matches.len() >= limit {
                break;
            }
            if line.to_lowercase().contains(term) {
                matches.push(QueryMatch {
                    file: rel_path.to_string(),
                    line: i + 1,
                    content: line.to_string(),
                });
            }
        }
    }

    Ok(())
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
    fn test_finds_matching_lines() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file(
            "/vault/wiki/daily/2025-01-01.md",
            "# Hello World\n\nThis is a test note.\n",
        );
        let results = brain_query(
            &fs,
            "/vault",
            &config,
            "hello",
            &QueryOptions::default(),
        )
        .expect("query");
        assert!(!results.is_empty());
        assert!(results[0].content.contains("Hello World"));
    }

    #[test]
    fn test_handles_no_results() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        let results = brain_query(
            &fs,
            "/vault",
            &config,
            "nonexistentterm12345",
            &QueryOptions::default(),
        )
        .expect("query");
        assert!(results.is_empty());
        let output = format_results(&results, false);
        assert_eq!(output, "No results found.");
    }

    #[test]
    fn test_json_output() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file(
            "/vault/wiki/daily/2025-01-01.md",
            "# Hello World\n",
        );
        let results = brain_query(
            &fs,
            "/vault",
            &config,
            "hello",
            &QueryOptions::default(),
        )
        .expect("query");
        let json_str = format_results(&results, true);
        let parsed: Vec<serde_json::Value> =
            serde_json::from_str(&json_str).expect("valid json");
        assert!(!parsed.is_empty());
        assert!(parsed[0].get("file").is_some());
        assert!(parsed[0].get("line").is_some());
        assert!(parsed[0].get("content").is_some());
    }

    #[test]
    fn test_respects_limit() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        // Create a file with many matching lines
        let mut content = String::new();
        for i in 0..100 {
            content.push_str(&format!("line {i} match target\n"));
        }
        fs.set_file("/vault/wiki/daily/2025-01-01.md", &content);
        let opts = QueryOptions {
            limit: 5,
            ..Default::default()
        };
        let results = brain_query(&fs, "/vault", &config, "match target", &opts).expect("query");
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn test_case_insensitive() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file(
            "/vault/wiki/daily/2025-01-01.md",
            "UPPERCASE match\nlowercase match\nMixed Match\n",
        );
        let results = brain_query(
            &fs,
            "/vault",
            &config,
            "MATCH",
            &QueryOptions::default(),
        )
        .expect("query");
        assert_eq!(results.len(), 3);
    }
}
