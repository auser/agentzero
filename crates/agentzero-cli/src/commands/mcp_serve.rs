use crate::command_core::{AgentZeroCommand, CommandContext};
use async_trait::async_trait;
use std::sync::Arc;

pub struct McpServeCommand;

#[async_trait]
impl AgentZeroCommand for McpServeCommand {
    type Options = ();

    async fn run(ctx: &CommandContext, _opts: Self::Options) -> anyhow::Result<()> {
        let policy =
            agentzero_config::load_tool_security_policy(&ctx.workspace_root, &ctx.config_path)?;

        // Build tools without model routing — MCP clients handle their own routing.
        let tools = agentzero_infra::tools::default_tools(&policy, None, None)?;

        let tool_count = tools.len();
        let server = Arc::new(agentzero_infra::mcp_server::McpServer::new(
            tools,
            ctx.workspace_root.to_string_lossy().to_string(),
        ));

        // Log to stderr so stdout stays clean for JSON-RPC.
        eprintln!(
            "agentzero mcp-serve: serving {tool_count} tools over stdio (protocol 2025-11-05)"
        );

        agentzero_infra::mcp_server::run_stdio(server).await
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mcp_serve_command_exists() {
        // Verify the command type exists and can be referenced.
        let _ = std::any::type_name::<super::McpServeCommand>();
    }
}
