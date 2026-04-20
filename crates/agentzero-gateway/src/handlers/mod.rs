mod agents;
mod auth_endpoints;
mod chat;
mod config;
mod control;
mod cron;
mod health;
mod jobs;
mod mcp;
mod memory;
mod sse;
mod swarm;
mod templates;
mod tools;
mod webhooks;
mod websocket;
mod workflows;
mod ws_subscribe;

pub(crate) use agents::*;
pub(crate) use auth_endpoints::*;
pub(crate) use chat::*;
pub(crate) use config::*;
pub(crate) use control::*;
pub(crate) use cron::*;
pub(crate) use health::*;
pub(crate) use jobs::*;
pub(crate) use mcp::*;
pub(crate) use memory::*;
pub(crate) use sse::*;
pub(crate) use templates::*;
pub(crate) use tools::*;
pub(crate) use webhooks::*;
pub(crate) use websocket::*;
pub(crate) use workflows::*;
pub(crate) use ws_subscribe::*;

// ---------------------------------------------------------------------------
// Shared imports — available to all sub-modules via `use super::*`
// ---------------------------------------------------------------------------

pub(crate) use crate::api_keys::Scope;
pub(crate) use crate::auth::{authorize_request, authorize_with_scope};
pub(crate) use crate::extractors::AppJson;
pub(crate) use crate::models::GatewayError;
pub(crate) use crate::state::GatewayState;

pub(crate) use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
pub(crate) use serde_json::{json, Value};
pub(crate) use std::sync::Arc;

// ---------------------------------------------------------------------------
// Shared utilities used across multiple handler modules
// ---------------------------------------------------------------------------

use agentzero_infra::runtime::RunAgentRequest;

/// Build a `RunAgentRequest` from gateway state and user parameters.
pub(super) fn build_agent_request(
    state: &GatewayState,
    message: String,
    model_override: Option<String>,
    capability_override: agentzero_core::security::CapabilitySet,
) -> Result<RunAgentRequest, GatewayError> {
    let config_path = state
        .config_path
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?
        .as_ref()
        .clone();
    let workspace_root = state
        .workspace_root
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?
        .as_ref()
        .clone();
    Ok(RunAgentRequest {
        workspace_root,
        config_path,
        message,
        provider_override: None,
        model_override,
        profile_override: None,
        extra_tools: vec![],
        conversation_id: None,
        agent_store: None,
        memory_override: None,
        memory_window_override: None,
        capability_set_override: capability_override,
    })
}

/// ISO 8601 timestamp for the current moment.
pub(super) fn chrono_now_iso() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{now}")
}
