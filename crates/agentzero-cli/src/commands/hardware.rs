use crate::cli::HardwareCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use async_trait::async_trait;

pub struct HardwareCommand;

#[async_trait]
impl AgentZeroCommand for HardwareCommand {
    type Options = HardwareCommands;

    async fn run(_ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        #[cfg(not(feature = "hardware"))]
        {
            let _ = opts;
            anyhow::bail!(
                "hardware command requested but agentzero-cli was built without `hardware` feature"
            );
        }

        #[cfg(feature = "hardware")]
        {
            match opts {
                HardwareCommands::Discover => {
                    let boards = agentzero_hardware::discover_boards();
                    println!("Discovered hardware boards ({}):", boards.len());
                    for board in boards {
                        println!(
                            "  - {} ({}, {} KB)",
                            board.id, board.architecture, board.memory_kb
                        );
                    }
                }
                HardwareCommands::Info { chip } => {
                    let board = resolve_board_from_chip(&chip)?;
                    println!("Hardware board {}", board.id);
                    println!("  chip: {}", chip);
                    println!("  id: {}", board.id);
                    println!("  name: {}", board.display_name);
                    println!("  architecture: {}", board.architecture);
                    println!("  memory_kb: {}", board.memory_kb);
                }
                HardwareCommands::Introspect => {
                    let boards = agentzero_hardware::discover_boards();
                    println!("Hardware introspection");
                    println!("  known_boards: {}", boards.len());
                    for board in boards {
                        println!("  - {} ({})", board.id, board.display_name);
                    }
                }
            }
            Ok(())
        }
    }
}

#[cfg(feature = "hardware")]
fn resolve_board_from_chip(chip: &str) -> anyhow::Result<agentzero_hardware::HardwareBoard> {
    let normalized = chip.trim().to_ascii_lowercase();
    let board_id = if normalized.starts_with("stm32") {
        "sim-stm32"
    } else if normalized.contains("rpi") || normalized.contains("raspberry") {
        "sim-rpi"
    } else {
        anyhow::bail!("unknown chip: {chip}");
    };

    agentzero_hardware::board_info(board_id)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn hardware_command_without_feature_fails_negative_path() {
        #[cfg(not(feature = "hardware"))]
        {
            use super::HardwareCommand;
            use crate::cli::HardwareCommands;
            use crate::command_core::{AgentZeroCommand, CommandContext};
            let ctx = CommandContext {
                workspace_root: std::env::temp_dir(),
                data_dir: std::env::temp_dir(),
                config_path: std::env::temp_dir().join("agentzero.toml"),
            };
            let err = HardwareCommand::run(&ctx, HardwareCommands::Discover)
                .await
                .expect_err("hardware command should fail when feature disabled");
            assert!(err.to_string().contains("without `hardware` feature"));
        }
    }
}
