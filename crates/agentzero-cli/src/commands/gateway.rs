use crate::command_core::{AgentZeroCommand, CommandContext};
use async_trait::async_trait;

pub struct GatewayOptions {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub new_pairing: bool,
}

pub struct GatewayCommand;

#[async_trait]
impl AgentZeroCommand for GatewayCommand {
    type Options = GatewayOptions;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let host = opts.host.unwrap_or_else(|| "127.0.0.1".to_string());
        let port = opts.port.unwrap_or(8080);
        let token_store_path = ctx.data_dir.join("gateway-paired-tokens.json");
        agentzero_gateway::run(
            &host,
            port,
            agentzero_gateway::GatewayRunOptions {
                token_store_path: Some(token_store_path),
                new_pairing: opts.new_pairing,
            },
        )
        .await
    }
}
