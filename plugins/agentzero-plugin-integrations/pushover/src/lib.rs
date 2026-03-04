use agentzero_plugin_sdk::prelude::*;
use serde_json::json;

declare_tool!("pushover", execute);

fn execute(input: ToolInput) -> ToolOutput {
    let req: serde_json::Value = match serde_json::from_str(&input.input) {
        Ok(v) => v,
        Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
    };

    let message = match req.get("message").and_then(|v| v.as_str()) {
        Some(m) => m.trim(),
        None => return ToolOutput::error("message field is required"),
    };
    if message.is_empty() {
        return ToolOutput::error("message must not be empty");
    }

    let title = req
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let priority = req.get("priority").and_then(|v| v.as_i64()).unwrap_or(0);
    if !(-2..=2).contains(&priority) {
        return ToolOutput::error(format!(
            "priority must be between -2 and 2, got {priority}"
        ));
    }

    let token = req
        .get("token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| std::env::var("PUSHOVER_TOKEN").ok());

    let token = match token {
        Some(t) if !t.is_empty() => t,
        _ => {
            return ToolOutput::error(
                "Pushover token required: pass 'token' or set PUSHOVER_TOKEN env var",
            )
        }
    };

    let user = req
        .get("user")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| std::env::var("PUSHOVER_USER").ok());

    let user = match user {
        Some(u) if !u.is_empty() => u,
        _ => {
            return ToolOutput::error(
                "Pushover user key required: pass 'user' or set PUSHOVER_USER env var",
            )
        }
    };

    // Build the form data that would be sent to Pushover
    let mut form_fields = vec![
        ("token", mask_key(&token)),
        ("user", mask_key(&user)),
        ("message", message.to_string()),
        ("priority", priority.to_string()),
    ];
    if let Some(ref t) = title {
        form_fields.push(("title", t.clone()));
    }

    // NOTE: Actual HTTP execution requires az_http_request host function.
    // Currently returns a dry-run response showing the prepared request.
    let form_obj: serde_json::Value = form_fields
        .iter()
        .map(|(k, v)| (k.to_string(), json!(v)))
        .collect::<serde_json::Map<String, serde_json::Value>>()
        .into();

    ToolOutput::success(
        json!({
            "mode": "dry-run",
            "note": "HTTP host function not yet available; showing prepared request",
            "endpoint": "https://api.pushover.net/1/messages.json",
            "method": "POST",
            "content_type": "application/x-www-form-urlencoded",
            "form_data": form_obj,
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
