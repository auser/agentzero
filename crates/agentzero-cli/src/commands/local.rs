use crate::cli::LocalCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_common::local_providers::{is_local_provider, local_provider_meta};
use agentzero_config::load;
use agentzero_local::{check_health, discover_local_services, DiscoveryOptions, ServiceStatus};
use async_trait::async_trait;

pub struct LocalCommand;

#[async_trait]
impl AgentZeroCommand for LocalCommand {
    type Options = LocalCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            LocalCommands::Discover { timeout_ms, json } => run_discover(timeout_ms, json).await,
            LocalCommands::Status { json } => run_local_status(ctx, json).await,
            LocalCommands::Health { provider, url } => {
                run_health_check(&provider, url.as_deref()).await
            }
        }
    }
}

async fn run_discover(timeout_ms: u64, json: bool) -> anyhow::Result<()> {
    let opts = DiscoveryOptions {
        timeout_ms,
        providers: Vec::new(),
    };

    let results = discover_local_services(opts).await;

    if json {
        let json_results: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "provider": r.provider_id,
                    "url": r.base_url,
                    "status": match &r.status {
                        ServiceStatus::Running => "running",
                        ServiceStatus::Unreachable => "offline",
                        ServiceStatus::Error(_) => "error",
                    },
                    "models": r.models,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_results)?);
        return Ok(());
    }

    println!();
    println!("  Local AI services:");
    println!();
    println!("  {:<12} {:<12} {:<32} MODELS", "PROVIDER", "STATUS", "URL");
    println!("  {:<12} {:<12} {:<32} ------", "--------", "------", "---");

    for result in &results {
        let status = match &result.status {
            ServiceStatus::Running => "running",
            ServiceStatus::Unreachable => "offline",
            ServiceStatus::Error(e) => e.as_str(),
        };
        let models = if result.models.is_empty() {
            "-".to_string()
        } else if result.models.len() <= 3 {
            result.models.join(", ")
        } else {
            format!(
                "{}, +{} more",
                result.models[..2].join(", "),
                result.models.len() - 2
            )
        };
        println!(
            "  {:<12} {:<12} {:<32} {}",
            result.provider_id, status, result.base_url, models
        );
    }

    let running_count = results
        .iter()
        .filter(|r| r.status == ServiceStatus::Running)
        .count();
    println!();
    if running_count > 0 {
        println!("  Tip: use `agentzero config set provider.kind <provider>` to switch.");
    } else {
        println!("  No local services detected. Start one (e.g., `ollama serve`) and try again.");
    }
    println!();

    Ok(())
}

async fn run_local_status(ctx: &CommandContext, json: bool) -> anyhow::Result<()> {
    let config = load(&ctx.config_path).ok();
    let provider = config
        .as_ref()
        .map(|cfg| cfg.provider.kind.as_str())
        .unwrap_or("openrouter");

    if !is_local_provider(provider) {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "provider": provider,
                    "local": false,
                    "message": "configured provider is not a local provider"
                })
            );
        } else {
            println!();
            println!("  Current provider '{}' is not a local provider.", provider);
            println!("  Run `agentzero local discover` to find local services.");
            println!();
        }
        return Ok(());
    }

    let meta = local_provider_meta(provider);
    let base_url = config
        .as_ref()
        .map(|cfg| cfg.provider.base_url.as_str())
        .or_else(|| meta.map(|m| m.default_base_url))
        .unwrap_or("unknown");

    let health = check_health(provider, base_url, 3000).await;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "provider": provider,
                "local": true,
                "url": base_url,
                "reachable": health.reachable,
                "latency_ms": health.latency_ms,
                "error": health.error,
            })
        );
    } else {
        println!();
        println!("  Provider:  {}", provider);
        println!("  URL:       {}", base_url);
        println!(
            "  Status:    {}",
            if health.reachable {
                "running"
            } else {
                "offline"
            }
        );
        println!("  Latency:   {}ms", health.latency_ms);
        if let Some(err) = &health.error {
            println!("  Error:     {}", err);
        }
        println!();
    }

    Ok(())
}

async fn run_health_check(provider: &str, url_override: Option<&str>) -> anyhow::Result<()> {
    let base_url = resolve_health_check_url(provider, url_override)?;

    let result = check_health(provider, &base_url, 5000).await;

    println!();
    println!("  Provider:  {}", result.provider_id);
    println!("  URL:       {}", result.base_url);
    println!(
        "  Reachable: {}",
        if result.reachable { "yes" } else { "no" }
    );
    println!("  Latency:   {}ms", result.latency_ms);
    if let Some(err) = &result.error {
        println!("  Error:     {}", err);
    }
    println!();

    if !result.reachable {
        anyhow::bail!("Service '{}' at {} is not reachable", provider, base_url);
    }

    Ok(())
}

fn resolve_health_check_url(provider: &str, url_override: Option<&str>) -> anyhow::Result<String> {
    url_override
        .map(str::to_string)
        .or_else(|| local_provider_meta(provider).map(|m| m.default_base_url.to_string()))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown provider '{}'. Known local providers: ollama, llamacpp, lmstudio, vllm, sglang",
                provider
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_check_unknown_provider_returns_error() {
        let result = resolve_health_check_url("not-real-provider", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown provider"));
    }

    #[test]
    fn health_check_known_provider_resolves_default_url() {
        let url = resolve_health_check_url("ollama", None).expect("ollama should resolve");
        assert_eq!(url, "http://localhost:11434");
    }

    #[test]
    fn health_check_url_override_takes_precedence() {
        let url = resolve_health_check_url("ollama", Some("http://gpu:11434"))
            .expect("override should work");
        assert_eq!(url, "http://gpu:11434");
    }

    #[test]
    fn health_check_url_override_works_for_unknown_provider() {
        let url = resolve_health_check_url("custom-thing", Some("http://custom:8000"))
            .expect("override should bypass provider lookup");
        assert_eq!(url, "http://custom:8000");
    }

    #[test]
    fn status_detects_non_local_provider() {
        assert!(!is_local_provider("openrouter"));
        assert!(!is_local_provider("openai"));
        assert!(!is_local_provider("anthropic"));
    }

    #[test]
    fn status_detects_local_provider() {
        assert!(is_local_provider("ollama"));
        assert!(is_local_provider("llamacpp"));
        assert!(is_local_provider("vllm"));
    }
}
