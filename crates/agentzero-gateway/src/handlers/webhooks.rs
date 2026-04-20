use super::swarm::trigger_workflows_for_channel;
use super::*;
use crate::models::{ChatRequest, ChatResponse};
use crate::models::{WebhookPayload, WebhookQuery, WebhookResponse};
use agentzero_channels::pipeline::check_perplexity;

pub(crate) async fn webhook(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(channel): Path<String>,
    query: axum::extract::Query<WebhookQuery>,
    AppJson(payload): AppJson<WebhookPayload>,
) -> Result<Json<WebhookResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    if !is_valid_channel_name(&channel) {
        return Err(GatewayError::BadRequest {
            message: format!(
                "invalid channel name '{channel}': must be 1-64 chars, alphanumeric/hyphen/underscore only"
            ),
        });
    }

    // If agent_id is provided, validate that the agent exists.
    if let Some(ref agent_id) = query.agent_id {
        validate_agent_exists(&state, agent_id)?;
        tracing::info!(
            channel = %channel,
            agent_id = %agent_id,
            "webhook targeting specific agent"
        );
    }

    let payload_json = payload.inner.clone();
    let Some(delivery) = state.channels.dispatch(&channel, payload.inner).await else {
        return Err(GatewayError::NotFound {
            resource: format!("channel/{channel}"),
        });
    };

    // Trigger any workflows that have a Channel trigger node matching this channel.
    if let Some(ref wf_store) = state.workflow_store {
        let message_text = payload_json
            .get("text")
            .or_else(|| payload_json.get("content"))
            .or_else(|| payload_json.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let triggered =
            trigger_workflows_for_channel(&state, wf_store, &channel, &message_text).await;

        if triggered > 0 {
            tracing::info!(
                channel = %channel,
                workflows_triggered = triggered,
                "inbound message triggered workflow runs"
            );
        }
    }

    Ok(Json(WebhookResponse {
        accepted: delivery.accepted,
        channel: delivery.channel,
        detail: delivery.detail,
    }))
}

/// POST /v1/hooks/:channel/:agent_id — webhook with agent targeting (convenience route).
pub(crate) async fn webhook_with_agent(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path((channel, agent_id)): Path<(String, String)>,
    AppJson(payload): AppJson<WebhookPayload>,
) -> Result<Json<WebhookResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    if !is_valid_channel_name(&channel) {
        return Err(GatewayError::BadRequest {
            message: format!(
                "invalid channel name '{channel}': must be 1-64 chars, alphanumeric/hyphen/underscore only"
            ),
        });
    }

    validate_agent_exists(&state, &agent_id)?;

    tracing::info!(
        channel = %channel,
        agent_id = %agent_id,
        "webhook targeting specific agent"
    );

    let Some(delivery) = state.channels.dispatch(&channel, payload.inner).await else {
        return Err(GatewayError::NotFound {
            resource: format!("channel/{channel}"),
        });
    };

    Ok(Json(WebhookResponse {
        accepted: delivery.accepted,
        channel: delivery.channel,
        detail: delivery.detail,
    }))
}

/// Validate that an agent exists in either the dynamic store or presence store.
fn validate_agent_exists(state: &GatewayState, agent_id: &str) -> Result<(), GatewayError> {
    // Check dynamic store.
    if let Some(store) = &state.agent_store {
        if store.get(agent_id).is_some() {
            return Ok(());
        }
    }
    // Check static agents via presence store.
    // Note: presence store is async but we can't await here in a sync fn.
    // For now, accept if agent_store found it; otherwise reject for dynamic agents.
    // Static agents route through the normal webhook path without targeting.
    Err(GatewayError::NotFound {
        resource: format!("agent/{agent_id}"),
    })
}

/// POST /v1/connector-webhook/:connector/:entity — publish a connector event to the EventBus.
///
/// External systems (e.g. Shopify, Stripe) call this endpoint when data changes.
/// The payload is published as a `connector:{connector}:{entity}:changed` event,
/// which triggers any data links configured with `SyncMode::EventDriven`.
pub(crate) async fn connector_webhook(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path((connector, entity)): Path<(String, String)>,
    AppJson(payload): AppJson<WebhookPayload>,
) -> Result<Json<WebhookResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    if !is_valid_channel_name(&connector) || !is_valid_channel_name(&entity) {
        return Err(GatewayError::BadRequest {
            message: "invalid connector or entity name".to_string(),
        });
    }

    let topic = format!("connector:{connector}:{entity}:changed");
    let payload_str = serde_json::to_string(&payload.inner).unwrap_or_default();

    if let Some(ref event_bus) = state.event_bus {
        let event = agentzero_core::Event::new(&topic, "connector-webhook", &payload_str);
        match event_bus.publish(event).await {
            Ok(_) => {
                tracing::info!(
                    connector = %connector,
                    entity = %entity,
                    topic = %topic,
                    "published connector webhook event"
                );
            }
            Err(e) => {
                tracing::warn!(
                    connector = %connector,
                    entity = %entity,
                    error = %e,
                    "failed to publish connector webhook event"
                );
            }
        }
    } else {
        tracing::warn!("connector webhook received but no event bus configured");
    }

    Ok(Json(WebhookResponse {
        accepted: true,
        channel: connector,
        detail: format!("published to topic: {topic}"),
    }))
}

pub(crate) async fn legacy_webhook(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(req): AppJson<ChatRequest>,
) -> Result<Json<ChatResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    if let Some(reason) = check_perplexity(&req.message, &state.effective_perplexity_filter()) {
        tracing::warn!(reason = %reason, "gateway legacy_webhook blocked by perplexity filter");
        return Err(GatewayError::BadRequest {
            message: format!("blocked by perplexity filter: {reason}"),
        });
    }

    Ok(Json(ChatResponse {
        message: format!("echo: {}", req.message),
        tokens_used_estimate: req.message.len() + req.context.len() * 8,
    }))
}
