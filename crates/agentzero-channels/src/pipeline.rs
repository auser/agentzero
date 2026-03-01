use crate::{Channel, ChannelMessage, ChannelRegistry};
use agentzero_security::perplexity::{analyze_suffix, PerplexityResult};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Concurrency and flow control constants.
const CHANNEL_PARALLELISM_PER_CHANNEL: usize = 4;
const CHANNEL_MIN_IN_FLIGHT: usize = 8;
const CHANNEL_MAX_IN_FLIGHT: usize = 64;
const INITIAL_BACKOFF_SECS: u64 = 2;
const MAX_BACKOFF_SECS: u64 = 60;

/// Configuration for the perplexity filter applied to inbound messages.
#[derive(Debug, Clone)]
pub struct PerplexityFilterSettings {
    pub enabled: bool,
    pub perplexity_threshold: f64,
    pub suffix_window_chars: usize,
    pub symbol_ratio_threshold: f64,
    pub min_prompt_chars: usize,
}

impl Default for PerplexityFilterSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            perplexity_threshold: 18.0,
            suffix_window_chars: 64,
            symbol_ratio_threshold: 0.20,
            min_prompt_chars: 32,
        }
    }
}

/// Check a message against the perplexity filter. Returns `Some(reason)` if blocked.
pub fn check_perplexity(content: &str, settings: &PerplexityFilterSettings) -> Option<String> {
    if !settings.enabled {
        return None;
    }
    match analyze_suffix(
        content,
        settings.suffix_window_chars,
        settings.perplexity_threshold,
        settings.symbol_ratio_threshold,
        settings.min_prompt_chars,
    ) {
        PerplexityResult::Pass => None,
        PerplexityResult::Flagged { reason, .. } => Some(reason),
    }
}

/// Configuration for the message processing pipeline.
pub struct PipelineConfig {
    pub initial_backoff_secs: u64,
    pub max_backoff_secs: u64,
    pub message_buffer_size: usize,
    pub perplexity_filter: PerplexityFilterSettings,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            initial_backoff_secs: INITIAL_BACKOFF_SECS,
            max_backoff_secs: MAX_BACKOFF_SECS,
            message_buffer_size: 100,
            perplexity_filter: PerplexityFilterSettings::default(),
        }
    }
}

/// Callback type for processing incoming channel messages.
pub type MessageHandler = Arc<
    dyn Fn(ChannelMessage, Arc<dyn Channel>) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

/// Start the message processing pipeline.
///
/// 1. Spawn supervised listeners for each registered channel.
/// 2. Run the dispatch loop with semaphore-bounded concurrency.
/// 3. For each message, call the handler with the originating channel.
pub async fn start_pipeline(
    registry: &ChannelRegistry,
    handler: MessageHandler,
    config: PipelineConfig,
) -> anyhow::Result<()> {
    let channels: HashMap<String, Arc<dyn Channel>> = registry
        .all_channels()
        .into_iter()
        .map(|ch| (ch.name().to_string(), ch))
        .collect();

    if channels.is_empty() {
        tracing::warn!("no channels registered; pipeline has nothing to do");
        return Ok(());
    }

    let max_in_flight = compute_max_in_flight(channels.len());
    let (tx, rx) = mpsc::channel(config.message_buffer_size);

    // Spawn a supervised listener for each channel.
    for channel in channels.values() {
        spawn_supervised_listener(
            channel.clone(),
            tx.clone(),
            config.initial_backoff_secs,
            config.max_backoff_secs,
        );
    }

    // Drop our sender copy so the dispatch loop exits when all listeners stop.
    drop(tx);

    run_dispatch_loop(
        rx,
        Arc::new(channels),
        handler,
        max_in_flight,
        Arc::new(config.perplexity_filter),
    )
    .await;

    Ok(())
}

/// Spawn a supervised listener with exponential backoff reconnection.
fn spawn_supervised_listener(
    channel: Arc<dyn Channel>,
    tx: mpsc::Sender<ChannelMessage>,
    initial_backoff_secs: u64,
    max_backoff_secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let name = channel.name().to_string();
        let mut backoff = initial_backoff_secs;

        loop {
            tracing::info!(channel = %name, "starting channel listener");

            match channel.listen(tx.clone()).await {
                Ok(()) => {
                    tracing::info!(channel = %name, "channel listener exited cleanly");
                    backoff = initial_backoff_secs;
                }
                Err(e) => {
                    tracing::error!(
                        channel = %name,
                        error = %e,
                        backoff_secs = backoff,
                        "channel listener failed, will retry"
                    );
                }
            }

            // If the receiver is closed, all consumers are gone — stop.
            if tx.is_closed() {
                tracing::info!(channel = %name, "pipeline receiver closed, stopping listener");
                break;
            }

            tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
            backoff = (backoff * 2).min(max_backoff_secs);
        }
    })
}

