use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_config::load;
use agentzero_providers::supported_providers;
use anyhow::Context;
use async_trait::async_trait;
#[cfg(feature = "interactive")]
use console::style;
use serde::Serialize;
use std::io::{self, Write};

pub struct ProvidersOptions {
    pub json: bool,
    pub no_color: bool,
}

pub struct ProvidersCommand;

#[async_trait]
impl AgentZeroCommand for ProvidersCommand {
    type Options = ProvidersOptions;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let active_provider = load(&ctx.config_path).ok().map(|cfg| cfg.provider.kind);
        let mut stdout = io::stdout();
        if opts.json {
            render_providers_json(&mut stdout, active_provider.as_deref())?;
        } else {
            render_providers_table(&mut stdout, active_provider.as_deref(), !opts.no_color)?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Clone)]
struct ProviderOutput {
    id: String,
    description: String,
    aliases: Vec<String>,
    active: bool,
    local: bool,
}

#[derive(Debug, Serialize)]
struct ProvidersJsonOutput {
    total: usize,
    active_provider: String,
    active_provider_known: bool,
    providers: Vec<ProviderOutput>,
    custom_endpoints: Vec<String>,
}

fn resolve_current(active_provider: Option<&str>) -> String {
    active_provider
        .unwrap_or("openrouter")
        .trim()
        .to_ascii_lowercase()
}

fn build_provider_rows(current: &str) -> (Vec<ProviderOutput>, bool) {
    let providers = supported_providers();
    let mut active_known = false;
    let rows = providers
        .iter()
        .map(|provider| {
            let is_active = provider.id.eq_ignore_ascii_case(current)
                || provider
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(current));
            if is_active {
                active_known = true;
            }
            ProviderOutput {
                id: provider.id.to_string(),
                description: provider.description.to_string(),
                aliases: provider
                    .aliases
                    .iter()
                    .map(|alias| alias.to_string())
                    .collect(),
                active: is_active,
                local: provider.description.contains("[local]"),
            }
        })
        .collect();
    (rows, active_known)
}

fn render_providers_json(
    writer: &mut dyn Write,
    active_provider: Option<&str>,
) -> anyhow::Result<()> {
    let current = resolve_current(active_provider);
    let (rows, active_known) = build_provider_rows(&current);
    let json = ProvidersJsonOutput {
        total: rows.len(),
        active_provider: current,
        active_provider_known: active_known,
        providers: rows,
        custom_endpoints: vec![
            "custom:<URL>".to_string(),
            "anthropic-custom:<URL>".to_string(),
        ],
    };
    writeln!(
        writer,
        "{}",
        serde_json::to_string_pretty(&json).context("failed to serialize providers json")?
    )
    .context("failed to write output")?;
    Ok(())
}

fn render_providers_table(
    writer: &mut dyn Write,
    active_provider: Option<&str>,
    colorize: bool,
) -> anyhow::Result<()> {
    let current = resolve_current(active_provider);
    let (rows, active_known) = build_provider_rows(&current);

    writeln!(writer, "Supported providers ({} total):", rows.len())
        .context("failed to write output")?;
    writeln!(writer).context("failed to write output")?;
    writeln!(writer, "  ID (use in config)  DESCRIPTION").context("failed to write output")?;
    writeln!(writer, "  ─────────────────── ───────────").context("failed to write output")?;

    for provider in rows {
        let id_cell = format!("{:<19}", provider.id);
        let styled_id = if colorize {
            #[cfg(feature = "interactive")]
            {
                if provider.active {
                    style(&id_cell).blue().force_styling(true).to_string()
                } else {
                    style(&id_cell).cyan().force_styling(true).to_string()
                }
            }
            #[cfg(not(feature = "interactive"))]
            {
                id_cell.clone()
            }
        } else {
            id_cell
        };
        if !provider.aliases.is_empty() {
            writeln!(
                writer,
                "  {:<19} {}{}  (aliases: {})",
                styled_id,
                provider.description,
                if provider.active { " (active)" } else { "" },
                provider.aliases.join(", ")
            )
            .context("failed to write output")?;
        } else {
            writeln!(
                writer,
                "  {:<19} {}{}",
                styled_id,
                provider.description,
                if provider.active { " (active)" } else { "" }
            )
            .context("failed to write output")?;
        }
    }

    writeln!(writer, "\n  custom:<URL>   Any OpenAI-compatible endpoint")
        .context("failed to write output")?;
    writeln!(
        writer,
        "  anthropic-custom:<URL>  Any Anthropic-compatible endpoint"
    )
    .context("failed to write output")?;

    if !current.is_empty() && !active_known {
        writeln!(
            writer,
            "\nwarning: configured provider `{current}` is not in the supported catalog"
        )
        .context("failed to write output")?;
    }

    Ok(())
}

pub struct ProvidersQuotaOptions {
    pub provider: Option<String>,
    pub json: bool,
}

pub struct ProvidersQuotaCommand;

#[async_trait]
impl AgentZeroCommand for ProvidersQuotaCommand {
    type Options = ProvidersQuotaOptions;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let cfg = load(&ctx.config_path).ok();
        let provider_kind = opts
            .provider
            .clone()
            .or_else(|| cfg.as_ref().map(|c| c.provider.kind.clone()))
            .unwrap_or_else(|| "openrouter".to_string());

        let base_url = cfg
            .as_ref()
            .map(|c| c.provider.base_url.clone())
            .unwrap_or_default();
        let model = cfg
            .as_ref()
            .map(|c| c.provider.model.clone())
            .unwrap_or_default();

        let descriptor = agentzero_providers::find_provider(&provider_kind);
        let api_key_available = resolve_api_key(&provider_kind, ctx).is_some();

