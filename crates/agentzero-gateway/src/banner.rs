pub(crate) fn print_gateway_banner(base: &str, pairing_code: Option<&str>) {
    println!("🦀 AgentZero Gateway listening on {base}");
    println!("  🌐 Web Dashboard: {base}/");
    println!("  POST /pair      — pair a new client (X-Pairing-Code header)");
    println!("  POST /webhook   — {{\"message\": \"your prompt\"}}");
    println!("  POST /api/chat  — {{\"message\": \"...\", \"context\": [...]}} (tools-enabled, OpenClaw compat)");
    println!("  POST /v1/chat/completions — OpenAI-compatible (full agent loop)");
    println!("  GET  /v1/models — list available models");
    println!("  GET  /api/*     — REST API (bearer token required)");
    println!("  GET  /ws/chat   — WebSocket agent chat");
    println!("  GET  /health    — health check");
    println!("  GET  /metrics   — Prometheus metrics");
    println!();
    if let Some(pairing_code) = pairing_code {
        println!("  🔐 PAIRING REQUIRED — use this one-time code:");
        println!("     ┌──────────────┐");
        println!("     │  {pairing_code:<8}  │");
        println!("     └──────────────┘");
        println!("     Send: POST /pair with header X-Pairing-Code: {pairing_code}");
    } else {
        println!("  ✅ Pairing already configured (paired tokens found).");
        println!("     Use `agentzero gateway --new-pairing` to clear tokens and generate a fresh pairing code.");
    }
}
