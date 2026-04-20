use super::*;
use crate::models::{
    ApiFallbackResponse, ChatCompletionsRequest, ChatCompletionsResponse, ChatRequest,
    ChatResponse, CompletionChoice, CompletionChoiceMessage, ModelItem, ModelsResponse,
};
use crate::util::now_epoch_secs;
use agentzero_channels::pipeline::check_perplexity;
use agentzero_infra::runtime::run_agent_once;
use agentzero_infra::runtime::{build_runtime_execution, run_agent_streaming};
use std::time::Duration;

pub(crate) async fn api_chat(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(req): AppJson<ChatRequest>,
) -> Result<Response, GatewayError> {
    let identity = authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    if let Some(reason) = check_perplexity(&req.message, &state.effective_perplexity_filter()) {
        tracing::warn!(reason = %reason, "gateway api_chat blocked by perplexity filter");
        return Err(GatewayError::BadRequest {
            message: format!("blocked by perplexity filter: {reason}"),
        });
    }

    // When the swarm is enabled, route through the gateway channel so
    // pipelines (e.g. research-to-brief) can handle the request.
    if let Some(ref gw_channel) = state.gateway_channel {
        let response = gw_channel
            .submit(req.message, Duration::from_secs(600))
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "api_chat pipeline execution failed");
                GatewayError::AgentExecutionFailed {
                    message: e.to_string(),
                }
            })?;

        return Ok(Json(ChatResponse {
            message: response,
            tokens_used_estimate: 0,
        })
        .into_response());
    }

    // Fallback: single-agent execution (no swarm).
    let agent_req = build_agent_request(&state, req.message, None, identity.capability_ceiling)?;
    let output = run_agent_once(agent_req).await.map_err(|e| {
        tracing::error!(error = %e, "api_chat agent execution failed");
        GatewayError::AgentExecutionFailed {
            message: e.to_string(),
        }
    })?;

    let mut response = Json(ChatResponse {
        message: output.response_text,
        tokens_used_estimate: 0,
    })
    .into_response();

    // Append fallback headers if a provider fallback occurred.
    for (name, value) in fallback_response_headers() {
        if let (Ok(header_name), Ok(header_value)) = (
            axum::http::HeaderName::try_from(&name),
            axum::http::HeaderValue::try_from(&value),
        ) {
            response.headers_mut().insert(header_name, header_value);
        }
    }

    Ok(response)
}

pub(crate) async fn v1_chat_completions(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(req): AppJson<ChatCompletionsRequest>,
) -> Result<Response, GatewayError> {
    let identity = authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    let last_user = req
        .messages
        .iter()
        .rev()
        .find(|msg| msg.role == "user")
        .map(|msg| msg.content.clone())
        .unwrap_or_else(|| "hello".to_string());

    if let Some(reason) = check_perplexity(&last_user, &state.effective_perplexity_filter()) {
        tracing::warn!(reason = %reason, "gateway chat_completions blocked by perplexity filter");
        return Err(GatewayError::BadRequest {
            message: format!("blocked by perplexity filter: {reason}"),
        });
    }

    let model_override = req.model;

    if req.stream {
        return v1_chat_completions_stream(
            &state,
            &last_user,
            model_override,
            identity.capability_ceiling.clone(),
        )
        .await;
    }

    // Route through swarm pipeline when available.
    if let Some(ref gw_channel) = state.gateway_channel {
        let response = gw_channel
            .submit(last_user, Duration::from_secs(600))
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "v1_chat_completions pipeline execution failed");
                GatewayError::AgentExecutionFailed {
                    message: e.to_string(),
                }
            })?;

        return Ok(Json(ChatCompletionsResponse {
            id: format!("chatcmpl-{}", now_epoch_secs()),
            object: "chat.completion",
            choices: vec![CompletionChoice {
                index: 0,
                message: CompletionChoiceMessage {
                    role: "assistant",
                    content: response,
                },
                finish_reason: "stop",
            }],
        })
        .into_response());
    }

    let agent_req = build_agent_request(
        &state,
        last_user,
        model_override,
        identity.capability_ceiling,
    )?;
    let output = run_agent_once(agent_req).await.map_err(|e| {
        tracing::error!(error = %e, "v1_chat_completions agent execution failed");
        GatewayError::AgentExecutionFailed {
            message: e.to_string(),
        }
    })?;

    let mut response = Json(ChatCompletionsResponse {
        id: format!("chatcmpl-{}", now_epoch_secs()),
        object: "chat.completion",
        choices: vec![CompletionChoice {
            index: 0,
            message: CompletionChoiceMessage {
                role: "assistant",
                content: output.response_text,
            },
            finish_reason: "stop",
        }],
    })
    .into_response();

    // Append fallback headers if a provider fallback occurred.
    for (name, value) in fallback_response_headers() {
        if let (Ok(header_name), Ok(header_value)) = (
            axum::http::HeaderName::try_from(&name),
            axum::http::HeaderValue::try_from(&value),
        ) {
            response.headers_mut().insert(header_name, header_value);
        }
    }

    Ok(response)
}

