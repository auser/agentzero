//! Event-driven sync: listens on the EventBus for connector events and
//! triggers data syncs when matching events arrive.

use crate::registry::ConnectorRegistry;
use crate::sync_engine;
use crate::templates::{ReadRequest, WriteRequest};
use crate::SyncMode;
use agentzero_core::EventBus;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Spawn background tasks that listen for EventBus events and trigger
/// syncs for data links with `SyncMode::EventDriven`.
///
/// Returns a `JoinHandle` for each listener spawned. The listeners run
/// until the EventBus channel closes or the task is cancelled.
pub fn spawn_event_listeners(
    registry: Arc<RwLock<ConnectorRegistry>>,
    event_bus: Arc<dyn EventBus>,
) -> Vec<tokio::task::JoinHandle<()>> {
    let rt = tokio::runtime::Handle::current();
    let mut handles = Vec::new();

    // We need to read the links snapshot to find event-driven ones.
    // This is called at startup, so we block briefly.
    let links_snapshot: Vec<(String, String)> = rt.block_on(async {
        let reg = registry.read().await;
        reg.links()
            .values()
            .filter_map(|link| {
                if let SyncMode::EventDriven { ref event_topic } = link.sync_mode {
                    Some((link.id.clone(), event_topic.clone()))
                } else {
                    None
                }
            })
            .collect()
    });

    for (link_id, event_topic) in links_snapshot {
        let registry = Arc::clone(&registry);
        let event_bus = Arc::clone(&event_bus);

        let handle = tokio::spawn(async move {
            info!(
                link_id = %link_id,
                topic = %event_topic,
                "started event-driven sync listener"
            );

            let mut subscriber = event_bus.subscribe();

            loop {
                match subscriber.recv().await {
                    Ok(event) => {
                        if event.topic != event_topic {
                            continue;
                        }

                        info!(
                            link_id = %link_id,
                            event_id = %event.id,
                            "event-driven sync triggered"
                        );

                        if let Err(e) = execute_sync(&link_id, &registry).await {
                            warn!(
                                link_id = %link_id,
                                error = %e,
                                "event-driven sync failed"
                            );
                        }
                    }
                    Err(e) => {
                        warn!(
                            link_id = %link_id,
                            error = %e,
                            "event subscriber error, stopping listener"
                        );
                        break;
                    }
                }
            }
        });

        handles.push(handle);
    }

    if !handles.is_empty() {
        info!(count = handles.len(), "spawned event-driven sync listeners");
    }

    handles
}

/// Execute a full sync for a data link.
async fn execute_sync(
    link_id: &str,
    registry: &Arc<RwLock<ConnectorRegistry>>,
) -> anyhow::Result<()> {
    let reg = registry.read().await;
    let link = reg
        .link(link_id)
        .ok_or_else(|| anyhow::anyhow!("link `{link_id}` not found"))?
        .clone();

    // Pre-flight validation.
    let validation_errors = reg.validate_link_mappings(&link);
    if !validation_errors.is_empty() {
        anyhow::bail!(
            "pre-flight validation failed for link `{link_id}`: {}",
            validation_errors.join("; ")
        );
    }

    let target_primary_key = reg
        .manifest(&link.target.connector)
        .and_then(|m| {
            m.entities
                .iter()
                .find(|e| e.name == link.target.entity)
                .map(|e| e.primary_key.clone())
        })
        .unwrap_or_else(|| "id".to_string());

    // Read → transform → write loop.
    let mut cursor = link.last_sync_cursor.clone();
    let mut total_written = 0u64;

    loop {
        let read_result = reg
            .read_records(
                &link.source.connector,
                &ReadRequest {
                    entity: link.source.entity.clone(),
                    cursor: cursor.clone(),
                    batch_size: 100,
                },
            )
            .await?;

        if read_result.records.is_empty() {
            break;
        }

        let (transformed, _errors) = sync_engine::transform_batch(&link, &read_result.records);

        if !transformed.is_empty() {
            let write_result = reg
                .write_records(
                    &link.target.connector,
                    &WriteRequest {
                        entity: link.target.entity.clone(),
                        records: transformed,
                        primary_key: target_primary_key.clone(),
                    },
                )
                .await?;

            total_written += write_result.written;
        }

        cursor = read_result.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    drop(reg);

    // Update sync state.
    let mut reg = registry.write().await;
    if let Some(link_mut) = reg.link_mut(link_id) {
        if let Some(ref c) = cursor {
            link_mut.last_sync_cursor = Some(c.clone());
        }
        link_mut.last_sync_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
    }
    reg.persist_sync_state();

    info!(
        link_id = %link_id,
        records_written = total_written,
        "event-driven sync completed"
    );

    Ok(())
}
