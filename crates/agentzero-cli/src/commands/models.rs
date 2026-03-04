use crate::cli::ModelCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_config::load;
use agentzero_core::common::local_providers::{is_local_provider, local_provider_meta};
use agentzero_providers::{find_models_for_provider, model_capabilities, supported_providers};
use agentzero_storage::EncryptedJsonStore;
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use toml::{map::Map, Value};

const MODEL_CACHE_TTL_SECS: u64 = 12 * 60 * 60;
const MODEL_PREVIEW_LIMIT: usize = 20;

pub struct ModelsCommand;

#[async_trait]
impl AgentZeroCommand for ModelsCommand {
    type Options = ModelCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            ModelCommands::Refresh {
                provider,
                all,
                force,
            } => {
                if all {
                    if provider.is_some() {
                        anyhow::bail!("`models refresh --all` cannot be combined with --provider");
                    }
                    run_models_refresh_all(ctx, force).await
                } else {
                    run_models_refresh(ctx, provider.as_deref(), force).await
                }
            }
            ModelCommands::List { provider } => run_models_list(ctx, provider.as_deref()),
            ModelCommands::Set { model } => run_models_set(ctx, &model),
            ModelCommands::Status => run_models_status(ctx),
            ModelCommands::Pull { model, provider } => {
                run_models_pull(ctx, &model, provider.as_deref()).await
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CachedModelCatalog {
    provider: String,
    models: Vec<String>,
    updated_at_epoch_secs: u64,
}

async fn run_models_refresh(
    ctx: &CommandContext,
    provider_override: Option<&str>,
    force: bool,
) -> anyhow::Result<()> {
    let provider_name = selected_provider(ctx, provider_override);

    if !supports_live_model_fetch(&provider_name) {
        anyhow::bail!("Provider '{provider_name}' does not support live model discovery yet");
    }

    if !force {
        if let Some(cached) =
            load_cached_models_for_provider(&ctx.data_dir, &provider_name, MODEL_CACHE_TTL_SECS)?
        {
            println!(
                "Using cached model list for '{}' (updated {} ago):",
                provider_name,
                humanize_age(age_secs_from_now(cached.updated_at_epoch_secs))
            );
            print_model_preview(&cached.models);
            println!();
            println!(
                "Tip: run `agentzero models refresh --force --provider {}` to fetch latest now.",
                provider_name
            );
            return Ok(());
        }
    }

    match fetch_live_models_for_provider(&provider_name).await {
        Ok(models) if !models.is_empty() => {
            cache_live_models_for_provider(&ctx.data_dir, &provider_name, &models)?;
            println!(
                "Refreshed '{}' model cache with {} models.",
                provider_name,
                models.len()
            );
            print_model_preview(&models);
            Ok(())
        }
        Ok(_) => {
            if let Some(stale_cache) =
                load_any_cached_models_for_provider(&ctx.data_dir, &provider_name)?
            {
                println!(
                    "Provider returned no models; using stale cache (updated {} ago):",
                    humanize_age(age_secs_from_now(stale_cache.updated_at_epoch_secs))
                );
                print_model_preview(&stale_cache.models);
                return Ok(());
            }

            anyhow::bail!("Provider '{}' returned an empty model list", provider_name)
        }
        Err(error) => {
            if let Some(stale_cache) =
                load_any_cached_models_for_provider(&ctx.data_dir, &provider_name)?
            {
                println!(
                    "Live refresh failed ({}). Falling back to stale cache (updated {} ago):",
                    error,
                    humanize_age(age_secs_from_now(stale_cache.updated_at_epoch_secs))
                );
                print_model_preview(&stale_cache.models);
                return Ok(());
            }

            Err(error)
                .with_context(|| format!("failed to refresh models for provider '{provider_name}'"))
        }
    }
}

fn run_models_list(ctx: &CommandContext, provider_override: Option<&str>) -> anyhow::Result<()> {
    let provider_name = selected_provider(ctx, provider_override);

    let cached = load_any_cached_models_for_provider(&ctx.data_dir, &provider_name)?;

    let Some(cached) = cached else {
        println!();
        println!(
            "  No cached models for '{provider_name}'. Run: agentzero models refresh --provider {provider_name}"
        );
        println!();
        return Ok(());
    };

    let active_model = load(&ctx.config_path)
        .ok()
        .map(|cfg| cfg.provider.model)
        .unwrap_or_default();

    println!();
    println!(
        "  {} models for '{}' (cached {} ago):",
        cached.models.len(),
        provider_name,
        humanize_age(age_secs_from_now(cached.updated_at_epoch_secs))
    );
    println!();
    for model in &cached.models {
        let marker = if active_model == *model { "* " } else { "  " };
        println!("  {marker}{model}");
    }
    println!();
    Ok(())
}

fn run_models_set(ctx: &CommandContext, model: &str) -> anyhow::Result<()> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Model name cannot be empty");
    }

