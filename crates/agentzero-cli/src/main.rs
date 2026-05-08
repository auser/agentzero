use clap::Parser;

mod commands;

#[derive(Parser)]
#[command(name = "az", about = "AgentZero — secure AI agent runtime", version)]
struct Cli {
    #[command(subcommand)]
    command: commands::Command,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let code = commands::run(cli.command).await;
    std::process::exit(code);
}
