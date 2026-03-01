use crate::cli::{IdentityCommands, IdentityKind};
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_identity::{ActorIdentity, ActorKind};
use agentzero_storage::EncryptedJsonStore;
use async_trait::async_trait;
use serde::Serialize;
use std::collections::BTreeMap;

const IDENTITIES_FILE: &str = "identities.json";

#[derive(Debug, Serialize)]
struct IdentityOutput<'a> {
    id: &'a str,
    name: &'a str,
    kind: &'a str,
    roles: Vec<String>,
}

pub struct IdentityCommand;

#[async_trait]
impl AgentZeroCommand for IdentityCommand {
    type Options = IdentityCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let store = EncryptedJsonStore::in_config_dir(&ctx.data_dir, IDENTITIES_FILE)?;
        let mut identities = store.load_or_default::<BTreeMap<String, ActorIdentity>>()?;

        match opts {
            IdentityCommands::Upsert {
                id,
                name,
                kind,
                json,
            } => {
                let actor = ActorIdentity::new(&id, &name, map_kind(kind))
                    .map_err(|err| anyhow::anyhow!(err.to_string()))?;
                identities.insert(id.clone(), actor.clone());
                store.save(&identities)?;
                emit_identity("upserted", &actor, json)?;
            }
            IdentityCommands::Get { id, json } => {
                let actor = identities
                    .get(&id)
                    .ok_or_else(|| anyhow::anyhow!("identity `{id}` not found"))?;
                emit_identity("identity", actor, json)?;
            }
            IdentityCommands::AddRole { id, role, json } => {
                let actor = identities
                    .get_mut(&id)
                    .ok_or_else(|| anyhow::anyhow!("identity `{id}` not found"))?;
                actor
                    .add_role(&role)
                    .map_err(|err| anyhow::anyhow!(err.to_string()))?;
                let updated = actor.clone();
                store.save(&identities)?;
                emit_identity("updated", &updated, json)?;
            }
        }

        Ok(())
    }
}

fn map_kind(kind: IdentityKind) -> ActorKind {
    match kind {
        IdentityKind::Human => ActorKind::Human,
        IdentityKind::Agent => ActorKind::Agent,
        IdentityKind::Service => ActorKind::Service,
    }
}

fn emit_identity(prefix: &str, actor: &ActorIdentity, json: bool) -> anyhow::Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&IdentityOutput {
                id: &actor.id,
                name: &actor.display_name,
                kind: &format!("{:?}", actor.kind).to_ascii_lowercase(),
                roles: actor.roles.iter().cloned().collect::<Vec<_>>(),
            })?
        );
    } else {
        println!(
            "{}: {} ({}) roles=[{}]",
            prefix,
            actor.id,
            format!("{:?}", actor.kind).to_ascii_lowercase(),
            actor.roles.iter().cloned().collect::<Vec<_>>().join(", ")
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::IdentityCommand;
    use crate::cli::{IdentityCommands, IdentityKind};
    use crate::command_core::{AgentZeroCommand, CommandContext};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("agentzero-identity-cmd-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn identity_upsert_get_add_role_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        IdentityCommand::run(
            &ctx,
            IdentityCommands::Upsert {
                id: "operator-1".to_string(),
                name: "Operator".to_string(),
                kind: IdentityKind::Human,
                json: false,
            },
        )
        .await
        .expect("upsert should succeed");

        IdentityCommand::run(
            &ctx,
            IdentityCommands::AddRole {
                id: "operator-1".to_string(),
                role: "admin".to_string(),
                json: true,
            },
        )
        .await
        .expect("add role should succeed");

        IdentityCommand::run(
            &ctx,
            IdentityCommands::Get {
                id: "operator-1".to_string(),
                json: true,
            },
        )
        .await
        .expect("get should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn identity_add_role_invalid_format_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        IdentityCommand::run(
            &ctx,
            IdentityCommands::Upsert {
                id: "operator-1".to_string(),
                name: "Operator".to_string(),
                kind: IdentityKind::Human,
                json: false,
            },
        )
        .await
        .expect("upsert should succeed");

        let err = IdentityCommand::run(
            &ctx,
            IdentityCommands::AddRole {
                id: "operator-1".to_string(),
                role: "Admin Role".to_string(),
                json: false,
            },
        )
        .await
        .expect_err("invalid role should fail");
        assert!(err.to_string().contains("snake_case"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
