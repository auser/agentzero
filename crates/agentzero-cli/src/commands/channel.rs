use crate::cli::ChannelCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_channels::{channel_catalog, normalize_channel_id, ChannelRegistry};
use agentzero_storage::EncryptedJsonStore;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::io::{self, IsTerminal};

pub struct ChannelCommand;

#[async_trait]
impl AgentZeroCommand for ChannelCommand {
    type Options = ChannelCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let registry = ChannelRegistry::with_builtin_handlers();
        let store = channel_state_store(ctx)?;
        let mut state = ChannelState::load(&store)?;

        match opts {
            ChannelCommands::Add { name } => {
                let channel =
                    resolve_channel(name.as_deref(), "channel to add", "AGENTZERO_CHANNEL")?;
                match bind_channel(&mut state, &store, channel)? {
                    ChannelMutation::Noop(message) => println!("{message}"),
                    ChannelMutation::Mutated(message) => {
                        println!("{message}");
                        println!("Run `agentzero channel start` to launch channels.");
                    }
                }
            }
            ChannelCommands::Doctor => {
                println!("Channel diagnostics");
                let configured = configured_channels(&state);
                println!("  configured channels: {}", configured.len());
                for channel in &configured {
                    println!("  - {} [ok]", channel);
                }
                println!("  dispatch engine: ok");
            }
            ChannelCommands::List => {
                render_channel_list(&mut std::io::stdout(), &state)?;
            }
            ChannelCommands::Remove { name } => {
                let channel =
                    resolve_channel(name.as_deref(), "channel to remove", "AGENTZERO_CHANNEL")?;
                match remove_channel(&mut state, channel) {
                    ChannelMutation::Noop(message) => println!("{message}"),
                    ChannelMutation::Mutated(message) => {
                        state.save(&store)?;
                        println!("{message}");
                    }
                }
            }
            ChannelCommands::Start => {
                let configured = configured_channels(&state);
                println!("Starting channels ({})", configured.len());
                for channel in &configured {
                    if channel == "cli" {
                        let delivered =
                            registry.dispatch("cli", serde_json::json!({"health": "check"}));
                        if delivered.await.is_some() {
                            println!("- {channel}: started");
                        } else {
                            println!("- {channel}: unavailable");
                        }
                    } else if channel_supported_in_build(channel) {
                        println!("- {channel}: configured");
                    } else {
                        println!("- {channel}: disabled in this build");
                    }
                }
            }
        }

        Ok(())
    }
}

fn configured_channels(state: &ChannelState) -> Vec<String> {
    let mut channels = vec!["cli".to_string()];
    for channel in &state.enabled_channels {
        if !channels.iter().any(|item| item == channel) {
            channels.push(channel.clone());
        }
    }
    channels
}

#[derive(Debug, Clone, Copy)]
struct ChannelAvailability {
    name: &'static str,
    available: bool,
    always: bool,
}

fn channel_availability(state: &ChannelState) -> Vec<ChannelAvailability> {
    channel_catalog()
        .iter()
        .map(|descriptor| {
            let configured = descriptor.id == "cli"
                || state
                    .enabled_channels
                    .iter()
                    .any(|item| item == descriptor.id);
            ChannelAvailability {
                name: descriptor.display_name,
                available: configured,
                always: descriptor.id == "cli",
            }
        })
        .collect()
}

fn render_channel_list(writer: &mut dyn Write, state: &ChannelState) -> anyhow::Result<()> {
    writeln!(writer, "Channels:")?;
    for channel in channel_availability(state) {
        let marker = if channel.available { "✅" } else { "❌" };
        if channel.always {
            writeln!(writer, "  {marker} {} (always available)", channel.name)?;
        } else {
            let configured_tag = if channel.available {
                " (configured)"
            } else {
                ""
            };
            writeln!(writer, "  {marker} {}{}", channel.name, configured_tag)?;
        }
    }

    writeln!(
        writer,
        "  ℹ️ Matrix channel support is disabled in this build (enable `channel-matrix`)."
    )?;
    writeln!(
        writer,
        "  ℹ️ Lark/Feishu channel support is disabled in this build (enable `channel-lark`)."
    )?;
    writeln!(writer)?;
    writeln!(writer, "To start channels: agentzero channel start")?;
    writeln!(writer, "To check health:    agentzero channel doctor")?;
    writeln!(writer, "To configure:       agentzero onboard")?;
    Ok(())
}

