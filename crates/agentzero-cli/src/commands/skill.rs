use crate::cli::SkillCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_skills::SkillStore;
use async_trait::async_trait;

pub struct SkillCommand;

#[async_trait]
impl AgentZeroCommand for SkillCommand {
    type Options = SkillCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let store = SkillStore::new(&ctx.data_dir)?;

        match opts {
            SkillCommands::List { json } => {
                let skills = store.list()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&skills)?);
                } else {
                    println!("Installed skills ({})", skills.len());
                    for skill in skills {
                        println!(
                            "- {} [{}] source={} ",
                            skill.name,
                            if skill.enabled { "enabled" } else { "disabled" },
                            skill.source
                        );
                    }
                }
            }
            SkillCommands::Install { name, source } => {
                let skill = store.install(&name, &source)?;
                println!("Installed skill `{}` from `{}`", skill.name, skill.source);
            }
            SkillCommands::Test { name } => {
                let output = store.test(&name)?;
                println!("{output}");
            }
            SkillCommands::Remove { name } => {
                store.remove(&name)?;
                println!("Removed skill `{name}`");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SkillCommand;
    use crate::cli::SkillCommands;
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
        let dir = std::env::temp_dir().join(format!("agentzero-skill-cmd-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn skill_install_then_test_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        SkillCommand::run(
            &ctx,
            SkillCommands::Install {
                name: "my_skill".to_string(),
                source: "local".to_string(),
            },
        )
        .await
        .expect("install should succeed");

        SkillCommand::run(
            &ctx,
            SkillCommands::Test {
                name: "my_skill".to_string(),
            },
        )
        .await
        .expect("test should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn skill_remove_missing_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = SkillCommand::run(
            &ctx,
            SkillCommands::Remove {
                name: "missing".to_string(),
            },
        )
        .await
        .expect_err("remove missing should fail");
        assert!(err.to_string().contains("not installed"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
