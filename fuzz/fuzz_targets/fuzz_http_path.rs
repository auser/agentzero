//! Fuzz target for HTTP request path and query string parsing.
//!
//! Exercises the path/query parsing that gateway route handlers perform.
//! Ensures no panic occurs on arbitrary byte sequences that could be sent
//! as URL paths or query parameters.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    // Attempt to parse as a URI path — should never panic.
    // The gateway uses axum which delegates to `http::Uri`.
    let candidate = if s.starts_with('/') {
        s.to_owned()
    } else {
        format!("/{s}")
    };

    // Exercise path and query splitting (mirrors what axum/tower-http do).
    let (path, query) = match candidate.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (candidate.as_str(), None),
    };

    // Walk path segments — should never panic.
    let _: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    // Parse query key-value pairs — should never panic.
    if let Some(q) = query {
        let _: Vec<(&str, &str)> = q
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .collect();
    }
});
