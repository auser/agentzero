use crate::cli::IntegrationsCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use crate::integrations::IntegrationDescriptor;
use async_trait::async_trait;

pub struct IntegrationsCommand;

#[async_trait]
impl AgentZeroCommand for IntegrationsCommand {
    type Options = IntegrationsCommands;

    async fn run(_ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let catalog = integrations_catalog();

        match opts {
            IntegrationsCommands::Info => {
                println!("Integrations platform");
                println!("  total integrations: {}", catalog.len());
                println!("  status values: active, available, coming-soon");
                println!("  categories: chat, ai, productivity, storage, infra");
            }
            IntegrationsCommands::List { category, status } => {
                let filtered = catalog
                    .into_iter()
                    .filter(|item| {
                        category
                            .as_deref()
                            .map(|needle| item_category(item).eq_ignore_ascii_case(needle))
                            .unwrap_or(true)
                    })
                    .filter(|item| {
                        status
                            .as_deref()
                            .map(|needle| item_status(item).eq_ignore_ascii_case(needle))
                            .unwrap_or(true)
                    })
                    .collect::<Vec<_>>();

                println!("Integrations ({})", filtered.len());
                for integration in &filtered {
                    println!(
                        "- {} [{}] ({})",
                        integration.id,
                        item_status(integration),
                        item_category(integration)
                    );
                }
            }
            IntegrationsCommands::Search { query } => {
                let query = query.unwrap_or_default().to_ascii_lowercase();
                let filtered = integrations_catalog()
                    .into_iter()
                    .filter(|item| {
                        query.is_empty()
                            || item.id.to_ascii_lowercase().contains(&query)
                            || item.display_name.to_ascii_lowercase().contains(&query)
                    })
                    .collect::<Vec<_>>();

                println!("Search results ({})", filtered.len());
                for integration in &filtered {
                    println!("- {} ({})", integration.id, integration.display_name);
                }
            }
        }

        Ok(())
    }
}

fn item_status(item: &IntegrationDescriptor) -> &'static str {
    if item.enabled_by_default {
        "active"
    } else {
        "available"
    }
}

fn item_category(item: &IntegrationDescriptor) -> &'static str {
    match item.id.as_str() {
        "discord" | "slack" | "telegram" => "chat",
        "github" | "gitlab" | "jira" | "linear" => "productivity",
        "google-drive" | "dropbox" | "s3" => "storage",
        "postgres" | "mysql" | "redis" => "infra",
        _ => "ai",
    }
}

fn integrations_catalog() -> Vec<IntegrationDescriptor> {
    vec![
        integration("discord", "Discord", true),
        integration("slack", "Slack", true),
        integration("telegram", "Telegram", true),
        integration("github", "GitHub", false),
        integration("gitlab", "GitLab", false),
        integration("notion", "Notion", false),
        integration("jira", "Jira", false),
        integration("linear", "Linear", false),
        integration("google-drive", "Google Drive", false),
        integration("dropbox", "Dropbox", false),
        integration("s3", "Amazon S3", false),
        integration("postgres", "Postgres", false),
        integration("mysql", "MySQL", false),
        integration("redis", "Redis", false),
        integration("twilio", "Twilio", false),
        integration("sendgrid", "SendGrid", false),
        integration("stripe", "Stripe", false),
        integration("hubspot", "HubSpot", false),
        integration("zendesk", "Zendesk", false),
        integration("salesforce", "Salesforce", false),
    ]
}

fn integration(id: &str, display_name: &str, enabled_by_default: bool) -> IntegrationDescriptor {
    IntegrationDescriptor {
        id: id.to_string(),
        display_name: display_name.to_string(),
        enabled_by_default,
    }
}

#[cfg(test)]
mod tests {
    use super::IntegrationsCommand;
    use crate::cli::IntegrationsCommands;
    use crate::command_core::{AgentZeroCommand, CommandContext};

    #[tokio::test]
    async fn integrations_list_success_path() {
        let ctx = CommandContext {
            workspace_root: std::env::temp_dir(),
            data_dir: std::env::temp_dir(),
            config_path: std::env::temp_dir().join("agentzero.toml"),
        };

        IntegrationsCommand::run(
            &ctx,
            IntegrationsCommands::List {
                category: None,
                status: None,
            },
        )
        .await
        .expect("list should succeed");
    }

    #[tokio::test]
    async fn integrations_search_success_path() {
        let ctx = CommandContext {
            workspace_root: std::env::temp_dir(),
            data_dir: std::env::temp_dir(),
            config_path: std::env::temp_dir().join("agentzero.toml"),
        };

        IntegrationsCommand::run(
            &ctx,
            IntegrationsCommands::Search {
                query: Some("discord".to_string()),
            },
        )
        .await
        .expect("search should succeed");
    }

    #[tokio::test]
    async fn integrations_info_success_path() {
        let ctx = CommandContext {
            workspace_root: std::env::temp_dir(),
            data_dir: std::env::temp_dir(),
            config_path: std::env::temp_dir().join("agentzero.toml"),
        };

        IntegrationsCommand::run(&ctx, IntegrationsCommands::Info)
            .await
            .expect("info should succeed");
    }

    #[tokio::test]
    async fn integrations_search_no_match_returns_empty_success_path() {
        let ctx = CommandContext {
            workspace_root: std::env::temp_dir(),
            data_dir: std::env::temp_dir(),
            config_path: std::env::temp_dir().join("agentzero.toml"),
        };

        IntegrationsCommand::run(
            &ctx,
            IntegrationsCommands::Search {
                query: Some("zzz-nonexistent".to_string()),
            },
        )
        .await
        .expect("search with no match should succeed");
    }

    #[tokio::test]
    async fn integrations_list_with_category_filter_success_path() {
        let ctx = CommandContext {
            workspace_root: std::env::temp_dir(),
            data_dir: std::env::temp_dir(),
            config_path: std::env::temp_dir().join("agentzero.toml"),
        };

        IntegrationsCommand::run(
            &ctx,
            IntegrationsCommands::List {
                category: Some("chat".to_string()),
                status: None,
            },
        )
        .await
        .expect("list with category filter should succeed");
    }
}
