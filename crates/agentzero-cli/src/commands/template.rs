use crate::cli::TemplateCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_config::{
    discover_templates, list_template_sources, TemplateFile, TEMPLATE_LOAD_ORDER,
};
use async_trait::async_trait;

pub struct TemplateCommand;

#[async_trait]
impl AgentZeroCommand for TemplateCommand {
    type Options = TemplateCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            TemplateCommands::List { json } => {
                let sources = list_template_sources(&ctx.workspace_root, Some(&ctx.data_dir));

                if json {
                    let entries: Vec<serde_json::Value> = sources
                        .iter()
                        .map(|(t, path)| {
                            serde_json::json!({
                                "name": t.file_name(),
                                "session": if t.is_main_session_only() { "main" } else { "shared" },
                                "found": path.is_some(),
                                "source": path.as_ref().map(|p| p.display().to_string()),
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&entries)?);
                } else {
                    println!("Templates ({} defined):", TEMPLATE_LOAD_ORDER.len());
                    for (template, path) in &sources {
                        let status = match path {
                            Some(p) => format!("found at {}", p.display()),
                            None => "not found".to_string(),
                        };
                        let session = if template.is_main_session_only() {
                            "main-only"
                        } else {
                            "shared"
                        };
                        println!("  {} [{}] — {}", template.file_name(), session, status);
                    }

                    let set = discover_templates(&ctx.workspace_root, Some(&ctx.data_dir));
                    if let Some(guidance) = set.missing_guidance() {
                        println!("\n{guidance}");
                    }
                }
            }
            TemplateCommands::Show { name } => {
                let template = parse_template_name(&name)?;
                let set = discover_templates(&ctx.workspace_root, Some(&ctx.data_dir));

                match set.get(template) {
                    Some(resolved) => {
                        println!(
                            "# {} (from {})\n",
                            template.file_name(),
                            resolved.source.display()
                        );
                        println!("{}", resolved.content);
                    }
                    None => {
                        anyhow::bail!(
                            "template `{}` not found. Run `agentzero template init --name {}` to create it.",
                            template.file_name(),
                            name.to_uppercase()
                        );
                    }
                }
            }
            TemplateCommands::Init { name, dir, force } => {
                let target_dir = match dir {
                    Some(d) => std::path::PathBuf::from(d),
                    None => ctx.workspace_root.clone(),
                };

                if !target_dir.exists() {
                    std::fs::create_dir_all(&target_dir)?;
                }

                let templates = match name {
                    Some(n) => vec![parse_template_name(&n)?],
                    None => TEMPLATE_LOAD_ORDER.to_vec(),
                };

                for template in &templates {
                    let path = target_dir.join(template.file_name());
                    if path.exists() && !force {
                        println!(
                            "  skip {} (already exists, use --force to overwrite)",
                            template.file_name()
                        );
                        continue;
                    }
                    let content = default_template_content(*template);
                    std::fs::write(&path, content)?;
                    println!("  created {}", path.display());
                }
            }
            TemplateCommands::Validate => {
                let set = discover_templates(&ctx.workspace_root, Some(&ctx.data_dir));
                let mut errors = 0;

                for resolved in &set.templates {
                    if resolved.content.trim().is_empty() {
                        println!(
                            "  warn: {} is empty ({})",
                            resolved.template.file_name(),
                            resolved.source.display()
                        );
                        errors += 1;
                    } else {
                        println!(
                            "  ok: {} ({} bytes, from {})",
                            resolved.template.file_name(),
                            resolved.content.len(),
                            resolved.source.display()
                        );
                    }
                }

                for missing in &set.missing {
                    println!("  missing: {}", missing.file_name());
                }

                if errors > 0 {
                    anyhow::bail!("{errors} template(s) have warnings");
                }

                println!(
                    "\n{} template(s) validated, {} missing (optional).",
                    set.templates.len(),
                    set.missing.len()
                );
            }
        }
        Ok(())
    }
}

fn parse_template_name(name: &str) -> anyhow::Result<TemplateFile> {
    let normalized = name.to_uppercase();
    let stem = normalized.trim_end_matches(".MD").trim_end_matches(".md");

    for &template in TEMPLATE_LOAD_ORDER {
        let template_stem = template.file_name().trim_end_matches(".md");
        if stem == template_stem {
            return Ok(template);
        }
    }

    let valid: Vec<&str> = TEMPLATE_LOAD_ORDER
        .iter()
        .map(|t| t.file_name().trim_end_matches(".md"))
        .collect();
    anyhow::bail!(
        "unknown template `{name}`. Valid names: {}",
        valid.join(", ")
    )
}

