use super::url_policy::{enforce_url_policy, UrlAccessPolicy, UrlPolicyResult};
use anyhow::anyhow;
use std::collections::HashMap;
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

/// Build a URL query string using standard percent-encoding (`%20` for spaces).
///
/// OAuth authorize endpoints and similar browser-facing URLs require `%20` for
/// spaces instead of `+` (which is only valid in `application/x-www-form-urlencoded`
/// POST bodies). This function matches the encoding used by the official OpenAI
/// Codex CLI (`urlencoding::encode`).
///
/// Keys are emitted in the order returned by the HashMap iterator. Use
/// [`build_query_string_ordered`] when deterministic ordering is required.
pub fn build_query_string(params: &HashMap<&str, &str>) -> String {
    encode_pairs(params.iter().map(|(&k, &v)| (k, v)))
}

/// Like [`build_query_string`] but emits keys in the provided order.
///
/// Use this when the parameter order matters (e.g. for readable URLs or when a
/// server is sensitive to ordering).
pub fn build_query_string_ordered(params: &[(&str, &str)]) -> String {
    encode_pairs(params.iter().map(|&(k, v)| (k, v)))
}

fn encode_pairs<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    pairs
        .map(|(k, v)| {
            let ek: String = url::form_urlencoded::byte_serialize(k.as_bytes()).collect();
            let ev: String = url::form_urlencoded::byte_serialize(v.as_bytes()).collect();
            format!("{ek}={ev}")
        })
        .collect::<Vec<_>>()
        .join("&")
        .replace('+', "%20")
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

    #[test]
    fn build_query_string_encodes_spaces_as_percent20() {
        let mut params = HashMap::new();
        params.insert("scope", "openid profile email");
        let qs = build_query_string(&params);
        assert!(qs.contains("scope=openid%20profile%20email"));
        assert!(!qs.contains('+'));
    }

    #[test]
    fn build_query_string_ordered_preserves_insertion_order() {
        let params = vec![
            ("response_type", "code"),
            ("client_id", "my_app"),
            ("scope", "openid email"),
        ];
        let qs = build_query_string_ordered(&params);
        assert_eq!(
            qs,
            "response_type=code&client_id=my_app&scope=openid%20email"
        );
    }

    #[test]
    fn build_query_string_ordered_encodes_special_chars() {
        let params = vec![("redirect_uri", "http://localhost:1455/auth/callback")];
        let qs = build_query_string_ordered(&params);
        assert_eq!(
            qs,
            "redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"
        );
    }
}