    upsert_provider_model_in_toml(&ctx.config_path, trimmed)?;

    println!();
    println!("  Default model set to '{trimmed}'.");
    println!();
    Ok(())
}

fn run_models_status(ctx: &CommandContext) -> anyhow::Result<()> {
    let config = load(&ctx.config_path).ok();
    let provider = config
        .as_ref()
        .map(|cfg| cfg.provider.kind.as_str())
        .unwrap_or("openrouter");
    let model = config
        .as_ref()
        .map(|cfg| cfg.provider.model.as_str())
        .unwrap_or("(not set)");

    println!();
    println!("  Provider:  {provider}");
    println!("  Model:     {model}");

    // Display model capabilities if known.
    if let Some(caps) = model_capabilities(provider, model) {
        let flag = |enabled: bool| if enabled { "yes" } else { "no" };
        println!("  Vision:    {}", flag(caps.vision));
        println!("  Tool use:  {}", flag(caps.tool_use));
        println!("  Streaming: {}", flag(caps.streaming));
        if caps.max_output_tokens > 0 {
            println!("  Max tokens: {}", caps.max_output_tokens);
        }
    } else {
        println!("  Capabilities: unknown (model not in catalog)");
    }

    match load_any_cached_models_for_provider(&ctx.data_dir, provider)? {
        Some(cached) => {
            let age_secs = age_secs_from_now(cached.updated_at_epoch_secs);
            println!(
                "  Cache:     {} models (updated {} ago)",
                cached.models.len(),
                humanize_age(age_secs)
            );
            let freshness = if age_secs < MODEL_CACHE_TTL_SECS {
                "fresh"
            } else {
                "stale"
            };
            println!("  Freshness: {freshness}");
        }
        None => {
            println!("  Cache:     none");
        }
    }

    println!();
    Ok(())
}

async fn run_models_refresh_all(ctx: &CommandContext, force: bool) -> anyhow::Result<()> {
    let mut targets: Vec<String> = supported_providers()
        .iter()
        .map(|provider| provider.id.to_string())
        .filter(|name| supports_live_model_fetch(name))
        .collect();

    targets.sort();
    targets.dedup();

    if targets.is_empty() {
        anyhow::bail!("No providers support live model discovery");
    }

    println!(
        "Refreshing model catalogs for {} providers (force: {})",
        targets.len(),
        if force { "yes" } else { "no" }
    );
    println!();

    let mut ok_count = 0usize;
    let mut fail_count = 0usize;

    for provider_name in &targets {
        println!("== {provider_name} ==");
        match run_models_refresh(ctx, Some(provider_name), force).await {
            Ok(()) => {
                ok_count += 1;
            }
            Err(error) => {
                fail_count += 1;
                println!("  failed: {error}");
            }
        }
        println!();
    }

    println!("Summary: {} succeeded, {} failed", ok_count, fail_count);

    if ok_count == 0 {
        anyhow::bail!("Model refresh failed for all providers");
    }
    Ok(())
}

fn selected_provider(ctx: &CommandContext, provider_override: Option<&str>) -> String {
    provider_override
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| load(&ctx.config_path).ok().map(|cfg| cfg.provider.kind))
        .unwrap_or_else(|| "openrouter".to_string())
}

fn supports_live_model_fetch(provider: &str) -> bool {
    find_models_for_provider(provider).is_some()
}

