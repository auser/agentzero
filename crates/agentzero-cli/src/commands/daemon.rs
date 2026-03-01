use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_daemon::DaemonManager;
use async_trait::async_trait;

pub struct DaemonOptions {
    pub host: Option<String>,
    pub port: Option<u16>,
}

pub struct DaemonCommand;

#[async_trait]
impl AgentZeroCommand for DaemonCommand {
    type Options = DaemonOptions;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let manager = DaemonManager::new(&ctx.data_dir)?;
        let host = opts.host.unwrap_or_else(|| "127.0.0.1".to_string());
        let port = opts.port.unwrap_or(8080);

        manager.mark_started(host.clone(), port)?;
        println!("Starting daemon runtime on {host}:{port}");

        let token_store_path = ctx.data_dir.join("gateway-paired-tokens.json");
        let run_result = agentzero_gateway::run(
            &host,
            port,
            agentzero_gateway::GatewayRunOptions {
                token_store_path: Some(token_store_path),
                new_pairing: false,
            },
        )
        .await;

        if let Err(err) = manager.mark_stopped() {
            eprintln!("Warning: failed to update daemon state after shutdown: {err}");
        }

        run_result
    }
}
