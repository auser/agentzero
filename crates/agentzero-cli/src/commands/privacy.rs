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
            PrivacyCommands::Test { json: json_output } => {
                return run_privacy_tests(ctx, json_output).await;
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

/// Run privacy diagnostic checks and report pass/fail.
async fn run_privacy_tests(ctx: &CommandContext, json_output: bool) -> anyhow::Result<()> {
    use agentzero_core::common::privacy_helpers::{boundary_allows_recall, resolve_boundary};
    use serde_json::json;

    struct CheckResult {
        name: &'static str,
        passed: bool,
        detail: String,
    }

    let mut results = Vec::new();

    // Check 1: Config validation
    let config_check = match agentzero_config::load(&ctx.config_path) {
        Ok(cfg) => {
            let mode = &cfg.privacy.mode;
            let valid = ["", "any", "encrypted_only", "local_only"].contains(&mode.as_str());
            CheckResult {
                name: "config_validation",
                passed: valid,
                detail: if valid {
                    format!("privacy mode '{mode}' is valid")
                } else {
                    format!("privacy mode '{mode}' is not a recognized boundary")
                },
            }
        }
        Err(e) => CheckResult {
            name: "config_validation",
            passed: false,
            detail: format!("failed to load config: {e}"),
        },
    };
    results.push(config_check);

    // Check 2: Boundary resolution
    {
        let ok1 = resolve_boundary("any", "local_only") == "local_only";
        let ok2 = resolve_boundary("encrypted_only", "local_only") == "local_only";
        let ok3 = resolve_boundary("any", "encrypted_only") == "encrypted_only";
        let ok4 = resolve_boundary("", "any") == "any";
        let passed = ok1 && ok2 && ok3 && ok4;
        results.push(CheckResult {
            name: "boundary_resolution",
            passed,
            detail: if passed {
                "child ≤ parent rule holds for all test cases".to_string()
            } else {
                "boundary resolution failed one or more checks".to_string()
            },
        });
    }

    // Check 3: Memory boundary isolation
    {
        let ok1 = boundary_allows_recall("local_only", "local_only");
        let ok2 = !boundary_allows_recall("local_only", "any");
        let ok3 = boundary_allows_recall("encrypted_only", "encrypted_only");
        let ok4 = !boundary_allows_recall("encrypted_only", "any");
        let ok5 = boundary_allows_recall("", "any");
        let passed = ok1 && ok2 && ok3 && ok4 && ok5;
        results.push(CheckResult {
            name: "memory_boundary_isolation",
            passed,
            detail: if passed {
                "recall filtering rules pass all checks".to_string()
            } else {
                "recall filtering rules failed one or more checks".to_string()
            },
        });
    }

    // Check 4: Sealed envelope round-trip (privacy feature only)
    #[cfg(feature = "privacy")]
    {
        use agentzero_core::privacy::envelope::{self, SealedEnvelope};

        let test_data = b"privacy test payload";
        let (pubkey, secret_bytes) = envelope::generate_keypair();
        let env = SealedEnvelope::seal(&pubkey, test_data, 300);
        match env.open(&secret_bytes) {
            Ok(plaintext) if plaintext == test_data => {
                results.push(CheckResult {
                    name: "sealed_envelope_roundtrip",
                    passed: true,
                    detail: "seal + open round-trip verified".to_string(),
                });
            }
            Ok(_) => {
                results.push(CheckResult {
                    name: "sealed_envelope_roundtrip",
                    passed: false,
                    detail: "decrypted data does not match original".to_string(),
                });
            }
            Err(e) => {
                results.push(CheckResult {
                    name: "sealed_envelope_roundtrip",
                    passed: false,
                    detail: format!("open failed: {e}"),
                });
            }
        }
    }
    #[cfg(not(feature = "privacy"))]
    {
        results.push(CheckResult {
            name: "sealed_envelope_roundtrip",
            passed: false,
            detail: "privacy feature not compiled in".to_string(),
        });
    }

    // Check 5: Noise XX handshake (in-process)
    #[cfg(feature = "privacy")]
    {
        use agentzero_core::privacy::noise::{NoiseHandshaker, NoiseKeypair};
        use agentzero_core::privacy::noise_client::NoiseClientHandshake;
        use base64::{engine::general_purpose::STANDARD, Engine as _};

        let passed = (|| -> anyhow::Result<bool> {
            let server_kp = NoiseKeypair::generate()?;
            let mut client = NoiseClientHandshake::new()?;
            let step1 = client.step1()?;

            let mut server = NoiseHandshaker::new_responder("XX", &server_kp)?;
            let mut buf = [0u8; 65535];
            server.read_message(&STANDARD.decode(&step1)?, &mut buf)?;
            let len = server.write_message(b"", &mut buf)?;
            let resp = STANDARD.encode(&buf[..len]);

            client.process_step1_response(&resp)?;
            let _step2 = client.step2()?;
            Ok(true)
        })()
        .unwrap_or(false);

        results.push(CheckResult {
            name: "noise_xx_handshake",
            passed,
            detail: if passed {
                "XX handshake completed in-process".to_string()
            } else {
                "XX handshake failed".to_string()
            },
        });
    }
    #[cfg(not(feature = "privacy"))]
    {
        results.push(CheckResult {
            name: "noise_xx_handshake",
            passed: false,
            detail: "privacy feature not compiled in".to_string(),
        });
    }

    // Check 6: Noise IK handshake (in-process)
    #[cfg(feature = "privacy")]
    {
        use agentzero_core::privacy::noise::{NoiseHandshaker, NoiseKeypair};
        use agentzero_core::privacy::noise_client::NoiseClientHandshake;
        use base64::{engine::general_purpose::STANDARD, Engine as _};

        let passed = (|| -> anyhow::Result<bool> {
            let server_kp = NoiseKeypair::generate()?;
            let mut client = NoiseClientHandshake::new_ik(&server_kp.public)?;
            let step1 = client.step1()?;

            let mut server = NoiseHandshaker::new_responder("IK", &server_kp)?;
            let mut buf = [0u8; 65535];
            server.read_message(&STANDARD.decode(&step1)?, &mut buf)?;
            let len = server.write_message(b"", &mut buf)?;
            let resp = STANDARD.encode(&buf[..len]);

            client.process_step1_response(&resp)?;
            let session = client.finish("ik-test".to_string())?;
            Ok(session.session_id() == "ik-test")
        })()
        .unwrap_or(false);

        results.push(CheckResult {
            name: "noise_ik_handshake",
            passed,
            detail: if passed {
                "IK handshake completed in-process".to_string()
            } else {
                "IK handshake failed".to_string()
            },
        });
    }
    #[cfg(not(feature = "privacy"))]
    {
        results.push(CheckResult {
            name: "noise_ik_handshake",
            passed: false,
            detail: "privacy feature not compiled in".to_string(),
        });
    }

    // Check 7: Channel locality
    {
        use agentzero_channels::is_local_channel;
        let ok = is_local_channel("cli") && !is_local_channel("telegram");
        results.push(CheckResult {
            name: "channel_locality",
            passed: ok,
            detail: if ok {
                "local/non-local channel classification correct".to_string()
            } else {
                "channel locality classification incorrect".to_string()
            },
        });
    }

    // Check 8: Encrypted store round-trip
    {
        let dir = std::env::temp_dir().join(format!("az-privacy-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let passed = (|| -> anyhow::Result<bool> {
            let store =
                agentzero_storage::EncryptedJsonStore::in_config_dir(&dir, "privacy-test.json")?;
            store.save(&json!({"test": true}))?;
            let loaded: Option<serde_json::Value> = store.load_optional()?;
            store.delete()?;
            Ok(loaded == Some(json!({"test": true})))
        })()
        .unwrap_or(false);
        std::fs::remove_dir_all(&dir).ok();
        results.push(CheckResult {
            name: "encrypted_store_roundtrip",
            passed,
            detail: if passed {
                "encrypted JSON store save/load/delete verified".to_string()
            } else {
                "encrypted store round-trip failed".to_string()
            },
        });
    }

    // Output
    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let all_passed = passed == total;

    if json_output {
        let checks: Vec<_> = results
            .iter()
            .map(|r| {
                json!({
                    "name": r.name,
                    "passed": r.passed,
                    "detail": r.detail,
                })
            })
            .collect();
        let output = json!({
            "passed": passed,
            "total": total,
            "all_passed": all_passed,
            "checks": checks,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Privacy Diagnostic Checks ({passed}/{total} passed)\n");
        for r in &results {
            let icon = if r.passed { "PASS" } else { "FAIL" };
            println!("  [{icon}] {}: {}", r.name, r.detail);
        }
        if all_passed {
            println!("\nAll checks passed.");
        } else {
            println!("\nSome checks failed. Review the output above.");
        }
    }

    if all_passed {
        Ok(())
    } else {
        anyhow::bail!("{} of {total} privacy checks failed", total - passed)
    }
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

    #[test]
    fn privacy_test_parses() {
        let cli = parse_cli_from(["agentzero", "privacy", "test"]).unwrap();
        assert!(matches!(
            cli.command,
            crate::cli::Commands::Privacy {
                command: crate::cli::PrivacyCommands::Test { json: false }
            }
        ));
    }

    #[test]
    fn privacy_test_json_parses() {
        let cli = parse_cli_from(["agentzero", "privacy", "test", "--json"]).unwrap();
        assert!(matches!(
            cli.command,
            crate::cli::Commands::Privacy {
                command: crate::cli::PrivacyCommands::Test { json: true }
            }
        ));
    }
}
