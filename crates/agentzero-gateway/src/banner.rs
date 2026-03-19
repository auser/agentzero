pub(crate) fn print_gateway_banner(base: &str, pairing_code: Option<&str>) {
    print_gateway_banner_with_privacy(base, pairing_code, None, None);
}

pub(crate) fn print_gateway_banner_with_privacy(
    base: &str,
    pairing_code: Option<&str>,
    privacy_mode: Option<&str>,
    provider_kind: Option<&str>,
) {
    println!("🦀 AgentZero Gateway listening on {base}");
    if let Some(mode) = privacy_mode {
        match mode {
            "private" => println!("  🔒 Privacy: PRIVATE (Noise-encrypted, network tools blocked)"),
            "local_only" => println!("  🔒 Privacy: LOCAL ONLY (all traffic stays on-device)"),
            "encrypted" => println!("  🔒 Privacy: ENCRYPTED (Noise Protocol active)"),
            "full" => println!("  🔒 Privacy: FULL (all privacy features enabled)"),
            _ => {}
        }
        if let Some(provider) = provider_kind {
            if mode != "local_only"
                && mode != "off"
                && !matches!(
                    provider,
                    "ollama" | "llamacpp" | "lmstudio" | "vllm" | "sglang"
                )
            {
                println!("  ⚠️  CLOUD PROVIDER: {provider} — data WILL leave this machine");
            }
        }
    }
    println!("  🌐 Web Dashboard: {base}/");
    println!("  POST /pair      — pair a new client (X-Pairing-Code header)");
    println!("  POST /webhook   — {{\"message\": \"your prompt\"}}");
    println!("  POST /api/chat  — {{\"message\": \"...\", \"context\": [...]}} (tools-enabled)");
    println!("  POST /v1/chat/completions — OpenAI-compatible (full agent loop)");
    println!("  POST /v1/runs   — submit async job (returns run_id)");
    println!("  GET  /v1/runs   — list jobs");
    println!("  GET  /v1/runs/:id — job status / result / events / transcript");
    println!("  GET  /v1/runs/:id/stream — SSE event stream");
    println!("  GET  /v1/models — list available models");
    println!("  GET  /api/*     — REST API (bearer token required)");
    println!("  GET  /ws/chat   — WebSocket agent chat");
    println!("  GET  /health    — health check");
    println!("  GET  /health/ready — readiness probe");
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
