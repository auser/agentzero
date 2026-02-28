use crate::command_core::{AgentZeroCommand, CommandContext};
use async_trait::async_trait;

pub struct GatewayOptions {
    pub host: Option<String>,
    pub port: Option<u16>,
}

pub struct GatewayCommand;

#[async_trait]
impl AgentZeroCommand for GatewayCommand {
    type Options = GatewayOptions;

    async fn run(_ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let host = opts.host.unwrap_or_else(|| "127.0.0.1".to_string());
        let port = opts.port.unwrap_or(8080);
        agentzero_gateway::run(&host, port).await
    }
}
