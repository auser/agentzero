use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskTier {
    P0Critical,
    P1High,
    P2Moderate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskDomain {
    ToolExecution,
    ProviderNetwork,
    ChannelIngress,
    RemoteMemory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequiredControls {
    pub deny_by_default: bool,
    pub requires_explicit_allowlist: bool,
    pub requires_redaction: bool,
    pub requires_timeout: bool,
}

impl FromStr for RiskDomain {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "tool_execution" => Ok(Self::ToolExecution),
            "provider_network" => Ok(Self::ProviderNetwork),
            "channel_ingress" => Ok(Self::ChannelIngress),
            "remote_memory" => Ok(Self::RemoteMemory),
            other => Err(format!("unsupported risk domain: {other}")),
        }
    }
}

pub fn baseline_version() -> &'static str {
    "2026-02-27"
}

pub fn tier_for(domain: RiskDomain) -> RiskTier {
    match domain {
        RiskDomain::ToolExecution => RiskTier::P0Critical,
        RiskDomain::ChannelIngress => RiskTier::P0Critical,
        RiskDomain::ProviderNetwork => RiskTier::P1High,
        RiskDomain::RemoteMemory => RiskTier::P1High,
    }
}

pub fn controls_for(domain: RiskDomain) -> RequiredControls {
    match domain {
        RiskDomain::ToolExecution => RequiredControls {
            deny_by_default: true,
            requires_explicit_allowlist: true,
            requires_redaction: true,
            requires_timeout: true,
        },
        RiskDomain::ChannelIngress => RequiredControls {
            deny_by_default: true,
            requires_explicit_allowlist: true,
            requires_redaction: true,
            requires_timeout: true,
        },
        RiskDomain::ProviderNetwork => RequiredControls {
            deny_by_default: false,
            requires_explicit_allowlist: false,
            requires_redaction: true,
            requires_timeout: true,
        },
        RiskDomain::RemoteMemory => RequiredControls {
            deny_by_default: false,
            requires_explicit_allowlist: false,
            requires_redaction: true,
            requires_timeout: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{controls_for, tier_for, RiskDomain, RiskTier};
    use std::str::FromStr;

    #[test]
    fn policy_marks_tool_execution_as_critical() {
        assert_eq!(tier_for(RiskDomain::ToolExecution), RiskTier::P0Critical);
        assert!(controls_for(RiskDomain::ToolExecution).deny_by_default);
    }

    #[test]
    fn parsing_unknown_domain_fails() {
        let parse = RiskDomain::from_str("unknown-domain");
        assert!(parse.is_err());
    }
}
