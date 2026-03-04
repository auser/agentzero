use anyhow::anyhow;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use url::Url;

/// URL access policy enforcement.
///
/// Checks URLs against private IP blocking, domain allowlists/blocklists,
/// CIDR ranges, and DNS rebinding protection before allowing network access.
#[derive(Debug, Clone)]
pub struct UrlAccessPolicy {
    pub block_private_ip: bool,
    pub allow_loopback: bool,
    pub allow_cidrs: Vec<CidrRange>,
    pub allow_domains: Vec<String>,
    pub enforce_domain_allowlist: bool,
    pub domain_allowlist: Vec<String>,
    pub domain_blocklist: Vec<String>,
    pub approved_domains: Vec<String>,
}

impl Default for UrlAccessPolicy {
    fn default() -> Self {
        Self {
            block_private_ip: true,
            allow_loopback: false,
            allow_cidrs: Vec::new(),
            allow_domains: Vec::new(),
            enforce_domain_allowlist: false,
            domain_allowlist: Vec::new(),
            domain_blocklist: Vec::new(),
            approved_domains: Vec::new(),
        }
    }
}

/// A parsed CIDR range for IP matching.
#[derive(Debug, Clone)]
pub struct CidrRange {
    pub network: IpAddr,
    pub prefix_len: u8,
}

impl CidrRange {
    /// Parse a CIDR string like "10.0.0.0/8" or "::1/128".
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 2 {
            return Err(anyhow!("invalid CIDR notation: {s}"));
        }
        let network: IpAddr = parts[0]
            .parse()
            .map_err(|_| anyhow!("invalid IP in CIDR: {}", parts[0]))?;
        let prefix_len: u8 = parts[1]
            .parse()
            .map_err(|_| anyhow!("invalid prefix length in CIDR: {}", parts[1]))?;
        let max_prefix = match network {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        if prefix_len > max_prefix {
            return Err(anyhow!(
                "prefix length {prefix_len} exceeds maximum {max_prefix}"
            ));
        }
        Ok(Self {
            network,
            prefix_len,
        })
    }

    /// Check if an IP address falls within this CIDR range.
    pub fn contains(&self, ip: &IpAddr) -> bool {
        match (&self.network, ip) {
            (IpAddr::V4(net), IpAddr::V4(addr)) => {
                let net_bits = u32::from(*net);
                let addr_bits = u32::from(*addr);
                if self.prefix_len == 0 {
                    return true;
                }
                let mask = u32::MAX << (32 - self.prefix_len);
                (net_bits & mask) == (addr_bits & mask)
            }
            (IpAddr::V6(net), IpAddr::V6(addr)) => {
                let net_bits = u128::from(*net);
                let addr_bits = u128::from(*addr);
                if self.prefix_len == 0 {
                    return true;
                }
                let mask = u128::MAX << (128 - self.prefix_len);
                (net_bits & mask) == (addr_bits & mask)
            }
            _ => false, // v4 vs v6 mismatch
        }
    }
}

/// Result of enforcing a URL access policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlPolicyResult {
    /// URL is allowed.
    Allowed,
    /// URL requires first-visit approval from the user.
    RequiresApproval { domain: String },
    /// URL is blocked by policy.
    Blocked { reason: String },
}

/// Enforce URL access policy on a parsed URL.
///
/// This checks: domain blocklist, private IP blocking, domain allowlist,
/// and first-visit approval requirements.
pub fn enforce_url_policy(url: &Url, policy: &UrlAccessPolicy) -> UrlPolicyResult {
    let host = match url.host_str() {
        Some(h) => h.to_lowercase(),
        None => {
            return UrlPolicyResult::Blocked {
                reason: "URL has no host".to_string(),
            }
        }
    };

    // 1. Check domain blocklist first (always applies)
    if is_domain_blocked(&host, &policy.domain_blocklist) {
        return UrlPolicyResult::Blocked {
            reason: format!("domain `{host}` is in the blocklist"),
        };
    }

    // 2. Check explicit allow_domains (bypass all other checks)
    if is_domain_allowed(&host, &policy.allow_domains) {
        return UrlPolicyResult::Allowed;
    }

    // 3. Check approved_domains
    if is_domain_allowed(&host, &policy.approved_domains) {
        return UrlPolicyResult::Allowed;
    }

    // 4. Check private IP blocking
    if policy.block_private_ip {
        match check_private_ip(&host, policy) {
            PrivateIpResult::NotPrivate => {}
            PrivateIpResult::AllowedLoopback => {}
            PrivateIpResult::AllowedByCidr => {}
            PrivateIpResult::Blocked(reason) => {
                return UrlPolicyResult::Blocked { reason };
            }
            PrivateIpResult::DnsRebindingRisk(reason) => {
                return UrlPolicyResult::Blocked { reason };
            }
        }
    }

    // 5. Check domain allowlist enforcement
    if policy.enforce_domain_allowlist && !is_domain_allowed(&host, &policy.domain_allowlist) {
        return UrlPolicyResult::Blocked {
            reason: format!("domain `{host}` is not in the allowlist"),
        };
    }

    UrlPolicyResult::Allowed
}

