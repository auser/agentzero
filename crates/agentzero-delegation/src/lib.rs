use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Configuration for a delegate sub-agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateConfig {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub system_prompt: Option<String>,
    pub api_key: Option<String>,
    pub temperature: Option<f64>,
    pub max_depth: usize,
    pub agentic: bool,
    pub allowed_tools: HashSet<String>,
    pub max_iterations: usize,
}

impl Default for DelegateConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            provider: String::new(),
            model: String::new(),
            system_prompt: None,
            api_key: None,
            temperature: None,
            max_depth: 3,
            agentic: false,
            allowed_tools: HashSet::new(),
            max_iterations: 10,
        }
    }
}

/// A delegation request from the parent agent.
#[derive(Debug, Clone)]
pub struct DelegateRequest {
    pub agent_name: String,
    pub prompt: String,
    pub current_depth: usize,
}

/// Result of a delegation.
#[derive(Debug, Clone)]
pub struct DelegateResult {
    pub agent_name: String,
    pub output: String,
    pub iterations_used: usize,
}

/// Validate delegation parameters before execution.
pub fn validate_delegation(
    request: &DelegateRequest,
    config: &DelegateConfig,
) -> anyhow::Result<()> {
    if request.current_depth >= config.max_depth {
        bail!(
            "delegation depth limit reached: current={}, max={}",
            request.current_depth,
            config.max_depth
        );
    }

    if config.provider.is_empty() {
        bail!(
            "delegate agent `{}` has no provider configured",
            request.agent_name
        );
    }

    if config.model.is_empty() {
        bail!(
            "delegate agent `{}` has no model configured",
            request.agent_name
        );
    }

    // The delegate tool itself must never appear in sub-agent tool lists
    // to prevent infinite delegation chains.
    if config.allowed_tools.contains("delegate") {
        bail!(
            "delegate agent `{}` must not have `delegate` in allowed_tools",
            request.agent_name
        );
    }

    Ok(())
}

/// Filter a tool list to only include allowed tools for a sub-agent.
pub fn filter_tools(all_tools: &[String], allowed: &HashSet<String>) -> Vec<String> {
    if allowed.is_empty() {
        // Empty allowlist means all tools (except delegate).
        all_tools
            .iter()
            .filter(|t| *t != "delegate")
            .cloned()
            .collect()
    } else {
        all_tools
            .iter()
            .filter(|t| allowed.contains(*t) && *t != "delegate")
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> DelegateConfig {
        DelegateConfig {
            name: "researcher".into(),
            provider: "openrouter".into(),
            model: "anthropic/claude-sonnet-4-6".into(),
            max_depth: 3,
            agentic: true,
            max_iterations: 10,
            ..Default::default()
        }
    }

    #[test]
    fn validate_rejects_depth_exceeded() {
        let req = DelegateRequest {
            agent_name: "researcher".into(),
            prompt: "find docs".into(),
            current_depth: 3,
        };
        let result = validate_delegation(&req, &config());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("depth limit"));
    }

    #[test]
    fn validate_rejects_delegate_in_allowed_tools() {
        let mut cfg = config();
        cfg.allowed_tools.insert("delegate".into());
        let req = DelegateRequest {
            agent_name: "researcher".into(),
            prompt: "search".into(),
            current_depth: 0,
        };
        assert!(validate_delegation(&req, &cfg).is_err());
    }

    #[test]
    fn validate_accepts_valid_request() {
        let req = DelegateRequest {
            agent_name: "researcher".into(),
            prompt: "search".into(),
            current_depth: 0,
        };
        assert!(validate_delegation(&req, &config()).is_ok());
    }

    #[test]
    fn filter_tools_excludes_delegate() {
        let tools = vec!["shell".into(), "file_read".into(), "delegate".into()];
        let result = filter_tools(&tools, &HashSet::new());
        assert!(!result.contains(&"delegate".to_string()));
        assert!(result.contains(&"shell".to_string()));
    }

    #[test]
    fn filter_tools_respects_allowlist() {
        let tools = vec!["shell".into(), "file_read".into(), "web_search".into()];
        let mut allowed = HashSet::new();
        allowed.insert("file_read".into());
        let result = filter_tools(&tools, &allowed);
        assert_eq!(result, vec!["file_read".to_string()]);
    }
}