async fn fetch_live_models_for_provider(provider: &str) -> anyhow::Result<Vec<String>> {
    if is_local_provider(provider) {
        if let Some(meta) = local_provider_meta(provider) {
            match crate::local::list_models(provider, meta.default_base_url, 5000).await {
                Ok(models) => {
                    return Ok(models.into_iter().map(|m| m.id).collect());
                }
                Err(_) => {
                    // Fall through to static catalog
                }
            }
        }
    }

    let (_resolved_provider, models) = find_models_for_provider(provider)
        .with_context(|| format!("unknown provider '{provider}'"))?;

    Ok(models.iter().map(|model| model.id.to_string()).collect())
}

fn model_cache_store(data_dir: &Path, provider: &str) -> anyhow::Result<EncryptedJsonStore> {
    EncryptedJsonStore::in_config_dir(data_dir, &format!("models/{provider}.json"))
}

fn cache_live_models_for_provider(
    data_dir: &Path,
    provider: &str,
    models: &[String],
) -> anyhow::Result<()> {
    let store = model_cache_store(data_dir, provider)?;
    let payload = CachedModelCatalog {
        provider: provider.to_string(),
        models: models.to_vec(),
        updated_at_epoch_secs: now_epoch_secs(),
    };
    store.save(&payload)
}

fn load_cached_models_for_provider(
    data_dir: &Path,
    provider: &str,
    max_age_secs: u64,
) -> anyhow::Result<Option<CachedModelCatalog>> {
    let Some(cached) = load_any_cached_models_for_provider(data_dir, provider)? else {
        return Ok(None);
    };

    let age_secs = age_secs_from_now(cached.updated_at_epoch_secs);
    if age_secs <= max_age_secs {
        Ok(Some(cached))
    } else {
        Ok(None)
    }
}

fn load_any_cached_models_for_provider(
    data_dir: &Path,
    provider: &str,
) -> anyhow::Result<Option<CachedModelCatalog>> {
    let store = model_cache_store(data_dir, provider)?;
    store
        .load_optional::<CachedModelCatalog>()
        .with_context(|| format!("failed to parse model cache {}", store.path().display()))
}

fn print_model_preview(models: &[String]) {
    for model in models.iter().take(MODEL_PREVIEW_LIMIT) {
        println!("  - {model}");
    }

    if models.len() > MODEL_PREVIEW_LIMIT {
        println!("  - ... and {} more", models.len() - MODEL_PREVIEW_LIMIT);
    }
}

fn humanize_age(age_secs: u64) -> String {
    if age_secs < 60 {
        format!("{age_secs}s")
    } else if age_secs < 60 * 60 {
        format!("{}m", age_secs / 60)
    } else if age_secs < 60 * 60 * 24 {
        format!("{}h", age_secs / (60 * 60))
    } else {
        format!("{}d", age_secs / (60 * 60 * 24))
    }
}

fn upsert_provider_model_in_toml(path: &Path, model: &str) -> anyhow::Result<()> {
    let mut root = if path.exists() {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        raw.parse::<Value>()
            .with_context(|| format!("failed to parse config at {}", path.display()))?
    } else {
        Value::Table(Map::new())
    };

    let root_table = root
        .as_table_mut()
        .ok_or_else(|| anyhow!("config root must be a table"))?;
    let provider_entry = root_table
        .entry("provider")
        .or_insert_with(|| Value::Table(Map::new()));
    let provider_table = provider_entry
        .as_table_mut()
        .ok_or_else(|| anyhow!("config `provider` section must be a table"))?;
    provider_table.insert("model".to_string(), Value::String(model.to_string()));

    let serialized = toml::to_string_pretty(&root)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serialized).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn age_secs_from_now(epoch_secs: u64) -> u64 {
    now_epoch_secs().saturating_sub(epoch_secs)
}

