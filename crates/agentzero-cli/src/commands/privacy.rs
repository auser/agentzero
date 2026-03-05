use crate::cli::PrivacyCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use async_trait::async_trait;
use serde_json::json;

pub struct PrivacyCommand;

#[async_trait]
impl AgentZeroCommand for PrivacyCommand {
    type Options = PrivacyCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            PrivacyCommands::Status { json: json_output } => {
                let config = agentzero_config::load(&ctx.config_path)?;
                let mode = &config.privacy.mode;
                let noise_enabled = config.privacy.noise.enabled;
                let sealed_enabled = config.privacy.sealed_envelopes.enabled;
                let rotation_enabled = config.privacy.key_rotation.enabled;

                if json_output {
                    let output = json!({
                        "mode": mode,
                        "noise_enabled": noise_enabled,
                        "sealed_envelopes_enabled": sealed_enabled,
                        "key_rotation_enabled": rotation_enabled,
                        "rotation_interval_secs": config.privacy.key_rotation.rotation_interval_secs,
                        "max_noise_sessions": config.privacy.noise.max_sessions,
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else {
                    println!("Privacy mode: {mode}");
                    println!(
                        "  Noise Protocol E2E: {}",
                        if noise_enabled { "enabled" } else { "disabled" }
                    );
                    println!(
                        "  Sealed envelopes:   {}",
                        if sealed_enabled {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                    println!(
                        "  Key rotation:       {}",
                        if rotation_enabled {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                    if rotation_enabled {
                        println!(
                            "  Rotation interval:  {} seconds",
                            config.privacy.key_rotation.rotation_interval_secs
                        );
                    }
                }
                Ok(())
            }
            #[allow(unused_variables)]
            PrivacyCommands::RotateKeys {
                json: json_output,
                force,
            } => {
                #[cfg(feature = "privacy")]
                {
                    use agentzero_core::privacy::keyring::PrivacyKeyRing;
                    use agentzero_storage::crypto::KeyRingStore;

                    let store = KeyRingStore::in_data_dir(&ctx.data_dir)?;
                    let keypairs = store.load_keypairs()?;

                    let config = agentzero_config::load(&ctx.config_path)?;
                    let rotation_secs = config.privacy.key_rotation.rotation_interval_secs;
                    let overlap_secs = config.privacy.key_rotation.overlap_secs;

                    let mut keyring = if keypairs.is_empty() {
                        PrivacyKeyRing::new(rotation_secs, overlap_secs)
                    } else {
                        let kps: Vec<agentzero_core::privacy::keyring::IdentityKeyPair> = keypairs
                            .into_iter()
                            .map(|(epoch, pubkey, seckey, created_at)| {
                                let val = serde_json::json!({
                                    "epoch": epoch,
                                    "public_key": pubkey,
                                    "secret_key": seckey,
                                    "created_at": created_at,
                                });
                                serde_json::from_value(val).expect("keypair should deserialize")
                            })
                            .collect();
                        PrivacyKeyRing::from_persisted(kps, rotation_secs, overlap_secs)?
                    };

                    let rotated = if force {
                        keyring.force_rotate();
                        true
                    } else {
                        keyring.check_rotation().is_some()
                    };

                    // Always persist current state.
                    let all = keyring.all_keypairs();
                    let tuples: Vec<_> = all
                        .iter()
                        .map(|kp| (kp.epoch, kp.public_key, *kp.secret_key(), kp.created_at))
                        .collect();
                    store.save_keypairs(&tuples)?;

                    let current = keyring.current();
                    if json_output {
                        let output = json!({
                            "epoch": current.epoch,
                            "fingerprint": current.fingerprint(),
                            "rotated": rotated,
                            "next_rotation_at": keyring.next_rotation_at(),
                        });
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    } else if rotated {
                        println!(
                            "Key rotated. Epoch: {}, Fingerprint: {}",
                            current.epoch,
                            current.fingerprint()
                        );
                    } else {
                        println!(
                            "No rotation needed (key is fresh). Epoch: {}, Fingerprint: {}",
                            current.epoch,
                            current.fingerprint()
                        );
                        if let Some(next) = keyring.next_rotation_at() {
                            println!("  Next rotation at: {next} (unix timestamp)");
                        }
                    }
                    return Ok(());
                }

                #[cfg(not(feature = "privacy"))]
                {
                    anyhow::bail!("privacy feature not enabled. Rebuild with --features privacy");
                }
            }
            #[allow(unused_variables)]
            PrivacyCommands::GenerateKeypair { json: json_output } => {
                #[cfg(feature = "privacy")]
                {
                    use agentzero_core::privacy::keyring::IdentityKeyPair;

                    let kp = IdentityKeyPair::generate(0);
                    if json_output {
                        let output = json!({
                            "epoch": kp.epoch,
                            "fingerprint": kp.fingerprint(),
                            "routing_id": hex_encode(&kp.routing_id()),
                        });
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    } else {
                        println!("Generated keypair:");
                        println!("  Fingerprint: {}", kp.fingerprint());
                        println!("  Routing ID:  {}", hex_encode(&kp.routing_id()));
                        println!("  (Not persisted — use rotate-keys to activate)");
                    }
                    return Ok(());
                }

                #[cfg(not(feature = "privacy"))]
                {
                    anyhow::bail!("privacy feature not enabled. Rebuild with --features privacy");
                }
            }
        }
    }
}

#[cfg(feature = "privacy")]
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use crate::parse_cli_from;

    #[test]
    fn privacy_status_parses() {
        let cli = parse_cli_from(["agentzero", "privacy", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            crate::cli::Commands::Privacy {
                command: crate::cli::PrivacyCommands::Status { json: false }
            }
        ));
    }

    #[test]
    fn privacy_rotate_keys_parses() {
        let cli = parse_cli_from(["agentzero", "privacy", "rotate-keys", "--json"]).unwrap();
        assert!(matches!(
            cli.command,
            crate::cli::Commands::Privacy {
                command: crate::cli::PrivacyCommands::RotateKeys {
                    json: true,
                    force: false,
                }
            }
        ));
    }

    #[test]
    fn privacy_rotate_keys_force_parses() {
        let cli = parse_cli_from(["agentzero", "privacy", "rotate-keys", "--force"]).unwrap();
        assert!(matches!(
            cli.command,
            crate::cli::Commands::Privacy {
                command: crate::cli::PrivacyCommands::RotateKeys {
                    json: false,
                    force: true,
                }
            }
        ));
    }

    #[test]
    fn privacy_generate_keypair_parses() {
        let cli = parse_cli_from(["agentzero", "privacy", "generate-keypair"]).unwrap();
        assert!(matches!(
            cli.command,
            crate::cli::Commands::Privacy {
                command: crate::cli::PrivacyCommands::GenerateKeypair { json: false }
            }
        ));
    }
}
