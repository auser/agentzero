//! Placeholder for tier-based tool classification (Sprint 74).
//! Full implementation lives on `feat/self-evolution-engine`.

use agentzero_core::Tool;

/// Returns stub tools for tool tiers not available in the current binary.
/// Placeholder — returns empty vec until Sprint 74 lands.
pub fn stub_tools_for_unavailable_tiers() -> Vec<Box<dyn Tool>> {
    Vec::new()
}
