use crate::cli::AgentsCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_orchestrator::agent_store::{AgentRecord, AgentStatus, AgentStore, AgentUpdate};
use async_trait::async_trait;
use std::collections::HashMap;

pub struct AgentsCommand;

#[async_trait]
impl AgentZeroCommand for AgentsCommand {
    type Options = AgentsCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let store = AgentStore::persistent(&ctx.data_dir)?;
        match opts {
            AgentsCommands::Create {
                name,
                description,
                model,
                provider,
                system_prompt,
                keywords,
                allowed_tools,
                json: emit_json,
            } => {
                let record = AgentRecord {
                    agent_id: String::new(),
                    name,
                    description: description.unwrap_or_default(),
                    system_prompt,
                    provider: provider.unwrap_or_default(),
                    model: model.unwrap_or_default(),
                    keywords,
                    allowed_tools,
                    channels: HashMap::new(),
                    created_at: 0,
                    updated_at: 0,
                    status: AgentStatus::Active,
                };
                let created = store.create(record)?;
                if emit_json {
                    println!("{}", serde_json::to_string_pretty(&created)?);
                } else {
                    println!(
                        "Created agent '{}' (id: {})",
                        created.name, created.agent_id
                    );
                }
            }
            AgentsCommands::List { json: emit_json } => {
                let agents = store.list();
                if emit_json {
                    println!("{}", serde_json::to_string_pretty(&agents)?);
                } else {
                    println!("Persistent agents ({})", agents.len());
                    for a in &agents {
                        let status = match a.status {
                            AgentStatus::Active => "active",
                            AgentStatus::Stopped => "stopped",
                        };
                        println!(
                            "  {} [{}]  model={}  keywords=[{}]  id={}",
                            a.name,
                            status,
                            if a.model.is_empty() {
                                "(default)"
                            } else {
                                &a.model
                            },
                            a.keywords.join(", "),
                            a.agent_id,
                        );
                    }
                }
            }
            AgentsCommands::Get {
                id,
                json: emit_json,
            } => {
                let record = store
                    .get(&id)
                    .ok_or_else(|| anyhow::anyhow!("agent '{id}' not found"))?;
                if emit_json {
                    println!("{}", serde_json::to_string_pretty(&record)?);
                } else {
                    println!("Agent: {}", record.name);
                    println!("  ID:           {}", record.agent_id);
                    println!("  Description:  {}", record.description);
                    println!(
                        "  Provider:     {}",
                        if record.provider.is_empty() {
                            "(default)"
                        } else {
                            &record.provider
                        }
                    );
                    println!(
                        "  Model:        {}",
                        if record.model.is_empty() {
                            "(default)"
                        } else {
                            &record.model
                        }
                    );
                    println!(
                        "  System prompt: {}",
                        record.system_prompt.as_deref().unwrap_or("(none)")
                    );
                    println!("  Keywords:     [{}]", record.keywords.join(", "));
                    println!(
                        "  Allowed tools: [{}]",
                        if record.allowed_tools.is_empty() {
                            "all".to_string()
                        } else {
                            record.allowed_tools.join(", ")
                        }
                    );
                    let status = match record.status {
                        AgentStatus::Active => "active",
                        AgentStatus::Stopped => "stopped",
                    };
                    println!("  Status:       {status}");
                }
            }
            AgentsCommands::Update {
                id,
                name,
                description,
                model,
                provider,
                system_prompt,
                keywords,
                allowed_tools,
                json: emit_json,
            } => {
                let update = AgentUpdate {
                    name,
                    description,
                    system_prompt,
                    provider,
                    model,
                    keywords,
                    allowed_tools,
                    channels: None,
                };
                match store.update(&id, update)? {
                    Some(updated) => {
                        if emit_json {
                            println!("{}", serde_json::to_string_pretty(&updated)?);
                        } else {
                            println!(
                                "Updated agent '{}' (id: {})",
                                updated.name, updated.agent_id
                            );
                        }
                    }
                    None => anyhow::bail!("agent '{id}' not found"),
                }
            }
            AgentsCommands::Delete { id } => {
                if store.delete(&id)? {
                    println!("Deleted agent '{id}'");
                } else {
                    anyhow::bail!("agent '{id}' not found");
                }
            }
            AgentsCommands::Status {
                id,
                active,
                stopped,
            } => {
                let status = if active {
                    AgentStatus::Active
                } else if stopped {
                    AgentStatus::Stopped
                } else {
                    anyhow::bail!("specify --active or --stopped");
                };
                let label = if active { "active" } else { "stopped" };
                if store.set_status(&id, status)? {
                    println!("Agent '{id}' status set to '{label}'");
                } else {
                    anyhow::bail!("agent '{id}' not found");
                }
            }
        }
        Ok(())
    }
}