async fn run_models_pull(
    ctx: &CommandContext,
    model: &str,
    provider_override: Option<&str>,
) -> anyhow::Result<()> {
    let provider_name = provider_override
        .map(str::to_string)
        .or_else(|| {
            load(&ctx.config_path)
                .ok()
                .map(|cfg| cfg.provider.kind)
                .filter(|kind| is_local_provider(kind))
        })
        .unwrap_or_else(|| "ollama".to_string());

    let meta = local_provider_meta(&provider_name).ok_or_else(|| {
        anyhow!(
            "'{}' is not a local provider. Model pulling is only available for local providers.",
            provider_name
        )
    })?;

    if !meta.supports_pull {
        anyhow::bail!(
            "Provider '{}' does not support model pulling. \
             Load models through its native interface.",
            provider_name
        );
    }

    let base_url = load(&ctx.config_path)
        .ok()
        .filter(|cfg| cfg.provider.kind == provider_name)
        .map(|cfg| cfg.provider.base_url)
        .unwrap_or_else(|| meta.default_base_url.to_string());

    println!("Pulling '{}' from {}...", model, provider_name);
    println!();

    crate::local::pull_model(&base_url, model, 600_000, |progress| {
        if let Some(pct) = progress.percent() {
            print!("\r  {} {:.0}%", progress.status, pct);
        } else {
            print!("\r  {}", progress.status);
        }
        use std::io::Write;
        let _ = std::io::stdout().flush();
    })
    .await?;

    println!();
    println!();
    println!("Done. Model '{}' is now available.", model);
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        cache_live_models_for_provider, humanize_age, load_any_cached_models_for_provider,
        load_cached_models_for_provider, upsert_provider_model_in_toml, CachedModelCatalog,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-models-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn cache_round_trip_success_path() {
        let dir = temp_dir();
        let models = vec!["gpt-4o-mini".to_string(), "gpt-4.1".to_string()];

        cache_live_models_for_provider(&dir, "openai", &models).expect("cache write should work");
        let cached = load_any_cached_models_for_provider(&dir, "openai")
            .expect("cache read should succeed")
            .expect("cache should exist");

        assert_eq!(cached.provider, "openai");
        assert_eq!(cached.models, models);

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn load_cache_returns_error_for_invalid_json_negative_path() {
        let dir = temp_dir();
        let models_dir = dir.join("models");
        fs::create_dir_all(&models_dir).expect("models dir should exist");
        fs::write(models_dir.join("openai.json"), b"not json").expect("file write should work");

        let err = load_any_cached_models_for_provider(&dir, "openai")
            .expect_err("invalid json should fail");
        assert!(err.to_string().contains("failed to parse model cache"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn load_cached_respects_ttl_success_path() {
        let dir = temp_dir();
        let models = vec!["gpt-4o-mini".to_string()];
        cache_live_models_for_provider(&dir, "openai", &models).expect("cache write should work");

        let fresh = load_cached_models_for_provider(&dir, "openai", 60)
            .expect("cache check should succeed")
            .expect("cache should be fresh");
        assert_eq!(fresh.models, models);

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn upsert_provider_model_updates_existing_config_success_path() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(
            &config_path,
            r#"[provider]
kind = "openai"
model = "old-model"
"#,
        )
        .expect("seed config should be written");

        upsert_provider_model_in_toml(&config_path, "new-model").expect("upsert should succeed");

        let updated = fs::read_to_string(&config_path).expect("config should be readable");
        assert!(updated.contains("model = \"new-model\""));
        assert!(updated.contains("kind = \"openai\""));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn upsert_provider_model_rejects_invalid_toml_negative_path() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "\"not-a-table\"").expect("invalid toml should be written");

        let err = upsert_provider_model_in_toml(&config_path, "gpt-4.1")
            .expect_err("invalid toml should fail");
        assert!(err.to_string().contains("failed to parse config"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn humanize_age_formats_units_success_path() {
        assert_eq!(humanize_age(59), "59s");
        assert_eq!(humanize_age(60), "1m");
        assert_eq!(humanize_age(3600), "1h");
        assert_eq!(humanize_age(172800), "2d");
    }

    #[test]
    fn cached_model_catalog_serde_round_trip_success_path() {
        let catalog = CachedModelCatalog {
            provider: "openrouter".to_string(),
            models: vec!["openai/gpt-4o-mini".to_string()],
            updated_at_epoch_secs: 1700000000,
        };

        let encoded = serde_json::to_vec(&catalog).expect("serialize should work");
        let decoded: CachedModelCatalog =
            serde_json::from_slice(&encoded).expect("deserialize should work");
        assert_eq!(decoded, catalog);
    }
}