/// Check if a domain matches any entry in a domain list.
/// Supports exact match and subdomain matching (e.g., "api.example.com" matches "example.com").
fn is_domain_allowed(host: &str, domains: &[String]) -> bool {
    domains.iter().any(|d| {
        let d_lower = d.to_lowercase();
        host == d_lower || host.ends_with(&format!(".{d_lower}"))
    })
}

fn is_domain_blocked(host: &str, blocklist: &[String]) -> bool {
    is_domain_allowed(host, blocklist)
}

enum PrivateIpResult {
    NotPrivate,
    AllowedLoopback,
    AllowedByCidr,
    Blocked(String),
    DnsRebindingRisk(String),
}

fn check_private_ip(host: &str, policy: &UrlAccessPolicy) -> PrivateIpResult {
    // Try to parse as IP literal first
    if let Ok(ip) = host.parse::<IpAddr>() {
        return check_ip_address(&ip, policy);
    }

    // Resolve domain to IP addresses for DNS rebinding protection
    let socket_addr = format!("{host}:80");
    match socket_addr.to_socket_addrs() {
        Ok(addrs) => {
            for addr in addrs {
                let ip = addr.ip();
                match check_ip_address(&ip, policy) {
                    PrivateIpResult::NotPrivate
                    | PrivateIpResult::AllowedLoopback
                    | PrivateIpResult::AllowedByCidr => continue,
                    PrivateIpResult::Blocked(_) => {
                        return PrivateIpResult::DnsRebindingRisk(format!(
                            "domain `{host}` resolves to private IP {ip}; possible DNS rebinding"
                        ));
                    }
                    PrivateIpResult::DnsRebindingRisk(r) => {
                        return PrivateIpResult::DnsRebindingRisk(r)
                    }
                }
            }
            PrivateIpResult::NotPrivate
        }
        Err(_) => {
            // DNS resolution failed — not necessarily a security issue,
            // let the HTTP client handle connectivity errors
            PrivateIpResult::NotPrivate
        }
    }
}

fn check_ip_address(ip: &IpAddr, policy: &UrlAccessPolicy) -> PrivateIpResult {
    // Check if IP is in explicitly allowed CIDRs first
    for cidr in &policy.allow_cidrs {
        if cidr.contains(ip) {
            return PrivateIpResult::AllowedByCidr;
        }
    }

    if ip.is_loopback() {
        if policy.allow_loopback {
            return PrivateIpResult::AllowedLoopback;
        }
        return PrivateIpResult::Blocked(format!("loopback address {ip} is blocked"));
    }

    if is_private_ip(ip) {
        return PrivateIpResult::Blocked(format!("private IP {ip} is blocked"));
    }

    PrivateIpResult::NotPrivate
}

/// Check if an IP address is in a private/reserved range.
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_ipv4(v4),
        IpAddr::V6(v6) => is_private_ipv6(v6),
    }
}

