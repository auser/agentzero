//! Remote skill resolution: parsing user input into skill references
//! and resolving them to downloadable URLs.

/// How the user specified the skill source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillRefKind {
    /// Local filesystem path (e.g., `/path/to/skill` or `./my-skill`).
    Local(String),
    /// Git URL (e.g., `https://github.com/user/repo`).
    GitUrl(String),
    /// GitHub owner/repo shorthand (e.g., `auser/my-skill`).
    GitHub { owner: String, repo: String },
}

/// A resolved GitHub release with download information.
#[derive(Debug, Clone)]
pub struct ResolvedRelease {
    pub tag: String,
    pub version: String,
    pub tarball_url: String,
    pub checksum: Option<String>,
}

/// Parse a user-provided skill source string into a typed reference.
///
/// Recognition order:
/// 1. Starts with `http://` or `https://` → GitUrl
/// 2. Contains `/` but no `.` and no path separator at start → GitHub owner/repo
/// 3. Everything else → Local path
pub fn parse_skill_ref(input: &str) -> SkillRefKind {
    let input = input.trim();

    // Git URLs
    if input.starts_with("http://") || input.starts_with("https://") {
        // Check if it looks like github.com/owner/repo (without .git extension)
        // and could be treated as a GitHub ref for API access
        if let Some(github_path) = input
            .strip_prefix("https://github.com/")
            .or_else(|| input.strip_prefix("http://github.com/"))
        {
            let github_path = github_path.trim_end_matches('/').trim_end_matches(".git");
            let parts: Vec<&str> = github_path.splitn(3, '/').collect();
            if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                return SkillRefKind::GitHub {
                    owner: parts[0].to_string(),
                    repo: parts[1].to_string(),
                };
            }
        }
        return SkillRefKind::GitUrl(input.to_string());
    }

    // Local paths: starts with /, ./, ../, ~, or contains OS path separators
    if input.starts_with('/')
        || input.starts_with("./")
        || input.starts_with("../")
        || input.starts_with('~')
    {
        return SkillRefKind::Local(input.to_string());
    }

    // GitHub owner/repo: exactly one slash, no dots in the parts
    if let Some(slash_pos) = input.find('/') {
        let owner = &input[..slash_pos];
        let repo = &input[slash_pos + 1..];
        if !owner.is_empty()
            && !repo.is_empty()
            && !repo.contains('/')
            && !owner.contains('.')
            && !repo.contains('.')
        {
            return SkillRefKind::GitHub {
                owner: owner.to_string(),
                repo: repo.to_string(),
            };
        }
    }

    // Default to local path
    SkillRefKind::Local(input.to_string())
}

/// Extract checksum from GitHub release body text.
///
/// Looks for a line matching `sha256:<hex>` in the release notes.
pub fn extract_checksum_from_body(body: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("sha256:") && trimmed.len() == 71 {
            return Some(trimmed.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_local_absolute_path() {
        assert_eq!(
            parse_skill_ref("/path/to/skill"),
            SkillRefKind::Local("/path/to/skill".into())
        );
    }

    #[test]
    fn parse_local_relative_path() {
        assert_eq!(
            parse_skill_ref("./my-skill"),
            SkillRefKind::Local("./my-skill".into())
        );
    }

    #[test]
    fn parse_local_parent_path() {
        assert_eq!(
            parse_skill_ref("../skills/audit"),
            SkillRefKind::Local("../skills/audit".into())
        );
    }

    #[test]
    fn parse_github_owner_repo() {
        assert_eq!(
            parse_skill_ref("auser/cool-skill"),
            SkillRefKind::GitHub {
                owner: "auser".into(),
                repo: "cool-skill".into()
            }
        );
    }

    #[test]
    fn parse_github_url_as_github_ref() {
        assert_eq!(
            parse_skill_ref("https://github.com/auser/my-skill"),
            SkillRefKind::GitHub {
                owner: "auser".into(),
                repo: "my-skill".into()
            }
        );
    }

    #[test]
    fn parse_github_url_with_trailing_slash() {
        assert_eq!(
            parse_skill_ref("https://github.com/auser/my-skill/"),
            SkillRefKind::GitHub {
                owner: "auser".into(),
                repo: "my-skill".into()
            }
        );
    }

    #[test]
    fn parse_github_url_with_git_suffix() {
        assert_eq!(
            parse_skill_ref("https://github.com/auser/my-skill.git"),
            SkillRefKind::GitHub {
                owner: "auser".into(),
                repo: "my-skill".into()
            }
        );
    }

    #[test]
    fn parse_non_github_url_stays_git() {
        assert_eq!(
            parse_skill_ref("https://gitlab.com/user/repo"),
            SkillRefKind::GitUrl("https://gitlab.com/user/repo".into())
        );
    }

    #[test]
    fn parse_bare_name_is_local() {
        assert_eq!(
            parse_skill_ref("my-skill"),
            SkillRefKind::Local("my-skill".into())
        );
    }

    #[test]
    fn extract_checksum_from_release_body() {
        let body = "## Release v1.0\n\nsha256:a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2\n\nEnjoy!";
        let checksum = extract_checksum_from_body(body);
        assert!(checksum.is_some());
        assert!(checksum
            .as_ref()
            .expect("should exist")
            .starts_with("sha256:"));
    }

    #[test]
    fn extract_checksum_missing_returns_none() {
        let body = "## Release v1.0\n\nNo checksum here.";
        assert!(extract_checksum_from_body(body).is_none());
    }
}
