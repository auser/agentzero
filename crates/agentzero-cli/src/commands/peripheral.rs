use crate::cli::PeripheralCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use async_trait::async_trait;

pub struct PeripheralCommand;

#[async_trait]
impl AgentZeroCommand for PeripheralCommand {
    type Options = PeripheralCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        #[cfg(not(feature = "hardware"))]
        {
            let _ = (ctx, opts);
            anyhow::bail!(
                "peripheral command requested but agentzero-cli was built without `hardware` feature"
            );
        }

        #[cfg(feature = "hardware")]
        {
            let registry_path = ctx.data_dir.join("peripherals").join("registry.json");
            let mut registry = agentzero_peripherals::PeripheralRegistry::load(&registry_path)?;

            match opts {
                PeripheralCommands::List { json } => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&registry)?);
                    } else if registry.peripherals.is_empty() {
                        println!("No peripherals registered");
                    } else {
                        println!("Registered peripherals ({}):", registry.peripherals.len());
                        for p in &registry.peripherals {
                            println!("  - {} ({}) {}", p.id, p.kind, p.connection);
                        }
                    }
                }
                PeripheralCommands::Add {
                    id,
                    kind,
                    connection,
                    json,
                } => match (id, kind, connection) {
                    (Some(id), Some(kind), Some(connection)) => {
                        registry.add(agentzero_peripherals::Peripheral {
                            id: id.clone(),
                            kind,
                            connection,
                        })?;
                        registry.save(&registry_path)?;
                        if json {
                            println!(
                                "{}",
                                serde_json::json!({
                                    "ok": true,
                                    "action": "add",
                                    "id": id,
                                })
                            );
                        } else {
                            println!("Registered peripheral {id}");
                        }
                    }
                    _ => {
                        if json {
                            println!(
                                "{}",
                                serde_json::json!({
                                    "ok": false,
                                    "action": "add",
                                    "message": "provide --id, --kind, and --connection to register a peripheral",
                                })
                            );
                        } else {
                            println!(
                                "Provide --id, --kind, and --connection to register a peripheral"
                            );
                        }
                    }
                },
                PeripheralCommands::Flash { id, firmware, json } => {
                    let message = format!(
                        "Flash requested (id={}, firmware={})",
                        id.as_deref().unwrap_or("default"),
                        firmware.as_deref().unwrap_or("auto")
                    );
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "ok": true,
                                "action": "flash",
                                "id": id,
                                "firmware": firmware,
                                "message": message,
                            })
                        );
                    } else {
                        println!("{message}");
                    }
                }
                PeripheralCommands::FlashNucleo { json } => {
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "ok": true,
                                "action": "flash-nucleo",
                                "message": "Nucleo flash profile requested",
                            })
                        );
                    } else {
                        println!("Nucleo flash profile requested");
                    }
                }
                PeripheralCommands::SetupUnoQ { host, json } => {
                    let target = host.as_deref().unwrap_or("local-device");
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "ok": true,
                                "action": "setup-uno-q",
                                "host": host,
                                "target": target,
                            })
                        );
                    } else {
                        println!("Uno Q setup requested for {target}");
                    }
                }
            }

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn peripheral_command_without_feature_fails_negative_path() {
        #[cfg(not(feature = "hardware"))]
        {
            use super::PeripheralCommand;
            use crate::cli::PeripheralCommands;
            use crate::command_core::{AgentZeroCommand, CommandContext};
            let ctx = CommandContext {
                workspace_root: std::env::temp_dir(),
                data_dir: std::env::temp_dir(),
                config_path: std::env::temp_dir().join("agentzero.toml"),
            };
            let err = PeripheralCommand::run(&ctx, PeripheralCommands::List { json: true })
                .await
                .expect_err("peripheral command should fail when feature disabled");
            assert!(err.to_string().contains("without `hardware` feature"));
        }
    }
}