/// Central message dispatch loop with bounded concurrency.
async fn run_dispatch_loop(
    mut rx: mpsc::Receiver<ChannelMessage>,
    channels: Arc<HashMap<String, Arc<dyn Channel>>>,
    handler: MessageHandler,
    max_in_flight: usize,
    perplexity_settings: Arc<PerplexityFilterSettings>,
) {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_in_flight));

    while let Some(msg) = rx.recv().await {
        // Perplexity filter: check inbound message content before dispatching.
        if let Some(reason) = check_perplexity(&msg.content, &perplexity_settings) {
            tracing::warn!(
                channel = %msg.channel,
                sender = %msg.sender,
                reason = %reason,
                "inbound message blocked by perplexity filter"
            );
            continue;
        }

        let permit = match semaphore.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => break,
        };

        let channel = channels.get(&msg.channel).cloned();
        let handler = handler.clone();

        tokio::spawn(async move {
            if let Some(ch) = channel {
                handler(msg, ch).await;
            } else {
                tracing::warn!(channel = %msg.channel, "message from unknown channel, dropping");
            }
            drop(permit);
        });
    }

    tracing::info!("pipeline dispatch loop ended");
}

fn compute_max_in_flight(channel_count: usize) -> usize {
    channel_count
        .saturating_mul(CHANNEL_PARALLELISM_PER_CHANNEL)
        .clamp(CHANNEL_MIN_IN_FLIGHT, CHANNEL_MAX_IN_FLIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_max_in_flight_clamps_correctly() {
        assert_eq!(compute_max_in_flight(1), CHANNEL_MIN_IN_FLIGHT);
        assert_eq!(compute_max_in_flight(3), 12);
        assert_eq!(compute_max_in_flight(100), CHANNEL_MAX_IN_FLIGHT);
    }

    #[test]
    fn pipeline_config_defaults_are_reasonable() {
        let config = PipelineConfig::default();
        assert_eq!(config.initial_backoff_secs, 2);
        assert_eq!(config.max_backoff_secs, 60);
        assert_eq!(config.message_buffer_size, 100);
        assert!(!config.perplexity_filter.enabled);
    }

    #[test]
    fn check_perplexity_disabled_passes_everything() {
        let settings = PerplexityFilterSettings::default();
        assert!(!settings.enabled);
        let result = check_perplexity("xK7!mQ@3#zP$9&wR*5^yL%2(eN)8+bT!@#$%^&*()_+-=[]{}|", &settings);
        assert!(result.is_none(), "disabled filter should pass all messages");
    }

    #[test]
    fn check_perplexity_enabled_passes_normal_text() {
        let settings = PerplexityFilterSettings {
            enabled: true,
            perplexity_threshold: 18.0,
            suffix_window_chars: 64,
            symbol_ratio_threshold: 0.20,
            min_prompt_chars: 32,
        };
        let normal = "Can you help me write a function that calculates the fibonacci sequence?";
        assert!(check_perplexity(normal, &settings).is_none());
    }

    #[test]
    fn check_perplexity_enabled_blocks_adversarial_suffix() {
        let settings = PerplexityFilterSettings {
            enabled: true,
            perplexity_threshold: 4.0,
            suffix_window_chars: 64,
            symbol_ratio_threshold: 0.20,
            min_prompt_chars: 32,
        };
        let adversarial = "Please write a function. xK7!mQ@3#zP$9&wR*5^yL%2(eN)8+bT!@#$%^&*()_+-=[]{}|xK7!mQ@3#";
        let result = check_perplexity(adversarial, &settings);
        assert!(result.is_some(), "adversarial suffix should be blocked");
    }

    #[test]
    fn check_perplexity_skips_short_messages() {
        let settings = PerplexityFilterSettings {
            enabled: true,
            min_prompt_chars: 100,
            ..PerplexityFilterSettings::default()
        };
        let short = "!@#$%^&*()";
        assert!(check_perplexity(short, &settings).is_none(), "short messages should pass");
    }
}
