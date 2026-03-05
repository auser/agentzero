use crate::cli::ToolsCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_core::Tool;
use agentzero_infra::tools::default_tools;
use agentzero_tools::ToolSecurityPolicy;
use async_trait::async_trait;

pub struct ToolsCommand;

#[async_trait]
impl AgentZeroCommand for ToolsCommand {
    type Options = ToolsCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let policy = ToolSecurityPolicy::default_for_workspace(ctx.workspace_root.clone());
        let tools = default_tools(&policy, None, None)?;

        match opts {
            ToolsCommands::List { with_schema, json } => run_list(&tools, with_schema, json),
            ToolsCommands::Info { name } => run_info(&tools, &name),
            ToolsCommands::Schema { name, pretty } => run_schema(&tools, &name, pretty),
        }
    }
}

fn run_list(tools: &[Box<dyn Tool>], with_schema: bool, json: bool) -> anyhow::Result<()> {
    let items: Vec<_> = tools
        .iter()
        .filter(|t| !with_schema || t.input_schema().is_some())
        .collect();

    if json {
        let entries: Vec<serde_json::Value> = items
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "has_schema": t.input_schema().is_some(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        println!("{:<30} {:<10} DESCRIPTION", "NAME", "SCHEMA");
        println!("{}", "-".repeat(80));
        for t in &items {
            let has_schema = if t.input_schema().is_some() {
                "yes"
            } else {
                "no"
            };
            let desc = t.description();
            // Truncate description to fit terminal
            let desc_display = if desc.len() > 60 {
                format!("{}...", &desc[..57])
            } else {
                desc.to_string()
            };
            println!("{:<30} {:<10} {}", t.name(), has_schema, desc_display);
        }
        println!("\n{} tools registered", items.len());
    }

    Ok(())
}

fn run_info(tools: &[Box<dyn Tool>], name: &str) -> anyhow::Result<()> {
    let tool = tools
        .iter()
        .find(|t| t.name() == name)
        .ok_or_else(|| anyhow::anyhow!("tool not found: {}", name))?;

    println!("Name:        {}", tool.name());
    println!("Description: {}", tool.description());
    if let Some(schema) = tool.input_schema() {
        println!("Has schema:  yes");
        println!("\nInput schema:");
        println!("{}", serde_json::to_string_pretty(&schema)?);
    } else {
        println!("Has schema:  no (accepts free-form text input)");
    }

    Ok(())
}

fn run_schema(tools: &[Box<dyn Tool>], name: &str, pretty: bool) -> anyhow::Result<()> {
    let tool = tools
        .iter()
        .find(|t| t.name() == name)
        .ok_or_else(|| anyhow::anyhow!("tool not found: {}", name))?;

    match tool.input_schema() {
        Some(schema) => {
            if pretty {
                println!("{}", serde_json::to_string_pretty(&schema)?);
            } else {
                println!("{}", serde_json::to_string(&schema)?);
            }
        }
        None => {
            anyhow::bail!(
                "tool \"{}\" has no JSON schema (accepts free-form text)",
                name
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolDefinition;
    use std::path::PathBuf;

    #[test]
    fn list_tools_produces_output() {
        let policy = ToolSecurityPolicy::default_for_workspace(PathBuf::from("/tmp"));
        let tools = default_tools(&policy, None, None).expect("default_tools should succeed");
        assert!(!tools.is_empty(), "should have at least one tool");
    }

    #[test]
    fn tool_definition_from_tool_works_for_schema_tools() {
        let policy = ToolSecurityPolicy::default_for_workspace(PathBuf::from("/tmp"));
        let tools = default_tools(&policy, None, None).unwrap();
        let with_schemas: Vec<ToolDefinition> = tools
            .iter()
            .filter_map(|t| ToolDefinition::from_tool(t.as_ref()))
            .collect();
        assert!(
            !with_schemas.is_empty(),
            "at least one tool should have a schema"
        );
    }
}
