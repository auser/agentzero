use crate::cli::{EstopCommands, EstopLevel};
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_core::security::otp;
use agentzero_storage::EncryptedJsonStore;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

const ESTOP_STATE_FILE: &str = "estop-state.json";
const OTP_SECRET_FILE: &str = "otp-secret.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct EstopState {
    engaged: bool,
    level: String,
    domains: Vec<String>,
    tools: Vec<String>,
    engaged_at_epoch_secs: Option<u64>,
    resumed_at_epoch_secs: Option<u64>,
    #[serde(default)]
    require_otp_to_resume: bool,
}

impl Default for EstopState {
    fn default() -> Self {
        Self {
            engaged: false,
            level: "kill-all".to_string(),
            domains: Vec::new(),
            tools: Vec::new(),
            engaged_at_epoch_secs: None,
            resumed_at_epoch_secs: None,
            require_otp_to_resume: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OtpSecret {
    secret: Vec<u8>,
}

pub struct EstopOptions {
    pub level: Option<EstopLevel>,
    pub domains: Vec<String>,
    pub tools: Vec<String>,
    pub require_otp: bool,
    pub command: Option<EstopCommands>,
}

pub struct EstopCommand;

#[async_trait]
impl AgentZeroCommand for EstopCommand {
    type Options = EstopOptions;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let store = EncryptedJsonStore::in_config_dir(&ctx.data_dir, ESTOP_STATE_FILE)?;
        let mut state = store.load_or_default::<EstopState>()?;

        match opts.command {
            Some(EstopCommands::Status) => {
                print_status(&state);
            }
            Some(EstopCommands::Resume {
                network,
                domains,
                tools,
                otp: otp_code,
            }) => {
                if !state.engaged {
                    anyhow::bail!("emergency stop is not engaged; run `agentzero estop` first");
                }

                // If OTP is required to resume, validate the code
                if state.require_otp_to_resume {
                    let otp_store =
                        EncryptedJsonStore::in_config_dir(&ctx.data_dir, OTP_SECRET_FILE)?;
                    let otp_secret: OtpSecret = otp_store.load_optional()?.ok_or_else(|| {
                        anyhow::anyhow!(
                            "OTP secret not found; run `agentzero estop` with --require-otp first"
                        )
                    })?;
                    let code = otp_code.as_deref().ok_or_else(|| {
                        anyhow::anyhow!("OTP code required to resume; pass --otp <code>")
                    })?;
                    let valid = otp::validate_totp(code, &otp_secret.secret, 30, 6, 1)?;
                    if !valid {
                        anyhow::bail!("invalid OTP code; resume denied");
                    }
                }

                let has_scoped_resume = network || !domains.is_empty() || !tools.is_empty();

                if !has_scoped_resume {
                    disengage_all(&mut state);
                } else {
                    apply_scoped_resume(&mut state, network, &domains, &tools);
                }

                state.resumed_at_epoch_secs = Some(now_epoch_secs());
                store.save(&state)?;
                println!(
                    "Emergency stop {}",
                    if state.engaged {
                        "partially resumed"
                    } else {
                        "resumed"
                    }
                );
            }
            None => {
                let level = opts.level.unwrap_or(EstopLevel::KillAll);
                state.engaged = true;
                state.level = level_to_str(level).to_string();
                state.domains = opts.domains;
                state.tools = opts.tools;
                state.engaged_at_epoch_secs = Some(now_epoch_secs());
                state.resumed_at_epoch_secs = None;
                state.require_otp_to_resume = opts.require_otp;

                if opts.require_otp {
                    let otp_store =
                        EncryptedJsonStore::in_config_dir(&ctx.data_dir, OTP_SECRET_FILE)?;
                    // Generate a random 20-byte secret for TOTP
                    let secret = generate_random_secret();
                    otp_store.save(&OtpSecret {
                        secret: secret.clone(),
                    })?;
                    let current_code = otp::generate_totp(&secret, 30, 6)?;
                    println!("OTP secret provisioned (encrypted). Current code: {current_code}");
                }

                store.save(&state)?;
                println!("Emergency stop engaged (level={})", state.level);
            }
        }

        Ok(())
    }
}

fn level_to_str(level: EstopLevel) -> &'static str {
    match level {
        EstopLevel::KillAll => "kill-all",
        EstopLevel::NetworkKill => "network-kill",
        EstopLevel::DomainBlock => "domain-block",
        EstopLevel::ToolFreeze => "tool-freeze",
    }
}

fn disengage_all(state: &mut EstopState) {
    state.engaged = false;
    state.domains.clear();
    state.tools.clear();
}

fn apply_scoped_resume(
    state: &mut EstopState,
    network: bool,
    domains: &[String],
    tools: &[String],
) {
    match state.level.as_str() {
        "network-kill" => {
            if network {
                disengage_all(state);
            }
        }
        "domain-block" => {
            if !domains.is_empty() {
                state.domains.retain(|domain| {
                    !domains
                        .iter()
                        .any(|needle| needle.eq_ignore_ascii_case(domain))
                });
            }
            if state.domains.is_empty() {
                state.engaged = false;
            }
        }
        "tool-freeze" => {
            if !tools.is_empty() {
                state
                    .tools
                    .retain(|tool| !tools.iter().any(|needle| needle.eq_ignore_ascii_case(tool)));
            }
            if state.tools.is_empty() {
                state.engaged = false;
            }
        }
        _ => {
            if network || !domains.is_empty() || !tools.is_empty() {
                // kill-all and unknown levels resume fully when any scope flag is provided
                disengage_all(state);
            }
        }
    }
}

fn print_status(state: &EstopState) {
    println!(
        "Emergency stop: {}",
        if state.engaged { "ENGAGED" } else { "inactive" }
    );
    println!("  level: {}", state.level);
    if state.require_otp_to_resume {
        println!("  require_otp_to_resume: true");
    }
    if !state.domains.is_empty() {
        println!("  domains: {}", state.domains.join(", "));
    }
    if !state.tools.is_empty() {
        println!("  tools: {}", state.tools.join(", "));
    }
    if let Some(engaged_at) = state.engaged_at_epoch_secs {
        println!("  engaged_at_epoch_secs: {engaged_at}");
    }
    if let Some(resumed_at) = state.resumed_at_epoch_secs {
        println!("  resumed_at_epoch_secs: {resumed_at}");
    }
}

fn generate_random_secret() -> Vec<u8> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Generate a 20-byte secret using system entropy sources
    let mut bytes = Vec::with_capacity(20);
    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);

    // Fill 20 bytes using successive hashes
    while bytes.len() < 20 {
        let h = hasher.finish();
        for b in h.to_le_bytes() {
            if bytes.len() < 20 {
                bytes.push(b);
            }
        }
        hasher.write_u64(h);
    }
    bytes
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{EstopCommand, EstopOptions, OtpSecret, OTP_SECRET_FILE};
    use crate::cli::{EstopCommands, EstopLevel};
    use crate::command_core::{AgentZeroCommand, CommandContext};
    use agentzero_core::security::otp;
    use agentzero_storage::EncryptedJsonStore;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-estop-cmd-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn estop_engage_then_resume_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        EstopCommand::run(
            &ctx,
            EstopOptions {
                level: Some(EstopLevel::KillAll),
                domains: Vec::new(),
                tools: Vec::new(),
                require_otp: false,
                command: None,
            },
        )
        .await
        .expect("engage should succeed");

        EstopCommand::run(
            &ctx,
            EstopOptions {
                level: None,
                domains: Vec::new(),
                tools: Vec::new(),
                require_otp: false,
                command: Some(EstopCommands::Status),
            },
        )
        .await
        .expect("status should succeed");

        EstopCommand::run(
            &ctx,
            EstopOptions {
                level: None,
                domains: Vec::new(),
                tools: Vec::new(),
                require_otp: false,
                command: Some(EstopCommands::Resume {
                    network: false,
                    domains: Vec::new(),
                    tools: Vec::new(),
                    otp: None,
                }),
            },
        )
        .await
        .expect("resume should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn estop_resume_without_engage_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = EstopCommand::run(
            &ctx,
            EstopOptions {
                level: None,
                domains: Vec::new(),
                tools: Vec::new(),
                require_otp: false,
                command: Some(EstopCommands::Resume {
                    network: true,
                    domains: Vec::new(),
                    tools: Vec::new(),
                    otp: None,
                }),
            },
        )
        .await
        .expect_err("resume without engage should fail");

        assert!(err.to_string().contains("not engaged"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn estop_otp_engage_then_resume_with_valid_code_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        // Engage with OTP required
        EstopCommand::run(
            &ctx,
            EstopOptions {
                level: Some(EstopLevel::KillAll),
                domains: Vec::new(),
                tools: Vec::new(),
                require_otp: true,
                command: None,
            },
        )
        .await
        .expect("engage with OTP should succeed");

        // Load the provisioned secret to generate a valid code
        let otp_store =
            EncryptedJsonStore::in_config_dir(&dir, OTP_SECRET_FILE).expect("store should open");
        let secret: OtpSecret = otp_store
            .load_optional()
            .expect("load should succeed")
            .expect("OTP secret should exist");
        let valid_code = otp::generate_totp(&secret.secret, 30, 6).expect("totp should generate");

        // Resume with correct OTP
        EstopCommand::run(
            &ctx,
            EstopOptions {
                level: None,
                domains: Vec::new(),
                tools: Vec::new(),
                require_otp: false,
                command: Some(EstopCommands::Resume {
                    network: false,
                    domains: Vec::new(),
                    tools: Vec::new(),
                    otp: Some(valid_code),
                }),
            },
        )
        .await
        .expect("resume with valid OTP should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn estop_otp_resume_without_code_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        // Engage with OTP required
        EstopCommand::run(
            &ctx,
            EstopOptions {
                level: Some(EstopLevel::KillAll),
                domains: Vec::new(),
                tools: Vec::new(),
                require_otp: true,
                command: None,
            },
        )
        .await
        .expect("engage should succeed");

        // Try resume without OTP code
        let err = EstopCommand::run(
            &ctx,
            EstopOptions {
                level: None,
                domains: Vec::new(),
                tools: Vec::new(),
                require_otp: false,
                command: Some(EstopCommands::Resume {
                    network: false,
                    domains: Vec::new(),
                    tools: Vec::new(),
                    otp: None,
                }),
            },
        )
        .await
        .expect_err("resume without OTP should fail");

        assert!(err.to_string().contains("OTP code required"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn estop_otp_resume_with_wrong_code_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        // Engage with OTP required
        EstopCommand::run(
            &ctx,
            EstopOptions {
                level: Some(EstopLevel::KillAll),
                domains: Vec::new(),
                tools: Vec::new(),
                require_otp: true,
                command: None,
            },
        )
        .await
        .expect("engage should succeed");

        // Try resume with wrong OTP code
        let err = EstopCommand::run(
            &ctx,
            EstopOptions {
                level: None,
                domains: Vec::new(),
                tools: Vec::new(),
                require_otp: false,
                command: Some(EstopCommands::Resume {
                    network: false,
                    domains: Vec::new(),
                    tools: Vec::new(),
                    otp: Some("000000".to_string()),
                }),
            },
        )
        .await
        .expect_err("resume with wrong OTP should fail");

        assert!(err.to_string().contains("invalid OTP code"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
