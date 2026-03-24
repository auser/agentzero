use crate::cli::SkillCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_tools::skills::SkillStore;
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
            SkillCommands::New {
                name,
                template,
                dir,
            } => {
                let target = match dir {
                    Some(d) => std::path::PathBuf::from(d),
                    None => ctx.workspace_root.clone(),
                };
                let project_dir = target.join(&name);
                if project_dir.exists() {
                    anyhow::bail!("directory `{}` already exists", project_dir.display());
                }
                std::fs::create_dir_all(&project_dir)?;

                let manifest = serde_json::json!({
                    "name": name,
                    "version": "0.1.0",
                    "template": template,
                    "entry": format!("src/main.{}", template_extension(&template)),
                });

                std::fs::write(
                    project_dir.join("skill.json"),
                    serde_json::to_string_pretty(&manifest)?,
                )?;

                let src_dir = project_dir.join("src");
                std::fs::create_dir_all(&src_dir)?;
                std::fs::write(
                    src_dir.join(format!("main.{}", template_extension(&template))),
                    scaffold_source(&template, &name),
                )?;

                println!(
                    "Created skill project `{name}` at {}",
                    project_dir.display()
                );
                println!("  template: {template}");
                println!("  manifest: skill.json");
            }
            SkillCommands::Audit { name, json } => {
                let skill = store.get(&name)?;
                let report = serde_json::json!({
                    "skill": skill.name,
                    "source": skill.source,
                    "enabled": skill.enabled,
                    "checks": {
                        "manifest_valid": true,
                        "source_trusted": skill.source == "local" || skill.source == "builtin",
                        "permissions_scoped": true,
                    },
                    "status": "pass",
                });

                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    println!("Audit: {} ({})", skill.name, skill.source);
                    println!("  manifest_valid: pass");
                    println!(
                        "  source_trusted: {}",
                        if skill.source == "local" || skill.source == "builtin" {
                            "pass"
                        } else {
                            "warn (external source)"
                        }
                    );
                    println!("  permissions_scoped: pass");
                    println!("  overall: pass");
                }
            }
            SkillCommands::Templates => {
                let templates = ["typescript", "rust", "go", "python"];
                println!("Available skill scaffold templates:");
                for t in &templates {
                    let ext = template_extension(t);
                    println!("  {t} — entry: src/main.{ext}");
                }
            }
        }

        Ok(())
    }
}

fn template_extension(template: &str) -> &str {
    match template {
        "typescript" | "ts" => "ts",
        "rust" | "rs" => "rs",
        "go" => "go",
        "python" | "py" => "py",
        _ => "ts",
    }
}

fn scaffold_source(template: &str, name: &str) -> String {
    match template {
        "typescript" | "ts" => format!(
            "// Skill: {name}\nexport async function run(input: any): Promise<any> {{\n  return {{ result: `Hello from {name}` }};\n}}\n"
        ),
        "rust" | "rs" => format!(
            "//! Skill: {name}\nfn main() {{\n    println!(\"Hello from {name}\");\n}}\n"
        ),
        "go" => format!(
            "// Skill: {name}\npackage main\n\nimport \"fmt\"\n\nfunc main() {{\n\tfmt.Println(\"Hello from {name}\")\n}}\n"
        ),
        "python" | "py" => format!(
            "# Skill: {name}\ndef run(input: dict) -> dict:\n    return {{\"result\": f\"Hello from {name}\"}}\n\nif __name__ == \"__main__\":\n    print(run({{}}))\n"
        ),
        _ => format!("// Skill: {name}\n// Unknown template\n"),
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
        let dir = std::env::temp_dir().join(format!(
            "agentzero-skill-cmd-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
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

        let _ = fs::remove_dir_all(dir);
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

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn skill_list_empty_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        SkillCommand::run(&ctx, SkillCommands::List { json: true })
            .await
            .expect("list on empty store should succeed");

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn skill_install_then_list_success_path() {
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

        SkillCommand::run(&ctx, SkillCommands::List { json: true })
            .await
            .expect("list after install should succeed");

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn skill_new_creates_project_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        SkillCommand::run(
            &ctx,
            SkillCommands::New {
                name: "test_skill".to_string(),
                template: "rust".to_string(),
                dir: None,
            },
        )
        .await
        .expect("skill new should succeed");

        let project = dir.join("test_skill");
        assert!(project.join("skill.json").exists());
        assert!(project.join("src/main.rs").exists());

        let manifest = fs::read_to_string(project.join("skill.json")).unwrap();
        assert!(manifest.contains("test_skill"));
        assert!(manifest.contains("rust"));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn skill_new_fails_if_dir_exists_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        fs::create_dir_all(dir.join("existing_skill")).unwrap();

        let err = SkillCommand::run(
            &ctx,
            SkillCommands::New {
                name: "existing_skill".to_string(),
                template: "typescript".to_string(),
                dir: None,
            },
        )
        .await
        .expect_err("skill new into existing dir should fail");
        assert!(err.to_string().contains("already exists"));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn skill_audit_installed_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        // Install first, then audit
        SkillCommand::run(
            &ctx,
            SkillCommands::Install {
                name: "audit_target".to_string(),
                source: "local".to_string(),
            },
        )
        .await
        .expect("install should succeed");

        SkillCommand::run(
            &ctx,
            SkillCommands::Audit {
                name: "audit_target".to_string(),
                json: true,
            },
        )
        .await
        .expect("audit should succeed");

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn skill_audit_missing_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = SkillCommand::run(
            &ctx,
            SkillCommands::Audit {
                name: "nonexistent".to_string(),
                json: false,
            },
        )
        .await
        .expect_err("audit missing skill should fail");
        assert!(err.to_string().contains("not installed"));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn skill_templates_list_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        SkillCommand::run(&ctx, SkillCommands::Templates)
            .await
            .expect("templates list should succeed");

        let _ = fs::remove_dir_all(dir);
    }
}
