//! Fuzz target for TOML config parsing.
//!
//! Ensures that arbitrary input never panics when parsed as an AgentZero config.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Attempt to parse as TOML config — should never panic.
        let _ = toml::from_str::<agentzero_config::AgentZeroConfig>(s);
    }
});