fn default_template_content(template: TemplateFile) -> &'static str {
    match template {
        TemplateFile::Agents => {
            "# Agents\n\nDefine agent behavior rules and coordination patterns.\n"
        }
        TemplateFile::Boot => "# Boot\n\nInitialization instructions executed at agent startup.\n",
        TemplateFile::Bootstrap => {
            "# Bootstrap\n\nFirst-run setup instructions for new workspaces.\n"
        }
        TemplateFile::Heartbeat => {
            "# Heartbeat\n\nHealthcheck and lifecycle management configuration.\n"
        }
        TemplateFile::Identity => {
            "# Identity\n\nDefine who the agent is: name, role, capabilities.\n"
        }
        TemplateFile::Soul => "# Soul\n\nAgent personality, communication style, and values.\n",
        TemplateFile::Tools => {
            "# Tools\n\nGuidance for tool usage, preferences, and restrictions.\n"
        }
        TemplateFile::User => {
            "# User\n\nUser context: preferences, project details, conventions.\n"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_core::CommandContext;
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
        let dir = std::env::temp_dir().join(format!("agentzero-template-cmd-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn test_ctx(dir: &std::path::Path) -> CommandContext {
        CommandContext {
            workspace_root: dir.to_path_buf(),
            data_dir: dir.to_path_buf(),
            config_path: dir.join("agentzero.toml"),
        }
    }

    #[test]
    fn parse_template_name_accepts_uppercase() {
        assert_eq!(parse_template_name("AGENTS").unwrap(), TemplateFile::Agents);
        assert_eq!(parse_template_name("BOOT").unwrap(), TemplateFile::Boot);
        assert_eq!(
            parse_template_name("IDENTITY").unwrap(),
            TemplateFile::Identity
        );
    }

    #[test]
    fn parse_template_name_accepts_lowercase() {
        assert_eq!(parse_template_name("agents").unwrap(), TemplateFile::Agents);
        assert_eq!(parse_template_name("soul").unwrap(), TemplateFile::Soul);
    }

    #[test]
    fn parse_template_name_accepts_with_extension() {
        assert_eq!(
            parse_template_name("AGENTS.md").unwrap(),
            TemplateFile::Agents
        );
        assert_eq!(parse_template_name("BOOT.MD").unwrap(), TemplateFile::Boot);
    }

    #[test]
    fn parse_template_name_rejects_unknown() {
        let err = parse_template_name("UNKNOWN").unwrap_err();
        assert!(err.to_string().contains("unknown template"));
    }

    #[tokio::test]
    async fn template_list_empty_workspace_success_path() {
        let dir = temp_dir();
        let ctx = test_ctx(&dir);

        TemplateCommand::run(&ctx, TemplateCommands::List { json: false })
            .await
            .expect("list should succeed on empty workspace");

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn template_list_json_success_path() {
        let dir = temp_dir();
        let ctx = test_ctx(&dir);
        fs::write(dir.join("AGENTS.md"), "# Agents").unwrap();

        TemplateCommand::run(&ctx, TemplateCommands::List { json: true })
            .await
            .expect("json list should succeed");

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn template_show_existing_success_path() {
        let dir = temp_dir();
        let ctx = test_ctx(&dir);
        fs::write(dir.join("IDENTITY.md"), "# My Identity").unwrap();

        TemplateCommand::run(
            &ctx,
            TemplateCommands::Show {
                name: "IDENTITY".to_string(),
            },
        )
        .await
        .expect("show should succeed for existing template");

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn template_show_missing_negative_path() {
        let dir = temp_dir();
        let ctx = test_ctx(&dir);

        let err = TemplateCommand::run(
            &ctx,
            TemplateCommands::Show {
                name: "AGENTS".to_string(),
            },
        )
        .await
        .expect_err("show missing template should fail");
        assert!(err.to_string().contains("not found"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn template_init_all_success_path() {
        let dir = temp_dir();
        let ctx = test_ctx(&dir);

        TemplateCommand::run(
            &ctx,
            TemplateCommands::Init {
                name: None,
                dir: None,
                force: false,
            },
        )
        .await
        .expect("init all should succeed");

        // Verify all templates were created
        for template in TEMPLATE_LOAD_ORDER {
            assert!(
                dir.join(template.file_name()).exists(),
                "{} should exist",
                template.file_name()
            );
        }

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn template_init_single_success_path() {
        let dir = temp_dir();
        let ctx = test_ctx(&dir);

        TemplateCommand::run(
            &ctx,
            TemplateCommands::Init {
                name: Some("SOUL".to_string()),
                dir: None,
                force: false,
            },
        )
        .await
        .expect("init single should succeed");

        assert!(dir.join("SOUL.md").exists());
        assert!(!dir.join("AGENTS.md").exists()); // Only SOUL should be created

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn template_init_skips_existing_without_force() {
        let dir = temp_dir();
        let ctx = test_ctx(&dir);
        fs::write(dir.join("AGENTS.md"), "custom content").unwrap();

        TemplateCommand::run(
            &ctx,
            TemplateCommands::Init {
                name: Some("AGENTS".to_string()),
                dir: None,
                force: false,
            },
        )
        .await
        .expect("init should succeed");

        // Content should be preserved
        let content = fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        assert_eq!(content, "custom content");

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn template_init_overwrites_with_force() {
        let dir = temp_dir();
        let ctx = test_ctx(&dir);
        fs::write(dir.join("AGENTS.md"), "custom content").unwrap();

        TemplateCommand::run(
            &ctx,
            TemplateCommands::Init {
                name: Some("AGENTS".to_string()),
                dir: None,
                force: true,
            },
        )
        .await
        .expect("init with force should succeed");

        let content = fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        assert!(content.starts_with("# Agents"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn template_validate_success_path() {
        let dir = temp_dir();
        let ctx = test_ctx(&dir);
        fs::write(dir.join("AGENTS.md"), "# Agents content").unwrap();
        fs::write(dir.join("IDENTITY.md"), "# Identity content").unwrap();

        TemplateCommand::run(&ctx, TemplateCommands::Validate)
            .await
            .expect("validate should succeed");

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn template_validate_warns_empty_negative_path() {
        let dir = temp_dir();
        let ctx = test_ctx(&dir);
        fs::write(dir.join("AGENTS.md"), "").unwrap();

        let err = TemplateCommand::run(&ctx, TemplateCommands::Validate)
            .await
            .expect_err("validate should fail for empty template");
        assert!(err.to_string().contains("warnings"));

        fs::remove_dir_all(dir).ok();
    }
}
