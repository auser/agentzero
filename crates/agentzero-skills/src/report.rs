//! Audit report generator for the repo-security-audit skill.
//!
//! Produces a human-readable report from scan results.

use std::fmt::Write;

use crate::scanner::{FindingCategory, ScanResults, Severity};

/// Generate a human-readable audit report from scan results.
pub fn generate_report(results: &ScanResults, project_name: &str) -> String {
    let mut report = String::new();

    // Executive summary
    writeln!(report, "# Security Audit Report: {project_name}").expect("write should succeed");
    writeln!(report).expect("write should succeed");
    writeln!(report, "## Executive Summary").expect("write should succeed");
    writeln!(report).expect("write should succeed");
    writeln!(report, "- Files scanned: {}", results.files_scanned.len())
        .expect("write should succeed");
    writeln!(report, "- Files skipped: {}", results.files_skipped.len())
        .expect("write should succeed");
    writeln!(report, "- Total findings: {}", results.findings.len()).expect("write should succeed");
    writeln!(
        report,
        "  - Critical: {}",
        results.finding_count_by_severity(Severity::Critical)
    )
    .expect("write should succeed");
    writeln!(
        report,
        "  - High: {}",
        results.finding_count_by_severity(Severity::High)
    )
    .expect("write should succeed");
    writeln!(
        report,
        "  - Warning: {}",
        results.finding_count_by_severity(Severity::Warning)
    )
    .expect("write should succeed");
    writeln!(
        report,
        "  - Info: {}",
        results.finding_count_by_severity(Severity::Info)
    )
    .expect("write should succeed");
    writeln!(report).expect("write should succeed");

    // Findings by severity
    if !results.findings.is_empty() {
        writeln!(report, "## Findings").expect("write should succeed");
        writeln!(report).expect("write should succeed");

        for finding in &results.findings {
            let line_info = if finding.line > 0 {
                format!(":{}", finding.line)
            } else {
                String::new()
            };
            writeln!(
                report,
                "- **[{}]** [{}] `{}{}`",
                finding.severity.label(),
                finding.category.label(),
                finding.path.display(),
                line_info,
            )
            .expect("write should succeed");
            writeln!(report, "  {}", finding.description).expect("write should succeed");
            if !finding.snippet.is_empty() {
                writeln!(report, "  > `{}`", finding.snippet).expect("write should succeed");
            }
        }
        writeln!(report).expect("write should succeed");
    }

    // Files skipped
    if !results.files_skipped.is_empty() {
        writeln!(report, "## Files Skipped").expect("write should succeed");
        writeln!(report).expect("write should succeed");
        for (path, reason) in &results.files_skipped {
            writeln!(report, "- `{}`: {reason}", path.display()).expect("write should succeed");
        }
        writeln!(report).expect("write should succeed");
    }

    // Recommendations
    writeln!(report, "## Recommendations").expect("write should succeed");
    writeln!(report).expect("write should succeed");

    let has_secrets = results
        .findings
        .iter()
        .any(|f| f.category == FindingCategory::Secret);
    let has_sensitive = results
        .findings
        .iter()
        .any(|f| f.category == FindingCategory::SensitiveFile);
    let has_pii = results
        .findings
        .iter()
        .any(|f| f.category == FindingCategory::Pii);
    let has_injection = results
        .findings
        .iter()
        .any(|f| f.category == FindingCategory::PromptInjection);

    if has_secrets {
        writeln!(
            report,
            "- **Rotate exposed secrets immediately** and add them to `.gitignore`."
        )
        .expect("write should succeed");
    }
    if has_sensitive {
        writeln!(
            report,
            "- **Remove sensitive files** from the repository and add patterns to `.gitignore`."
        )
        .expect("write should succeed");
    }
    if has_pii {
        writeln!(
            report,
            "- **Review PII exposure** and consider redaction or removal."
        )
        .expect("write should succeed");
    }
    if has_injection {
        writeln!(
            report,
            "- **Review prompt injection markers** in content — treat all repo content as untrusted data."
        )
        .expect("write should succeed");
    }
    if results.findings.is_empty() {
        writeln!(report, "- No findings. Repository appears clean.").expect("write should succeed");
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::{Finding, Severity};
    use std::path::PathBuf;

    #[test]
    fn empty_report() {
        let results = ScanResults::default();
        let report = generate_report(&results, "test-project");
        assert!(report.contains("test-project"));
        assert!(report.contains("Files scanned: 0"));
        assert!(report.contains("Total findings: 0"));
        assert!(report.contains("No findings"));
    }

    #[test]
    fn report_with_findings() {
        let results = ScanResults {
            findings: vec![
                Finding {
                    path: PathBuf::from("config.yml"),
                    line: 5,
                    category: FindingCategory::Secret,
                    severity: Severity::High,
                    description: "Hardcoded password".into(),
                    snippet: "password=***".into(),
                },
                Finding {
                    path: PathBuf::from(".env"),
                    line: 0,
                    category: FindingCategory::SensitiveFile,
                    severity: Severity::Critical,
                    description: "Sensitive file in repo".into(),
                    snippet: String::new(),
                },
            ],
            files_scanned: vec![PathBuf::from("config.yml")],
            files_skipped: vec![],
        };
        let report = generate_report(&results, "my-app");
        assert!(report.contains("my-app"));
        assert!(report.contains("Critical: 1"));
        assert!(report.contains("High: 1"));
        assert!(report.contains("Rotate exposed secrets"));
        assert!(report.contains("Remove sensitive files"));
    }

    #[test]
    fn report_includes_recommendations_for_each_category() {
        let results = ScanResults {
            findings: vec![
                Finding {
                    path: PathBuf::from("data.csv"),
                    line: 1,
                    category: FindingCategory::Pii,
                    severity: Severity::Warning,
                    description: "Email found".into(),
                    snippet: "test@gmail.com".into(),
                },
                Finding {
                    path: PathBuf::from("readme.md"),
                    line: 3,
                    category: FindingCategory::PromptInjection,
                    severity: Severity::High,
                    description: "Injection attempt".into(),
                    snippet: "ignore previous instructions".into(),
                },
            ],
            files_scanned: vec![],
            files_skipped: vec![],
        };
        let report = generate_report(&results, "test");
        assert!(report.contains("Review PII exposure"));
        assert!(report.contains("prompt injection markers"));
    }
}