/// SSE streaming variant of v1/chat/completions (OpenAI-compatible).
async fn v1_chat_completions_stream(
    state: &GatewayState,
    message: &str,
    model_override: Option<String>,
    capability_override: agentzero_core::security::CapabilitySet,
) -> Result<Response, GatewayError> {
    let agent_req = build_agent_request(
        state,
        message.to_string(),
        model_override,
        capability_override,
    )?;
    let execution = build_runtime_execution(agent_req).await.map_err(|e| {
        tracing::error!(error = %e, "v1_chat_completions_stream build failed");
        GatewayError::AgentExecutionFailed {
            message: e.to_string(),
        }
    })?;

    let workspace_root = state
        .workspace_root
        .as_ref()
        .ok_or(GatewayError::AgentUnavailable)?
        .as_ref()
        .clone();

    let (mut rx, _handle) = run_agent_streaming(execution, workspace_root, message.to_string());
    let id = format!("chatcmpl-{}", now_epoch_secs());

    let stream = async_stream::stream! {
        while let Some(chunk) = rx.recv().await {
            if chunk.done {
                let data = json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": "stop"
                    }]
                });
                yield Ok::<_, std::convert::Infallible>(
                    axum::response::sse::Event::default().data(data.to_string())
                );
                yield Ok(axum::response::sse::Event::default().data("[DONE]"));
                break;
            }
            if !chunk.delta.is_empty() {
                let data = json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "choices": [{
                        "index": 0,
                        "delta": {"role": "assistant", "content": chunk.delta},
                        "finish_reason": null
                    }]
                });
                yield Ok::<_, std::convert::Infallible>(
                    axum::response::sse::Event::default().data(data.to_string())
                );
            }
        }
    };

    Ok(axum::response::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response())
}

pub(crate) async fn v1_models(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<ModelsResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let mut models = Vec::new();
    for provider in agentzero_providers::supported_providers() {
        if let Some((_pid, provider_models)) =
            agentzero_providers::find_models_for_provider(provider.id)
        {
            for model in provider_models {
                models.push(ModelItem {
                    id: model.id.to_string(),
                    object: "model",
                    owned_by: provider.id.to_string(),
                });
            }
        }
    }

    Ok(Json(ModelsResponse {
        object: "list",
        data: models,
    }))
}

pub(crate) async fn api_fallback(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(path): Path<String>,
) -> Result<Json<ApiFallbackResponse>, GatewayError> {
    authorize_request(&state, &headers, true)?;

    Ok(Json(ApiFallbackResponse { ok: true, path }))
}

/// Default maximum WebSocket message size (2 MB).
/// Used as fallback for endpoints that don't have access to `GatewayState`.
pub(crate) const WS_MAX_MESSAGE_SIZE: usize = 2 * 1024 * 1024;
