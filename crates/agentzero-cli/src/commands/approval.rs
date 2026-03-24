use crate::approval::{
    ApprovalDecision, ApprovalEngine, ApprovalError, ApprovalRequest, AuditEntry, RiskLevel,
};
use crate::cli::{ApprovalCommands, ApprovalDecisionMode, ApprovalRisk};
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_storage::EncryptedJsonStore;
use async_trait::async_trait;
use serde::Serialize;

const APPROVAL_AUDIT_FILE: &str = "approval-audit.json";

#[derive(Debug, Serialize)]
struct ApprovalOutput {
    actor: String,
    action: String,
    risk: String,
    outcome: Option<String>,
    error: Option<String>,
}

pub struct ApprovalCommand;

#[async_trait]
impl AgentZeroCommand for ApprovalCommand {
    type Options = ApprovalCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let store = EncryptedJsonStore::in_config_dir(&ctx.data_dir, APPROVAL_AUDIT_FILE)?;
        let mut existing_log = store.load_or_default::<Vec<AuditEntry>>()?;

        match opts {
            ApprovalCommands::Evaluate {
                actor,
                action,
                risk,
                approver,
                decision,
                reason,
                json,
            } => {
                let request = ApprovalRequest::new(&actor, &action, map_risk(risk))
                    .map_err(|err| anyhow::anyhow!(err.to_string()))?;
                let decision = build_decision(approver.as_deref(), decision, reason.as_deref())?;

                let mut engine = ApprovalEngine::new();
                let evaluation = engine.evaluate(request.clone(), decision);
                existing_log.extend_from_slice(engine.audit_log());
                store.save(&existing_log)?;

                match evaluation {
                    Ok(outcome) => {
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&ApprovalOutput {
                                    actor,
                                    action,
                                    risk: format!("{:?}", risk).to_ascii_lowercase(),
                                    outcome: Some(format!("{outcome:?}").to_ascii_lowercase()),
                                    error: None,
                                })?
                            );
                        } else {
                            println!(
                                "Approval outcome: {}",
                                format!("{outcome:?}").to_ascii_lowercase()
                            );
                        }
                    }
                    Err(err) => {
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&ApprovalOutput {
                                    actor,
                                    action,
                                    risk: format!("{:?}", risk).to_ascii_lowercase(),
                                    outcome: None,
                                    error: Some(err.to_string()),
                                })?
                            );
                        } else {
                            println!("Approval blocked: {err}");
                        }
                        return Err(anyhow::anyhow!(err.to_string()));
                    }
                }
            }
        }

        Ok(())
    }
}

fn map_risk(risk: ApprovalRisk) -> RiskLevel {
    match risk {
        ApprovalRisk::Low => RiskLevel::Low,
        ApprovalRisk::Medium => RiskLevel::Medium,
        ApprovalRisk::High => RiskLevel::High,
        ApprovalRisk::Critical => RiskLevel::Critical,
    }
}

fn build_decision(
    approver: Option<&str>,
    mode: Option<ApprovalDecisionMode>,
    reason: Option<&str>,
) -> anyhow::Result<Option<ApprovalDecision>> {
    let decision = match mode {
        Some(ApprovalDecisionMode::Allow) => {
            let approver = approver
                .ok_or_else(|| anyhow::anyhow!("--approver is required when --decision is set"))?;
            Some(ApprovalDecision::allow(approver, reason).map_err(map_approval_err)?)
        }
        Some(ApprovalDecisionMode::Deny) => {
            let approver = approver
                .ok_or_else(|| anyhow::anyhow!("--approver is required when --decision is set"))?;
            Some(ApprovalDecision::deny(approver, reason).map_err(map_approval_err)?)
        }
        None => None,
    };

    Ok(decision)
}

fn map_approval_err(err: ApprovalError) -> anyhow::Error {
    anyhow::anyhow!(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::ApprovalCommand;
    use crate::cli::{ApprovalCommands, ApprovalDecisionMode, ApprovalRisk};
    use crate::command_core::{AgentZeroCommand, CommandContext};
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
            "agentzero-approval-cmd-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn approval_evaluate_low_risk_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        ApprovalCommand::run(
            &ctx,
            ApprovalCommands::Evaluate {
                actor: "operator-1".to_string(),
                action: "read_status".to_string(),
                risk: ApprovalRisk::Low,
                approver: None,
                decision: None,
                reason: None,
                json: true,
            },
        )
        .await
        .expect("low-risk evaluate should succeed");

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn approval_evaluate_high_risk_without_decision_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = ApprovalCommand::run(
            &ctx,
            ApprovalCommands::Evaluate {
                actor: "operator-1".to_string(),
                action: "wipe_data".to_string(),
                risk: ApprovalRisk::High,
                approver: None,
                decision: None,
                reason: None,
                json: false,
            },
        )
        .await
        .expect_err("high-risk evaluate without decision should fail");
        assert!(err.to_string().contains("approval required"));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn approval_evaluate_with_allow_decision_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        ApprovalCommand::run(
            &ctx,
            ApprovalCommands::Evaluate {
                actor: "operator-1".to_string(),
                action: "deploy_service".to_string(),
                risk: ApprovalRisk::High,
                approver: Some("admin".to_string()),
                decision: Some(ApprovalDecisionMode::Allow),
                reason: Some("deployment approved".to_string()),
                json: true,
            },
        )
        .await
        .expect("high-risk evaluate with allow decision should succeed");

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn approval_evaluate_with_deny_decision_returns_error_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = ApprovalCommand::run(
            &ctx,
            ApprovalCommands::Evaluate {
                actor: "operator-1".to_string(),
                action: "deploy_service".to_string(),
                risk: ApprovalRisk::High,
                approver: Some("admin".to_string()),
                decision: Some(ApprovalDecisionMode::Deny),
                reason: Some("not ready".to_string()),
                json: false,
            },
        )
        .await
        .expect_err("deny decision should return error");
        assert!(err.to_string().contains("denied"));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn approval_evaluate_decision_without_approver_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = ApprovalCommand::run(
            &ctx,
            ApprovalCommands::Evaluate {
                actor: "operator-1".to_string(),
                action: "deploy_service".to_string(),
                risk: ApprovalRisk::Low,
                approver: None,
                decision: Some(ApprovalDecisionMode::Allow),
                reason: None,
                json: false,
            },
        )
        .await
        .expect_err("decision without approver should fail");
        assert!(err.to_string().contains("--approver is required"));

        let _ = fs::remove_dir_all(dir);
    }
}