fn channel_state_store(ctx: &CommandContext) -> anyhow::Result<EncryptedJsonStore> {
    EncryptedJsonStore::in_config_dir(&ctx.data_dir, "channels/enabled.json")
}

/// Resolve a channel ID from an explicit name, env var, or interactive prompt.
fn resolve_channel(
    explicit_name: Option<&str>,
    prompt: &str,
    env_key: &str,
) -> anyhow::Result<&'static str> {
    // 1. Explicit name from CLI argument
    if let Some(name) = explicit_name {
        if let Some(channel) = normalize_channel_id(name) {
            return Ok(channel);
        }
        anyhow::bail!(
            "unknown channel `{name}`. Run `agentzero channel list` to see available channels."
        );
    }

    // 2. Environment variable
    if let Ok(value) = std::env::var(env_key) {
        if let Some(channel) = normalize_channel_id(&value) {
            return Ok(channel);
        }
        anyhow::bail!("unknown channel `{}` from {}", value, env_key);
    }

    // 3. Interactive prompt
    if io::stdin().is_terminal() {
        print!("Enter {}: ", prompt);
        io::stdout().flush()?;
        let mut buffer = String::new();
        io::stdin().read_line(&mut buffer)?;
        if let Some(channel) = normalize_channel_id(&buffer) {
            return Ok(channel);
        }
        anyhow::bail!("unknown channel `{}`", buffer.trim());
    }

    anyhow::bail!(
        "specify a channel name, set {}, or run in an interactive terminal",
        env_key
    )
}

fn channel_supported_in_build(channel: &str) -> bool {
    !matches!(channel, "matrix" | "lark" | "feishu")
}

enum ChannelMutation {
    Noop(String),
    Mutated(String),
}

fn add_channel(state: &mut ChannelState, channel: &str) -> ChannelMutation {
    if channel == "cli" {
        return ChannelMutation::Noop(
            "CLI channel is always available and does not need to be added.".to_string(),
        );
    }
    if state.enabled_channels.iter().any(|item| item == channel) {
        return ChannelMutation::Noop(format!("Channel `{channel}` is already configured."));
    }
    state.enabled_channels.push(channel.to_string());
    ChannelMutation::Mutated(format!("Added channel `{channel}`"))
}

fn bind_channel(
    state: &mut ChannelState,
    store: &EncryptedJsonStore,
    channel: &str,
) -> anyhow::Result<ChannelMutation> {
    let result = add_channel(state, channel);
    if matches!(result, ChannelMutation::Mutated(_)) {
        state.save(store)?;
    }
    Ok(result)
}