        let quota_info = QuotaInfo {
            provider: provider_kind.clone(),
            description: descriptor.map(|d| d.description).unwrap_or("unknown"),
            base_url,
            model,
            api_key_configured: api_key_available,
            quota_api_supported: supports_quota_api(&provider_kind),
        };

        if opts.json {
            println!("{}", serde_json::to_string_pretty(&quota_info)?);
        } else {
            println!(
                "Provider: {} ({})",
                quota_info.provider, quota_info.description
            );
            println!("  Base URL: {}", quota_info.base_url);
            println!("  Model: {}", quota_info.model);
            println!(
                "  API key: {}",
                if quota_info.api_key_configured {
                    "configured"
                } else {
                    "not found"
                }
            );
            println!(
                "  Quota API: {}",
                if quota_info.quota_api_supported {
                    "supported (use provider dashboard for live data)"
                } else {
                    "not available for this provider"
                }
            );
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct QuotaInfo {
    provider: String,
    description: &'static str,
    base_url: String,
    model: String,
    api_key_configured: bool,
    quota_api_supported: bool,
}

fn resolve_api_key(provider: &str, ctx: &CommandContext) -> Option<String> {
    // Check provider-specific env vars first.
    let env_keys = match provider {
        "openrouter" => &["OPENROUTER_API_KEY"][..],
        "anthropic" => &["ANTHROPIC_API_KEY"][..],
        "openai" => &["OPENAI_API_KEY"][..],
        "google" | "gemini" => &["GOOGLE_API_KEY", "GEMINI_API_KEY"][..],
        "groq" => &["GROQ_API_KEY"][..],
        "together" | "together-ai" => &["TOGETHER_API_KEY"][..],
        "deepseek" => &["DEEPSEEK_API_KEY"][..],
        "mistral" => &["MISTRAL_API_KEY"][..],
        "xai" | "grok" => &["XAI_API_KEY"][..],
        _ => &["AGENTZERO_API_KEY"][..],
    };

    for key in env_keys {
        if let Ok(val) = std::env::var(key) {
            if !val.is_empty() {
                return Some(val);
            }
        }
    }

    // Fall back to auth profiles.
    let manager = agentzero_auth::AuthManager::in_config_dir(&ctx.data_dir).ok()?;
    let cred = manager.resolve_credential(None, provider).ok()??;
    Some(cred.token)
}

fn supports_quota_api(provider: &str) -> bool {
    matches!(provider, "openrouter" | "anthropic" | "openai")
}

#[cfg(test)]
mod tests {
    use super::{render_providers_json, render_providers_table};
    #[cfg(feature = "interactive")]
    use console::strip_ansi_codes;
    use serde_json::Value;

    #[test]
    #[cfg(feature = "interactive")]
    fn render_providers_marks_configured_provider_as_active() {
        let mut out = Vec::new();
        render_providers_table(&mut out, Some("openrouter"), true).expect("render should succeed");
        let output = String::from_utf8(out).expect("output should be utf8");
        let plain = strip_ansi_codes(&output);

        assert!(plain.contains("Supported providers (36 total):"));
        assert!(plain.contains("  ID (use in config)  DESCRIPTION"));
        assert!(plain.contains("  openrouter          OpenRouter (active)"));
    }

    #[test]
    #[cfg(feature = "interactive")]
    fn render_providers_marks_alias_match_as_active() {
        let mut out = Vec::new();
        render_providers_table(&mut out, Some("github-copilot"), true)
            .expect("render should succeed");
        let output = String::from_utf8(out).expect("output should be utf8");
        let plain = strip_ansi_codes(&output);

        assert!(plain.contains("  copilot             GitHub Copilot (active)"));
    }

    #[test]
    #[cfg(feature = "interactive")]
    fn render_providers_warns_when_active_provider_is_unknown() {
        let mut out = Vec::new();
        render_providers_table(&mut out, Some("not-real"), true).expect("render should succeed");
        let output = String::from_utf8(out).expect("output should be utf8");
        let plain = strip_ansi_codes(&output);

        assert!(plain.contains("warning: configured provider `not-real`"));
    }

    #[test]
    #[cfg(feature = "interactive")]
    fn render_providers_colorizes_provider_id_column() {
        let mut out = Vec::new();
        render_providers_table(&mut out, Some("openrouter"), true).expect("render should succeed");
        let output = String::from_utf8(out).expect("output should be utf8");

        assert!(output.contains("\u{1b}[34m"));
        assert!(output.contains("\u{1b}[36m"));
        assert!(output.contains("openrouter"));
    }

    #[test]
    fn render_providers_no_color_emits_plain_text() {
        let mut out = Vec::new();
        render_providers_table(&mut out, Some("openrouter"), false).expect("render should succeed");
        let output = String::from_utf8(out).expect("output should be utf8");
        assert!(!output.contains("\u{1b}["));
        assert!(output.contains("  openrouter          OpenRouter (active)"));
    }

    #[test]
    fn render_providers_json_is_uncolored_and_includes_active_state() {
        let mut out = Vec::new();
        render_providers_json(&mut out, Some("openrouter")).expect("render should succeed");
        let output = String::from_utf8(out).expect("output should be utf8");
        assert!(!output.contains("\u{1b}["));

        let value: Value = serde_json::from_str(&output).expect("json output should parse");
        assert_eq!(value["total"].as_u64(), Some(36));
        assert_eq!(value["active_provider"].as_str(), Some("openrouter"));
        assert!(value["providers"]
            .as_array()
            .expect("providers should be array")
            .iter()
            .any(|provider| provider["id"] == "openrouter" && provider["active"] == true));
    }
}