fn is_private_ipv4(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();
    // 10.0.0.0/8
    if octets[0] == 10 {
        return true;
    }
    // 172.16.0.0/12
    if octets[0] == 172 && (16..=31).contains(&octets[1]) {
        return true;
    }
    // 192.168.0.0/16
    if octets[0] == 192 && octets[1] == 168 {
        return true;
    }
    // 169.254.0.0/16 (link-local)
    if octets[0] == 169 && octets[1] == 254 {
        return true;
    }
    // 100.64.0.0/10 (carrier-grade NAT)
    if octets[0] == 100 && (64..=127).contains(&octets[1]) {
        return true;
    }
    // 0.0.0.0/8
    if octets[0] == 0 {
        return true;
    }
    // 240.0.0.0/4 (reserved)
    if octets[0] >= 240 {
        return true;
    }
    false
}

fn is_private_ipv6(ip: &Ipv6Addr) -> bool {
    let segments = ip.segments();
    // ::1 (loopback — handled separately)
    // :: (unspecified)
    if ip.is_unspecified() {
        return true;
    }
    // fc00::/7 (unique local)
    if (segments[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    // fe80::/10 (link-local)
    if (segments[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    // ::ffff:0:0/96 (IPv4-mapped — check the embedded v4 address)
    if segments[0..5] == [0, 0, 0, 0, 0] && segments[5] == 0xffff {
        let v4 = Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        );
        return is_private_ipv4(&v4);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_parse_valid() {
        let cidr = CidrRange::parse("10.0.0.0/8").unwrap();
        assert_eq!(cidr.prefix_len, 8);
    }

    #[test]
    fn cidr_parse_invalid() {
        assert!(CidrRange::parse("not-a-cidr").is_err());
        assert!(CidrRange::parse("10.0.0.0/33").is_err());
    }

    #[test]
    fn cidr_contains_ipv4() {
        let cidr = CidrRange::parse("192.168.1.0/24").unwrap();
        assert!(cidr.contains(&"192.168.1.100".parse().unwrap()));
        assert!(!cidr.contains(&"192.168.2.1".parse().unwrap()));
    }

    #[test]
    fn cidr_contains_ipv6() {
        let cidr = CidrRange::parse("fc00::/7").unwrap();
        assert!(cidr.contains(&"fd12::1".parse().unwrap()));
        assert!(!cidr.contains(&"2001:db8::1".parse().unwrap()));
    }

    #[test]
    fn private_ipv4_ranges() {
        assert!(is_private_ip(&"10.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"172.16.0.1".parse().unwrap()));
        assert!(is_private_ip(&"172.31.255.255".parse().unwrap()));
        assert!(is_private_ip(&"192.168.0.1".parse().unwrap()));
        assert!(is_private_ip(&"169.254.1.1".parse().unwrap()));
        assert!(is_private_ip(&"100.64.0.1".parse().unwrap()));
        assert!(is_private_ip(&"0.0.0.0".parse().unwrap()));
        assert!(!is_private_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip(&"1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn private_ipv6_ranges() {
        assert!(is_private_ip(&"fc00::1".parse().unwrap()));
        assert!(is_private_ip(&"fd12:3456::1".parse().unwrap()));
        assert!(is_private_ip(&"fe80::1".parse().unwrap()));
        assert!(is_private_ip(&"::".parse().unwrap()));
        assert!(!is_private_ip(&"2001:db8::1".parse().unwrap()));
    }

    #[test]
    fn ipv4_mapped_ipv6_private() {
        // ::ffff:192.168.1.1
        assert!(is_private_ip(&"::ffff:192.168.1.1".parse().unwrap()));
        // ::ffff:8.8.8.8
        assert!(!is_private_ip(&"::ffff:8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn policy_blocks_private_ip_literal() {
        let policy = UrlAccessPolicy::default();
        let url = Url::parse("http://192.168.1.1/api").unwrap();
        let result = enforce_url_policy(&url, &policy);
        assert!(matches!(result, UrlPolicyResult::Blocked { .. }));
    }

    #[test]
    fn policy_allows_public_ip() {
        let policy = UrlAccessPolicy::default();
        let url = Url::parse("https://8.8.8.8/dns-query").unwrap();
        let result = enforce_url_policy(&url, &policy);
        assert_eq!(result, UrlPolicyResult::Allowed);
    }

    #[test]
    fn policy_blocks_loopback_by_default() {
        let policy = UrlAccessPolicy::default();
        let url = Url::parse("http://127.0.0.1:8080").unwrap();
        let result = enforce_url_policy(&url, &policy);
        assert!(matches!(result, UrlPolicyResult::Blocked { .. }));
    }

    #[test]
    fn policy_allows_loopback_when_configured() {
        let policy = UrlAccessPolicy {
            allow_loopback: true,
            ..Default::default()
        };
        let url = Url::parse("http://127.0.0.1:8080").unwrap();
        let result = enforce_url_policy(&url, &policy);
        assert_eq!(result, UrlPolicyResult::Allowed);
    }

    #[test]
    fn policy_allow_cidrs_exempts_private_ip() {
        let policy = UrlAccessPolicy {
            allow_cidrs: vec![CidrRange::parse("10.0.0.0/8").unwrap()],
            ..Default::default()
        };
        let url = Url::parse("http://10.1.2.3/api").unwrap();
        let result = enforce_url_policy(&url, &policy);
        assert_eq!(result, UrlPolicyResult::Allowed);
    }

    #[test]
    fn policy_domain_blocklist() {
        let policy = UrlAccessPolicy {
            domain_blocklist: vec!["evil.com".to_string()],
            ..Default::default()
        };
        let url = Url::parse("https://evil.com/phish").unwrap();
        let result = enforce_url_policy(&url, &policy);
        assert!(matches!(result, UrlPolicyResult::Blocked { .. }));
    }

    #[test]
    fn policy_domain_blocklist_subdomain() {
        let policy = UrlAccessPolicy {
            domain_blocklist: vec!["evil.com".to_string()],
            ..Default::default()
        };
        let url = Url::parse("https://api.evil.com/data").unwrap();
        let result = enforce_url_policy(&url, &policy);
        assert!(matches!(result, UrlPolicyResult::Blocked { .. }));
    }

    #[test]
    fn policy_domain_allowlist_enforced() {
        let policy = UrlAccessPolicy {
            enforce_domain_allowlist: true,
            domain_allowlist: vec!["api.example.com".to_string()],
            ..Default::default()
        };

        let allowed = Url::parse("https://api.example.com/v1").unwrap();
        assert_eq!(
            enforce_url_policy(&allowed, &policy),
            UrlPolicyResult::Allowed
        );

        let blocked = Url::parse("https://other.com/v1").unwrap();
        assert!(matches!(
            enforce_url_policy(&blocked, &policy),
            UrlPolicyResult::Blocked { .. }
        ));
    }

    #[test]
    fn policy_allow_domains_bypass_private_ip_check() {
        let policy = UrlAccessPolicy {
            allow_domains: vec!["internal.corp".to_string()],
            ..Default::default()
        };
        // Even though this might resolve to a private IP, allow_domains bypasses
        let url = Url::parse("http://internal.corp/api").unwrap();
        let result = enforce_url_policy(&url, &policy);
        assert_eq!(result, UrlPolicyResult::Allowed);
    }

    #[test]
    fn policy_no_host_blocked() {
        let policy = UrlAccessPolicy::default();
        let url = Url::parse("file:///etc/passwd").unwrap();
        let result = enforce_url_policy(&url, &policy);
        assert!(matches!(result, UrlPolicyResult::Blocked { .. }));
    }

    #[test]
    fn policy_approved_domains_allowed() {
        let policy = UrlAccessPolicy {
            approved_domains: vec!["trusted.io".to_string()],
            ..Default::default()
        };
        let url = Url::parse("https://trusted.io/data").unwrap();
        assert_eq!(enforce_url_policy(&url, &policy), UrlPolicyResult::Allowed);
    }

    #[test]
    fn default_policy_allows_public_domains() {
        let policy = UrlAccessPolicy::default();
        let url = Url::parse("https://api.github.com/repos").unwrap();
        let result = enforce_url_policy(&url, &policy);
        assert_eq!(result, UrlPolicyResult::Allowed);
    }

    #[test]
    fn domain_matching_case_insensitive() {
        let policy = UrlAccessPolicy {
            domain_blocklist: vec!["Evil.Com".to_string()],
            ..Default::default()
        };
        let url = Url::parse("https://evil.com/path").unwrap();
        assert!(matches!(
            enforce_url_policy(&url, &policy),
            UrlPolicyResult::Blocked { .. }
        ));
    }
}
