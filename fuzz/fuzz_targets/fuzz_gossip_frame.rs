//! Fuzz target for gossip wire protocol frame parsing.
//!
//! Simulates reading a length-prefixed JSON frame from arbitrary bytes.
//! Ensures the parser never panics on malformed input.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The gossip wire protocol is: [4-byte BE length][JSON Event].
    // Simulate parsing without network IO.
    if data.len() < 4 {
        return;
    }
    let len_bytes: [u8; 4] = [data[0], data[1], data[2], data[3]];
    let len = u32::from_be_bytes(len_bytes) as usize;

    // Reject absurdly large frames (matches the 16 MB limit in gossip.rs).
    if len > 16 * 1024 * 1024 {
        return;
    }

    if data.len() < 4 + len {
        return;
    }

    let payload = &data[4..4 + len];
    let _ = serde_json::from_slice::<agentzero_core::event_bus::Event>(payload);
});
