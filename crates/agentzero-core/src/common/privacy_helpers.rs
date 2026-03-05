//! String-based privacy boundary helpers.
//!
//! These work with the string representation of privacy boundaries ("local_only",
//! "encrypted_only", "any", "inherit", or empty) so enforcement logic doesn't
//! require the full `privacy` feature flag.

use super::local_providers::is_local_provider;

/// Check if a privacy boundary allows the given provider kind.
///
/// Only `"local_only"` restricts providers — it requires a local provider.
pub fn boundary_allows_provider(boundary: &str, provider_kind: &str) -> bool {
    match boundary {
        "local_only" => is_local_provider(provider_kind),
        _ => true,
    }
}

/// Check if a privacy boundary allows outbound network access.
pub fn boundary_allows_network(boundary: &str) -> bool {
    boundary != "local_only"
}

/// Resolve a child boundary against a parent. The result is always at least
/// as strict as the parent.
///
/// Rules:
/// - Empty or `"inherit"` → adopts parent
/// - `"local_only"` always wins (strictest)
/// - `"encrypted_only"` + `"any"` → `"encrypted_only"`
/// - `"any"` gets clamped to parent
pub fn resolve_boundary(child: &str, parent: &str) -> String {
    let effective = if child.is_empty() || child == "inherit" {
        parent
    } else {
        child
    };

    match (parent, effective) {
        ("local_only", _) => "local_only".to_string(),
        ("encrypted_only", "any") | ("encrypted_only", "") | ("encrypted_only", "inherit") => {
            "encrypted_only".to_string()
        }
        _ => effective.to_string(),
    }
}

/// Check if a memory entry with `entry_boundary` should be visible to a
/// query running under `query_boundary`.
///
/// Rules:
/// - Empty entry boundary → visible to everyone (pre-migration data).
/// - `"local_only"` entry → only visible to `"local_only"` queries.
/// - `"encrypted_only"` entry → visible to `"local_only"` and `"encrypted_only"`.
/// - `"any"` / empty / other → visible to all.
pub fn boundary_allows_recall(entry_boundary: &str, query_boundary: &str) -> bool {
    match entry_boundary {
        "" | "any" | "inherit" => true,
        "local_only" => query_boundary == "local_only",
        "encrypted_only" => matches!(query_boundary, "local_only" | "encrypted_only"),
        _ => true,
    }
}

/// Known tool names that require outbound network access.
const NETWORK_TOOLS: &[&str] = &[
    "web_search",
    "web_fetch",
    "http_request",
    "browser",
    "browser_open",
    "composio",
    "url_validation",
];

/// Check if a tool name is known to require network access.
pub fn is_network_tool(tool_name: &str) -> bool {
    NETWORK_TOOLS.contains(&tool_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_only_blocks_cloud_providers() {
        assert!(!boundary_allows_provider("local_only", "anthropic"));
        assert!(!boundary_allows_provider("local_only", "openai"));
        assert!(boundary_allows_provider("local_only", "ollama"));
        assert!(boundary_allows_provider("local_only", "llamacpp"));
    }

    #[test]
    fn other_boundaries_allow_all_providers() {
        assert!(boundary_allows_provider("encrypted_only", "anthropic"));
        assert!(boundary_allows_provider("any", "openai"));
        assert!(boundary_allows_provider("", "anthropic"));
        assert!(boundary_allows_provider("inherit", "openai"));
    }

    #[test]
    fn local_only_blocks_network() {
        assert!(!boundary_allows_network("local_only"));
        assert!(boundary_allows_network("encrypted_only"));
        assert!(boundary_allows_network("any"));
        assert!(boundary_allows_network(""));
    }

    #[test]
    fn resolve_inherit_adopts_parent() {
        assert_eq!(resolve_boundary("", "local_only"), "local_only");
        assert_eq!(
            resolve_boundary("inherit", "encrypted_only"),
            "encrypted_only"
        );
        assert_eq!(resolve_boundary("", "any"), "any");
    }

    #[test]
    fn resolve_local_only_parent_always_wins() {
        assert_eq!(resolve_boundary("any", "local_only"), "local_only");
        assert_eq!(
            resolve_boundary("encrypted_only", "local_only"),
            "local_only"
        );
    }

    #[test]
    fn resolve_encrypted_only_clamps_any() {
        assert_eq!(resolve_boundary("any", "encrypted_only"), "encrypted_only");
    }

    #[test]
    fn resolve_child_can_be_stricter() {
        assert_eq!(resolve_boundary("local_only", "any"), "local_only");
        assert_eq!(resolve_boundary("encrypted_only", "any"), "encrypted_only");
    }

    #[test]
    fn network_tool_detection() {
        assert!(is_network_tool("web_search"));
        assert!(is_network_tool("http_request"));
        assert!(!is_network_tool("shell"));
        assert!(!is_network_tool("read_file"));
    }

    #[test]
    fn boundary_recall_empty_entry_visible_to_all() {
        assert!(boundary_allows_recall("", "local_only"));
        assert!(boundary_allows_recall("", "encrypted_only"));
        assert!(boundary_allows_recall("", "any"));
        assert!(boundary_allows_recall("", ""));
    }

    #[test]
    fn boundary_recall_local_only_entry_only_visible_to_local() {
        assert!(boundary_allows_recall("local_only", "local_only"));
        assert!(!boundary_allows_recall("local_only", "encrypted_only"));
        assert!(!boundary_allows_recall("local_only", "any"));
        assert!(!boundary_allows_recall("local_only", ""));
    }

    #[test]
    fn boundary_recall_encrypted_only_visible_to_local_and_encrypted() {
        assert!(boundary_allows_recall("encrypted_only", "local_only"));
        assert!(boundary_allows_recall("encrypted_only", "encrypted_only"));
        assert!(!boundary_allows_recall("encrypted_only", "any"));
        assert!(!boundary_allows_recall("encrypted_only", ""));
    }

    #[test]
    fn boundary_recall_any_and_inherit_visible_to_all() {
        assert!(boundary_allows_recall("any", "local_only"));
        assert!(boundary_allows_recall("any", "any"));
        assert!(boundary_allows_recall("inherit", "encrypted_only"));
        assert!(boundary_allows_recall("inherit", ""));
    }
}
