use crate::cli::DoctorCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_providers::{find_models_for_provider, supported_providers};
use agentzero_storage::EncryptedJsonStore;
use anyhow::Context;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::fs;
use std::path::Path;

pub struct DoctorCommand;

#[async_trait]
impl AgentZeroCommand for DoctorCommand {
    type Options = DoctorCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            DoctorCommands::Models {
                provider,
                use_cache,
            } => run_models_probe(ctx, provider.as_deref(), use_cache),
            DoctorCommands::Traces {
                id,
                event,
                contains,
                limit,
            } => run_traces_query(
                ctx,
                id.as_deref(),
                event.as_deref(),
                contains.as_deref(),
                limit,
            ),
        }
    }
}

fn run_models_probe(
    ctx: &CommandContext,
    provider_filter: Option<&str>,
    use_cache: bool,
) -> anyhow::Result<()> {
    let mut providers = if let Some(provider) = provider_filter {
        vec![provider.trim().to_string()]
    } else {
        supported_providers()
            .iter()
            .map(|provider| provider.id.to_string())
            .collect::<Vec<_>>()
    };

    providers.sort();
    providers.dedup();

    println!("Model catalog diagnostics");
    println!();
    println!("  {:<20} {:<10} MODELS", "PROVIDER", "STATUS");
    println!("  {:<20} {:<10} ------", "--------", "------");

    let mut ok = 0usize;
    let mut warn = 0usize;
    let mut err = 0usize;

    for provider in providers {
        let outcome = if use_cache {
            probe_models_from_cache(&ctx.data_dir, &provider)
        } else {
            probe_models_live_catalog(&provider)
        };

        match outcome {
            ModelProbeOutcome::Ok(count) => {
                ok += 1;
                println!("  {:<20} {:<10} {}", provider, "ok", count);
            }
            ModelProbeOutcome::Warn(detail) => {
                warn += 1;
                println!("  {:<20} {:<10} {}", provider, "warn", detail);
            }
            ModelProbeOutcome::Error(detail) => {
                err += 1;
                println!("  {:<20} {:<10} {}", provider, "error", detail);
            }
        }
    }

    println!();
    println!("Summary: {ok} ok, {warn} warnings, {err} errors");
    if err > 0 {
        println!("Tip: run `agentzero models refresh --provider <name>` to prime model cache.");
    }

    Ok(())
}

enum ModelProbeOutcome {
    Ok(usize),
    Warn(String),
    Error(String),
}

fn probe_models_live_catalog(provider: &str) -> ModelProbeOutcome {
    match find_models_for_provider(provider) {
        Some((_resolved, models)) if !models.is_empty() => ModelProbeOutcome::Ok(models.len()),
        Some((_resolved, _)) => ModelProbeOutcome::Warn("empty model list".to_string()),
        None => ModelProbeOutcome::Error("unknown provider".to_string()),
    }
}

fn probe_models_from_cache(data_dir: &Path, provider: &str) -> ModelProbeOutcome {
    let store =
        match EncryptedJsonStore::in_config_dir(data_dir, &format!("models/{provider}.json")) {
            Ok(store) => store,
            Err(err) => return ModelProbeOutcome::Error(format!("cache open failed: {err}")),
        };

    let payload = match store.load_optional::<CachedModelCatalog>() {
        Ok(Some(payload)) => payload,
        Ok(None) => return ModelProbeOutcome::Error("cache missing".to_string()),
        Err(err) => return ModelProbeOutcome::Error(format!("cache parse failed: {err}")),
    };

    if payload.models.is_empty() {
        ModelProbeOutcome::Warn("cache empty".to_string())
    } else {
        ModelProbeOutcome::Ok(payload.models.len())
    }
}

fn run_traces_query(
    ctx: &CommandContext,
    id_filter: Option<&str>,
    event_filter: Option<&str>,
    contains_filter: Option<&str>,
    limit: usize,
) -> anyhow::Result<()> {
    let limit = limit.max(1);
    let contains_filter = contains_filter.map(|value| value.to_ascii_lowercase());

    let mut events = load_trace_events(ctx).context("failed to load trace events")?;

    events.retain(|event| {
        if let Some(id) = id_filter {
            if event.id.as_deref() != Some(id) {
                return false;
            }
        }

        if let Some(event_name) = event_filter {
            if event.event.as_deref() != Some(event_name) {
                return false;
            }
        }

        if let Some(needle) = &contains_filter {
            let message_hit = event
                .message
                .as_ref()
                .map(|text| text.to_ascii_lowercase().contains(needle))
                .unwrap_or(false);
            let payload_hit = event
                .payload
                .as_ref()
                .and_then(|value| serde_json::to_string(value).ok())
                .map(|text| text.to_ascii_lowercase().contains(needle))
                .unwrap_or(false);
            if !message_hit && !payload_hit {
                return false;
            }
        }

        true
    });

    events.sort_by_key(|event| Reverse(event.ts_epoch_secs.unwrap_or(0)));

    println!("Trace events");
    println!();

    if events.is_empty() {
        println!("  no trace events found");
        return Ok(());
    }

    for event in events.into_iter().take(limit) {
        println!(
            "- id={} event={} ts={}",
            event.id.as_deref().unwrap_or("(none)"),
            event.event.as_deref().unwrap_or("(none)"),
            event
                .ts_epoch_secs
                .map(|value| value.to_string())
                .unwrap_or_else(|| "(none)".to_string())
        );
        if let Some(message) = event.message {
            println!("  message: {message}");
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedModelCatalog {
    provider: String,
    models: Vec<String>,
    updated_at_epoch_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraceEvent {
    id: Option<String>,
    event: Option<String>,
    message: Option<String>,
    payload: Option<serde_json::Value>,
    ts_epoch_secs: Option<u64>,
}

fn load_trace_events(ctx: &CommandContext) -> anyhow::Result<Vec<TraceEvent>> {
    let path = ctx.data_dir.join("trace_events.jsonl");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read trace file at {}", path.display()))?;
    let mut out = Vec::new();

    for (idx, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: TraceEvent = serde_json::from_str(trimmed)
            .with_context(|| format!("invalid trace event JSON at line {}", idx + 1))?;
        out.push(parsed);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{load_trace_events, probe_models_from_cache, run_traces_query, ModelProbeOutcome};
    use crate::command_core::CommandContext;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-doctor-cli-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn probe_models_from_cache_success_path() {
        let dir = temp_dir();
        let models_dir = dir.join("models");
        fs::create_dir_all(&models_dir).expect("models dir should exist");
        fs::write(
            models_dir.join("openai.json"),
            r#"{"provider":"openai","models":["gpt-4o-mini"],"updated_at_epoch_secs":1}"#,
        )
        .expect("cache should be written");

        let outcome = probe_models_from_cache(&dir, "openai");
        assert!(matches!(outcome, ModelProbeOutcome::Ok(1)));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn probe_models_from_cache_missing_file_negative_path() {
        let dir = temp_dir();
        let outcome = probe_models_from_cache(&dir, "openai");
        assert!(matches!(outcome, ModelProbeOutcome::Error(_)));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn load_trace_events_parses_jsonl_success_path() {
        let dir = temp_dir();
        fs::write(
            dir.join("trace_events.jsonl"),
            "{\"id\":\"1\",\"event\":\"tool\",\"message\":\"ok\",\"ts_epoch_secs\":10}\n",
        )
        .expect("trace file should be written");

        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let events = load_trace_events(&ctx).expect("trace load should succeed");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id.as_deref(), Some("1"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn run_traces_query_handles_no_file_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        run_traces_query(&ctx, None, None, None, 20).expect("traces query should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
