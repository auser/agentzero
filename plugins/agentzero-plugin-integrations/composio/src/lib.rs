use agentzero_plugin_sdk::prelude::*;
use serde_json::json;

declare_tool!("composio", execute);

fn execute(input: ToolInput) -> ToolOutput {
    let req: serde_json::Value = match serde_json::from_str(&input.input) {
        Ok(v) => v,
        Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
    };

    let action = match req.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.trim(),
        None => return ToolOutput::error("action field is required"),
    };
    if action.is_empty() {
        return ToolOutput::error("action must not be empty");
    }

    let params = req.get("params").cloned().unwrap_or(json!({}));

    let api_key = req
        .get("api_key")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| std::env::var("COMPOSIO_API_KEY").ok());

    let api_key = match api_key {
        Some(k) if !k.is_empty() => k,
        _ => {
            return ToolOutput::error(
                "Composio API key required: pass 'api_key' or set COMPOSIO_API_KEY env var",
            )
        }
    };

    // Build the request that would be sent to Composio
    let request_body = json!({
        "action": action,
        "params": params,
    });

    // NOTE: Actual HTTP execution requires az_http_request host function.
    // Currently returns a dry-run response showing the prepared request.
    ToolOutput::success(
        json!({
            "mode": "dry-run",
            "note": "HTTP host function not yet available; showing prepared request",
            "endpoint": "https://backend.composio.dev/api/v1/actions/execute",
            "method": "POST",
            "headers": {
                "x-api-key": mask_key(&api_key),
                "content-type": "application/json"
            },
            "body": request_body,
        })
        .to_string(),
    )
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        "****".to_string()
    } else {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    }
}
