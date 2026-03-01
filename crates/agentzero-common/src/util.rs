use crate::url_policy::{enforce_url_policy, UrlAccessPolicy, UrlPolicyResult};
use anyhow::anyhow;
use url::Url;

/// Parse and validate an HTTP/HTTPS URL (scheme + host check only).
pub fn parse_http_url(input: &str) -> anyhow::Result<Url> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("url input is required"));
    }

    let parsed = Url::parse(trimmed).map_err(|err| anyhow!("invalid url: {err}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(anyhow!("unsupported url scheme `{}`", parsed.scheme()));
    }
    if parsed.host_str().is_none() {
        return Err(anyhow!("url host is required"));
    }
    Ok(parsed)
}

/// Parse an HTTP URL and enforce the URL access policy.
///
/// Returns the parsed URL if allowed, or an error describing why access is denied.
pub fn parse_http_url_with_policy(input: &str, policy: &UrlAccessPolicy) -> anyhow::Result<Url> {
    let parsed = parse_http_url(input)?;
    match enforce_url_policy(&parsed, policy) {
        UrlPolicyResult::Allowed => Ok(parsed),
        UrlPolicyResult::RequiresApproval { domain } => {
            Err(anyhow!("domain `{domain}` requires first-visit approval"))
        }
        UrlPolicyResult::Blocked { reason } => Err(anyhow!("URL access denied: {reason}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_url_accepts_https_success_path() {
        let parsed = parse_http_url("https://example.com/path").expect("https should parse");
        assert_eq!(parsed.scheme(), "https");
        assert_eq!(parsed.host_str(), Some("example.com"));
    }

    #[test]
    fn parse_http_url_rejects_non_http_scheme_negative_path() {
        let err = parse_http_url("file:///tmp/test").expect_err("file scheme should fail");
        assert!(err.to_string().contains("unsupported url scheme"));
    }

    #[test]
    fn parse_with_policy_blocks_private_ip() {
        let policy = UrlAccessPolicy::default();
        let err = parse_http_url_with_policy("http://192.168.1.1/api", &policy)
            .expect_err("private IP should be blocked");
        assert!(err.to_string().contains("URL access denied"));
    }

    #[test]
    fn parse_with_policy_allows_public_url() {
        let policy = UrlAccessPolicy::default();
        let parsed = parse_http_url_with_policy("https://example.com/path", &policy)
            .expect("public URL should be allowed");
        assert_eq!(parsed.host_str(), Some("example.com"));
    }

    #[test]
    fn parse_with_policy_blocks_blocklisted_domain() {
        let policy = UrlAccessPolicy {
            domain_blocklist: vec!["blocked.com".to_string()],
            ..Default::default()
        };
        let err = parse_http_url_with_policy("https://blocked.com/path", &policy)
            .expect_err("blocklisted domain should be blocked");
        assert!(err.to_string().contains("URL access denied"));
    }
}
