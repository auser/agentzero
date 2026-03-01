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

#[cfg(test)]
mod tests {
    use super::GatewayOptions;
    use std::path::PathBuf;

    #[test]
    fn gateway_options_default_host_port_success_path() {
        let opts = GatewayOptions {
            host: None,
            port: None,
            new_pairing: false,
        };
        let host = opts.host.unwrap_or_else(|| "127.0.0.1".to_string());
        let port = opts.port.unwrap_or(8080);
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 8080);
    }

    #[test]
    fn gateway_token_store_path_construction_success_path() {
        let data_dir = PathBuf::from("/tmp/test-data");
        let token_store_path = data_dir.join("gateway-paired-tokens.json");
        assert_eq!(
            token_store_path,
            PathBuf::from("/tmp/test-data/gateway-paired-tokens.json")
        );
    }
}
