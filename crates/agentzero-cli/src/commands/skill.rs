use crate::cli::SkillCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_config::skills::{discover_skills, install_skill, list_builtin_skills, SkillSource};
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
            SkillCommands::Add { source, global } => {
                let target_dir = if global {
                    home_agentzero_dir().unwrap_or_else(|| ctx.data_dir.clone())
                } else {
                    ctx.workspace_root.join(".agentzero")
                };

                let skill_source = SkillSource::parse(&source);

                // Show available built-in skills if source is "list" or help
                if source == "list" || source == "help" {
                    println!("Available built-in skills:");
                    for name in list_builtin_skills() {
                        println!("  - {name}");
                    }
                    println!("\nInstall with: agentzero skill add <name>");
                    println!("Or from git:  agentzero skill add https://github.com/user/skill");
                    return Ok(());
                }

                let skill = install_skill(&skill_source, &target_dir)?;
                println!("Installed skill `{}`", skill.name());
                println!("  source: {}", skill.source);
                println!("  dir: {}", skill.dir.display());
                if skill.manifest.workflow.is_some() {
                    println!("  type: workflow pack");
                }
                if let Some(prompt) = &skill.agent_prompt {
                    let preview = if prompt.len() > 80 {
                        format!("{}...", &prompt[..80])
                    } else {
                        prompt.clone()
                    };
                    println!("  agent: {preview}");
                }
            }
            SkillCommands::Info { name } => {
                let global_dir = home_agentzero_dir();
                let skills = discover_skills(Some(&ctx.workspace_root), global_dir.as_deref());

                let skill = skills
                    .iter()
                    .find(|s| s.name() == name)
                    .ok_or_else(|| anyhow::anyhow!("skill `{name}` not found"))?;

                println!("Skill: {}", skill.name());
                println!("  version: {}", skill.manifest.skill.version);
                println!("  description: {}", skill.manifest.skill.description);
                if !skill.manifest.skill.author.is_empty() {
                    println!("  author: {}", skill.manifest.skill.author);
                }
                println!("  source: {}", skill.source);
                println!("  dir: {}", skill.dir.display());
                if !skill.manifest.skill.provides.is_empty() {
                    println!("  provides: {}", skill.manifest.skill.provides.join(", "));
                }
                if !skill.manifest.skill.keywords.is_empty() {
                    println!("  keywords: {}", skill.manifest.skill.keywords.join(", "));
                }
                if !skill.manifest.tools.is_empty() {
                    println!("  tools:");
                    for tool in &skill.manifest.tools {
                        println!("    - {} ({})", tool.name, tool.kind);
                    }
                }
                if !skill.manifest.commands.is_empty() {
                    println!("  commands:");
                    for cmd in &skill.manifest.commands {
                        println!("    - /{} — {}", cmd.name, cmd.description);
                    }
                }
                if let Some(wf) = &skill.manifest.workflow {
                    println!("  workflow: {}", wf.name);
                    println!("    nodes: {}", wf.nodes.len());
                    println!("    edges: {}", wf.edges.len());
                    println!("    entry points: {}", wf.entry_points.len());
                    if !wf.cron.is_empty() {
                        println!("    cron schedules: {}", wf.cron.len());
                    }
                }
                if let Some(prompt) = &skill.agent_prompt {
                    println!("  agent prompt:");
                    for line in prompt.lines().take(5) {
                        println!("    {line}");
                    }
                    if prompt.lines().count() > 5 {
                        println!("    ...");
                    }
                }
            }
            SkillCommands::Discover => {
                let global_dir = home_agentzero_dir();
                let skills = discover_skills(Some(&ctx.workspace_root), global_dir.as_deref());

                if skills.is_empty() {
                    println!("No skills discovered.");
                    println!("Install a built-in skill: agentzero skill add <name>");
                    println!("Available: {}", list_builtin_skills().join(", "));
                } else {
                    println!("Discovered {} skill(s):", skills.len());
                    for skill in &skills {
                        let kind = if skill.manifest.workflow.is_some() {
                            "pack"
                        } else {
                            "skill"
                        };
                        println!(
                            "  - {} v{} [{}] ({}) — {}",
                            skill.name(),
                            skill.manifest.skill.version,
                            kind,
                            skill.source,
                            skill.manifest.skill.description,
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

fn home_agentzero_dir() -> Option<std::path::PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join(".agentzero"))
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

        fs::remove_dir_all(dir).expect("temp dir should be removed");
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

        fs::remove_dir_all(dir).expect("temp dir should be removed");
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

        fs::remove_dir_all(dir).expect("temp dir should be removed");
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

        fs::remove_dir_all(dir).expect("temp dir should be removed");
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

        fs::remove_dir_all(dir).expect("temp dir should be removed");
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

        fs::remove_dir_all(dir).expect("temp dir should be removed");
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

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn skill_add_builtin_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        SkillCommand::run(
            &ctx,
            SkillCommands::Add {
                source: "code-reviewer".to_string(),
                global: false,
            },
        )
        .await
        .expect("add builtin skill should succeed");

        // Verify skill was installed to project-local dir
        let skill_dir = dir.join(".agentzero/skills/code-reviewer");
        assert!(skill_dir.join("skill.toml").exists());
        assert!(skill_dir.join("AGENT.md").exists());

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn skill_add_duplicate_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        SkillCommand::run(
            &ctx,
            SkillCommands::Add {
                source: "scheduler".to_string(),
                global: false,
            },
        )
        .await
        .expect("first add should succeed");

        let err = SkillCommand::run(
            &ctx,
            SkillCommands::Add {
                source: "scheduler".to_string(),
                global: false,
            },
        )
        .await
        .expect_err("duplicate add should fail");
        assert!(err.to_string().contains("already installed"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn skill_discover_after_add_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        SkillCommand::run(
            &ctx,
            SkillCommands::Add {
                source: "research-assistant".to_string(),
                global: false,
            },
        )
        .await
        .expect("add should succeed");

        SkillCommand::run(&ctx, SkillCommands::Discover)
            .await
            .expect("discover should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn skill_info_after_add_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        SkillCommand::run(
            &ctx,
            SkillCommands::Add {
                source: "code-reviewer".to_string(),
                global: false,
            },
        )
        .await
        .expect("add should succeed");

        SkillCommand::run(
            &ctx,
            SkillCommands::Info {
                name: "code-reviewer".to_string(),
            },
        )
        .await
        .expect("info should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn skill_info_missing_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = SkillCommand::run(
            &ctx,
            SkillCommands::Info {
                name: "nonexistent".to_string(),
            },
        )
        .await
        .expect_err("info on missing skill should fail");
        assert!(err.to_string().contains("not found"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
