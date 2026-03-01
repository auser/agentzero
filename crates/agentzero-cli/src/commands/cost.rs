use crate::cli::CostCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_cost::CostSummary;
use agentzero_storage::EncryptedJsonStore;
use async_trait::async_trait;

const COST_SUMMARY_FILE: &str = "cost-summary.json";

pub struct CostCommand;

#[async_trait]
impl AgentZeroCommand for CostCommand {
    type Options = CostCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let store = EncryptedJsonStore::in_config_dir(&ctx.data_dir, COST_SUMMARY_FILE)?;
        let mut summary = store.load_or_default::<CostSummary>()?;

        match opts {
            CostCommands::Status { json } => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&summary)?);
                } else {
                    println!(
                        "Cost: total_tokens={} total_usd={:.6}",
                        summary.total_tokens, summary.total_usd
                    );
                }
            }
            CostCommands::Record { tokens, usd } => {
                summary.record(tokens, usd);
                store.save(&summary)?;
                println!(
                    "Recorded cost: tokens+={} usd+={:.6} (total_tokens={} total_usd={:.6})",
                    tokens, usd, summary.total_tokens, summary.total_usd
                );
            }
            CostCommands::Reset => {
                summary = CostSummary::default();
                store.save(&summary)?;
                println!("Cost summary reset");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::CostCommand;
    use crate::cli::CostCommands;
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
        let dir = std::env::temp_dir().join(format!("agentzero-cost-cmd-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn cost_record_status_reset_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        CostCommand::run(
            &ctx,
            CostCommands::Record {
                tokens: 200,
                usd: 0.04,
            },
        )
        .await
        .expect("record should succeed");

        CostCommand::run(&ctx, CostCommands::Status { json: true })
            .await
            .expect("status should succeed");

        CostCommand::run(&ctx, CostCommands::Reset)
            .await
            .expect("reset should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