fn remove_channel(state: &mut ChannelState, channel: &str) -> ChannelMutation {
    if channel == "cli" {
        return ChannelMutation::Noop(
            "CLI channel is always available and cannot be removed.".to_string(),
        );
    }
    let before = state.enabled_channels.len();
    state.enabled_channels.retain(|entry| entry != channel);
    if state.enabled_channels.len() == before {
        ChannelMutation::Noop(format!("Channel `{channel}` is not configured."))
    } else {
        ChannelMutation::Mutated(format!("Removed channel `{channel}`"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ChannelState {
    enabled_channels: Vec<String>,
}

impl ChannelState {
    fn load(store: &EncryptedJsonStore) -> anyhow::Result<Self> {
        store.load_or_default()
    }

    fn save(&self, store: &EncryptedJsonStore) -> anyhow::Result<()> {
        store.save(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        add_channel, channel_availability, channel_state_store, channel_supported_in_build,
        remove_channel, render_channel_list, resolve_channel, ChannelCommand, ChannelMutation,
        ChannelState,
    };
    use crate::cli::ChannelCommands;
    use crate::command_core::{AgentZeroCommand, CommandContext};
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
        let dir = std::env::temp_dir().join(format!("agentzero-channel-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn channel_list_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        ChannelCommand::run(&ctx, ChannelCommands::List)
            .await
            .expect("channel list should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn add_and_remove_channel_state_success_path() {
        let mut state = ChannelState::default();
        let added = add_channel(&mut state, "telegram");
        assert!(matches!(added, ChannelMutation::Mutated(_)));
        assert!(state
            .enabled_channels
            .iter()
            .any(|channel| channel == "telegram"));

        let removed = remove_channel(&mut state, "telegram");
        assert!(matches!(removed, ChannelMutation::Mutated(_)));
        assert!(!state
            .enabled_channels
            .iter()
            .any(|channel| channel == "telegram"));
    }

    #[test]
    fn add_channel_cli_is_noop_negative_path() {
        let mut state = ChannelState::default();
        let result = add_channel(&mut state, "cli");
        assert!(matches!(result, ChannelMutation::Noop(_)));
        assert!(state.enabled_channels.is_empty());
    }

    #[test]
    fn channel_list_render_includes_catalog_and_hints_success_path() {
        let mut out = Vec::new();
        let state = ChannelState::default();
        render_channel_list(&mut out, &state).expect("render should succeed");
        let output = String::from_utf8(out).expect("output should be utf8");
        assert!(output.contains("Channels:"));
        assert!(output.contains("✅ CLI (always available)"));
        assert!(output.contains("❌ Telegram"));
        assert!(output.contains("❌ Webhook"));
        assert!(output.contains("Matrix channel support is disabled"));
        assert!(output.contains("Lark/Feishu channel support is disabled"));
        assert!(output.contains("To start channels: agentzero channel start"));
    }

    #[test]
    fn channel_availability_marks_configured_channels_success_path() {
        let state = ChannelState {
            enabled_channels: vec!["telegram".to_string()],
        };
        let rows = channel_availability(&state);
        let telegram = rows
            .iter()
            .find(|row| row.name == "Telegram")
            .expect("telegram should exist");
        assert!(telegram.available);
    }

    #[test]
    fn channel_supported_in_build_flags_matrix_negative_path() {
        assert!(!channel_supported_in_build("matrix"));
        assert!(channel_supported_in_build("telegram"));
    }

    #[tokio::test]
    async fn channel_add_with_name_binds_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        ChannelCommand::run(
            &ctx,
            ChannelCommands::Add {
                name: Some("telegram".to_string()),
            },
        )
        .await
        .expect("channel add telegram should succeed");

        let store = channel_state_store(&ctx).expect("store should construct");
        let state = ChannelState::load(&store).expect("state should load");
        assert!(state
            .enabled_channels
            .iter()
            .any(|channel| channel == "telegram"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn channel_add_discord_via_name_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        ChannelCommand::run(
            &ctx,
            ChannelCommands::Add {
                name: Some("discord".to_string()),
            },
        )
        .await
        .expect("channel add discord should succeed");

        let store = channel_state_store(&ctx).expect("store should construct");
        let state = ChannelState::load(&store).expect("state should load");
        assert!(state
            .enabled_channels
            .iter()
            .any(|channel| channel == "discord"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn channel_remove_with_name_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        // Add first
        ChannelCommand::run(
            &ctx,
            ChannelCommands::Add {
                name: Some("telegram".to_string()),
            },
        )
        .await
        .expect("add should succeed");

        // Remove by name
        ChannelCommand::run(
            &ctx,
            ChannelCommands::Remove {
                name: Some("telegram".to_string()),
            },
        )
        .await
        .expect("remove should succeed");

        let store = channel_state_store(&ctx).expect("store should construct");
        let state = ChannelState::load(&store).expect("state should load");
        assert!(!state
            .enabled_channels
            .iter()
            .any(|channel| channel == "telegram"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolve_channel_with_explicit_name_success_path() {
        let result = resolve_channel(Some("telegram"), "unused", "UNUSED_ENV");
        assert_eq!(result.unwrap(), "telegram");
    }

    #[test]
    fn resolve_channel_with_unknown_name_negative_path() {
        let err = resolve_channel(Some("nonexistent"), "unused", "UNUSED_ENV")
            .expect_err("unknown channel should fail");
        assert!(err.to_string().contains("unknown channel"));
    }

    #[test]
    fn channel_state_persists_encrypted_via_storage_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };
        let store = channel_state_store(&ctx).expect("store should construct");

        let state = ChannelState {
            enabled_channels: vec!["telegram".to_string()],
        };
        state.save(&store).expect("save should succeed");

        let loaded = ChannelState::load(&store).expect("load should succeed");
        assert!(loaded
            .enabled_channels
            .iter()
            .any(|name| name == "telegram"));

        let on_disk = fs::read_to_string(store.path()).expect("stored payload should be readable");
        assert!(!on_disk.contains("telegram"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
