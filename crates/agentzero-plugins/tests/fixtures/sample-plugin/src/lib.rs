use agentzero_plugin_sdk::prelude::*;

declare_tool!("sample_plugin", execute);

fn execute(input: ToolInput) -> ToolOutput {
    let req: serde_json::Value = match serde_json::from_str(&input.input) {
        Ok(v) => v,
        Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
    };

    let name = req["name"].as_str().unwrap_or("world");
    ToolOutput::success(format!("Hello, {name}! workspace={}", input.workspace_root))
}
