//! Fuzz target for JSON event deserialization.
//!
//! Ensures that arbitrary JSON never panics when parsed as an Event.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Attempt to parse as a JSON Event — should never panic.
    let _ = serde_json::from_slice::<agentzero_core::event_bus::Event>(data);
});
