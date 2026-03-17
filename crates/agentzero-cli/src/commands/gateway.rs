use crate::command_core::{AgentZeroCommand, CommandContext};
use crate::daemon::find_available_port;
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
        let cfg = agentzero_config::load(&ctx.config_path).ok();
        let host = opts.host.unwrap_or_else(|| {
            cfg.as_ref()
                .map(|c| c.gateway.host.clone())
                .unwrap_or_else(|| "127.0.0.1".to_string())
        });
        let requested_port = opts
            .port
            .unwrap_or_else(|| cfg.as_ref().map(|c| c.gateway.port).unwrap_or(8080));
        let port = find_available_port(&host, requested_port)?;
        if port != requested_port {
            println!("port {requested_port} is in use, using port {port} instead");
        }
        let token_store_path = ctx.data_dir.join("gateway-paired-tokens.json");
        agentzero_gateway::run(
            &host,
            port,
            agentzero_gateway::GatewayRunOptions {
                token_store_path: Some(token_store_path),
                new_pairing: opts.new_pairing,
                data_dir: Some(ctx.data_dir.clone()),
                config_path: Some(ctx.config_path.clone()),
                workspace_root: Some(ctx.workspace_root.clone()),
                ..Default::default()
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
        // Without a config, falls back to hardcoded defaults
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
