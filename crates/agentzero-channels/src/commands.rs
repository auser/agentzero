use crate::ChannelMessage;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Result of attempting to parse a message as a runtime command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    /// The message is a recognized command; the response should be sent back
    /// to the user instead of forwarding to the LLM.
    Response(String),
    /// The message is not a command; pass it through to the LLM pipeline.
    PassThrough,
}

/// Runtime context for command execution.
///
/// Carries mutable state (e.g. approval list) and the config path so that
/// commands like `/approve` can persist changes to disk.
#[derive(Clone)]
pub struct CommandContext {
    pub auto_approve: Arc<Mutex<Vec<String>>>,
    pub config_path: Option<PathBuf>,
}

/// Parsed in-chat command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatCommand {
    /// `/models` or `/models <provider>` — list available models.
    Models(Option<String>),
    /// `/model` — show current model; `/model <id>` — switch model.
    Model(Option<String>),
    /// `/new` — clear conversation history for this sender.
    New,
    /// `/approve <tool>` — auto-approve a tool for this session.
    Approve(String),
    /// `/unapprove <tool>` — remove auto-approval.
    Unapprove(String),
    /// `/approvals` — list current approvals.
    Approvals,
    /// `/approve-request <tool>` — request approval for a tool.
    ApproveRequest(String),
    /// `/approve-confirm <id>` — confirm a pending approval request.
    ApproveConfirm(String),
    /// `/approve-pending` — list pending approval requests.
    ApprovePending,
    /// `/help` — show available commands.
    Help,
}

/// Try to parse a message as a runtime command.
/// Returns `None` if the message is not a command.
pub fn parse_command(text: &str) -> Option<ChatCommand> {
    let trimmed = text.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let parts: Vec<&str> = trimmed.splitn(2, char::is_whitespace).collect();
    let cmd = parts[0].to_lowercase();
    let arg = parts.get(1).map(|s| s.trim().to_string());

    match cmd.as_str() {
        "/models" => Some(ChatCommand::Models(arg)),
        "/model" => Some(ChatCommand::Model(arg)),
        "/new" => Some(ChatCommand::New),
        "/approve" => arg.map(ChatCommand::Approve),
        "/unapprove" => arg.map(ChatCommand::Unapprove),
        "/approvals" => Some(ChatCommand::Approvals),
        "/approve-request" => arg.map(ChatCommand::ApproveRequest),
        "/approve-confirm" => arg.map(ChatCommand::ApproveConfirm),
        "/approve-pending" => Some(ChatCommand::ApprovePending),
        "/help" => Some(ChatCommand::Help),
        _ => None,
    }
}

/// Handle a parsed command and produce a response string.
/// This provides default responses; the runtime can override with richer behavior.
pub fn handle_command(cmd: &ChatCommand, _msg: &ChannelMessage) -> CommandResult {
    handle_command_with_context(cmd, _msg, None)
}

/// Handle a parsed command with optional runtime context for stateful operations.
pub fn handle_command_with_context(
    cmd: &ChatCommand,
    _msg: &ChannelMessage,
    ctx: Option<&CommandContext>,
) -> CommandResult {
    match cmd {
        ChatCommand::Models(provider) => {
            let response = if let Some(p) = provider {
                format!("Listing models for provider `{p}`. (Requires runtime integration)")
            } else {
                "Available providers: openrouter, openai, anthropic, ollama. Use `/models <provider>` to list models.".to_string()
            };
            CommandResult::Response(response)
        }
        ChatCommand::Model(id) => {
            let response = if let Some(model_id) = id {
                format!("Switching model to `{model_id}` for this session.")
            } else {
                "Current model: (default from config). Use `/model <id>` to switch.".to_string()
            };
            CommandResult::Response(response)
        }
        ChatCommand::New => CommandResult::Response("Conversation history cleared.".to_string()),
        ChatCommand::Approve(tool) => {
            if let Some(ctx) = ctx {
                approve_tool(tool, ctx)
            } else {
                CommandResult::Response(format!("Auto-approved tool `{tool}` for this session."))
            }
        }
        ChatCommand::Unapprove(tool) => {
            if let Some(ctx) = ctx {
                unapprove_tool(tool, ctx)
            } else {
                CommandResult::Response(format!("Removed auto-approval for tool `{tool}`."))
            }
        }
        ChatCommand::Approvals => {
            if let Some(ctx) = ctx {
                let list = ctx.auto_approve.lock().unwrap();
                if list.is_empty() {
                    CommandResult::Response("Current approvals: (none)".to_string())
                } else {
                    CommandResult::Response(format!("Current approvals: {}", list.join(", ")))
                }
            } else {
                CommandResult::Response("Current approvals: (none configured)".to_string())
            }
        }
        ChatCommand::ApproveRequest(tool) => {
            CommandResult::Response(format!("Approval requested for tool `{tool}`."))
        }
        ChatCommand::ApproveConfirm(id) => {
            CommandResult::Response(format!("Approval confirmed for request `{id}`."))
        }
        ChatCommand::ApprovePending => {
            CommandResult::Response("Pending approvals: (none)".to_string())
        }
        ChatCommand::Help => CommandResult::Response(
            "Available commands:\n\
                /models [provider] - List available models\n\
                /model [id] - Show or switch model\n\
                /new - Clear conversation history\n\
                /approve <tool> - Auto-approve a tool\n\
                /unapprove <tool> - Remove auto-approval\n\
                /approvals - List current approvals\n\
                /approve-request <tool> - Request tool approval\n\
                /approve-confirm <id> - Confirm pending approval\n\
                /approve-pending - List pending approvals\n\
                /help - Show this help"
                .to_string(),
        ),
    }
}

fn approve_tool(tool: &str, ctx: &CommandContext) -> CommandResult {
    let mut list = ctx.auto_approve.lock().unwrap();
    if list.contains(&tool.to_string()) {
        return CommandResult::Response(format!("Tool `{tool}` is already approved."));
    }
    list.push(tool.to_string());
    let persist_msg = persist_approvals(&list, ctx.config_path.as_deref());
    CommandResult::Response(format!("Auto-approved tool `{tool}`.{persist_msg}"))
}

fn unapprove_tool(tool: &str, ctx: &CommandContext) -> CommandResult {
    let mut list = ctx.auto_approve.lock().unwrap();
    let before = list.len();
    list.retain(|t| t != tool);
    if list.len() == before {
        return CommandResult::Response(format!("Tool `{tool}` was not in the approval list."));
    }
    let persist_msg = persist_approvals(&list, ctx.config_path.as_deref());
    CommandResult::Response(format!(
        "Removed auto-approval for tool `{tool}`.{persist_msg}"
    ))
}

fn persist_approvals(tools: &[String], config_path: Option<&Path>) -> String {
    let Some(path) = config_path else {
        return String::new();
    };
    match agentzero_config::update_auto_approve(path, tools) {
        Ok(()) => " Saved to config.".to_string(),
        Err(e) => format!(" (warning: failed to persist: {e})"),
    }
}

/// Check if a message is a runtime command and handle it.
/// Returns `CommandResult::Response` with the reply if it's a command,
/// or `CommandResult::PassThrough` if the message should go to the LLM.
pub fn intercept_command(msg: &ChannelMessage) -> CommandResult {
    match parse_command(&msg.content) {
        Some(cmd) => handle_command(&cmd, msg),
        None => CommandResult::PassThrough,
    }
}

/// Check if a message is a runtime command and handle it with context.
pub fn intercept_command_with_context(msg: &ChannelMessage, ctx: &CommandContext) -> CommandResult {
    match parse_command(&msg.content) {
        Some(cmd) => handle_command_with_context(&cmd, msg, Some(ctx)),
        None => CommandResult::PassThrough,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_msg(content: &str) -> ChannelMessage {
        ChannelMessage {
            id: "1".into(),
            sender: "alice".into(),
            reply_target: "alice".into(),
            content: content.into(),
            channel: "test".into(),
            timestamp: 0,
            thread_ts: None,
        }
    }

    #[test]
    fn parse_models_no_arg() {
        assert_eq!(parse_command("/models"), Some(ChatCommand::Models(None)));
    }

    #[test]
    fn parse_models_with_provider() {
        assert_eq!(
            parse_command("/models openai"),
            Some(ChatCommand::Models(Some("openai".into())))
        );
    }

    #[test]
    fn parse_model_no_arg() {
        assert_eq!(parse_command("/model"), Some(ChatCommand::Model(None)));
    }

    #[test]
    fn parse_model_with_id() {
        assert_eq!(
            parse_command("/model gpt-4o"),
            Some(ChatCommand::Model(Some("gpt-4o".into())))
        );
    }

    #[test]
    fn parse_new() {
        assert_eq!(parse_command("/new"), Some(ChatCommand::New));
    }

    #[test]
    fn parse_approve_with_tool() {
        assert_eq!(
            parse_command("/approve shell"),
            Some(ChatCommand::Approve("shell".into()))
        );
    }

    #[test]
    fn parse_approve_without_tool_returns_none() {
        assert_eq!(parse_command("/approve"), None);
    }

    #[test]
    fn parse_unapprove() {
        assert_eq!(
            parse_command("/unapprove shell"),
            Some(ChatCommand::Unapprove("shell".into()))
        );
    }

    #[test]
    fn parse_approvals() {
        assert_eq!(parse_command("/approvals"), Some(ChatCommand::Approvals));
    }

    #[test]
    fn parse_approve_pending() {
        assert_eq!(
            parse_command("/approve-pending"),
            Some(ChatCommand::ApprovePending)
        );
    }

    #[test]
    fn parse_approve_confirm() {
        assert_eq!(
            parse_command("/approve-confirm req-123"),
            Some(ChatCommand::ApproveConfirm("req-123".into()))
        );
    }

    #[test]
    fn parse_help() {
        assert_eq!(parse_command("/help"), Some(ChatCommand::Help));
    }

    #[test]
    fn parse_unknown_command_returns_none() {
        assert_eq!(parse_command("/foobar"), None);
    }

    #[test]
    fn parse_non_command_returns_none() {
        assert_eq!(parse_command("hello world"), None);
        assert_eq!(parse_command(""), None);
    }

    #[test]
    fn intercept_command_handles_known_command() {
        let msg = test_msg("/help");
        match intercept_command(&msg) {
            CommandResult::Response(text) => {
                assert!(text.contains("Available commands"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
    }

    #[test]
    fn intercept_command_passes_through_non_command() {
        let msg = test_msg("hello");
        assert_eq!(intercept_command(&msg), CommandResult::PassThrough);
    }

    #[test]
    fn case_insensitive_commands() {
        assert_eq!(parse_command("/MODELS"), Some(ChatCommand::Models(None)));
        assert_eq!(parse_command("/Help"), Some(ChatCommand::Help));
        assert_eq!(parse_command("/NEW"), Some(ChatCommand::New));
    }

    #[test]
    fn handle_model_switch_response() {
        let cmd = ChatCommand::Model(Some("claude-3-opus".into()));
        let msg = test_msg("/model claude-3-opus");
        match handle_command(&cmd, &msg) {
            CommandResult::Response(text) => {
                assert!(text.contains("claude-3-opus"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
    }

    #[test]
    fn handle_new_clears_history() {
        let msg = test_msg("/new");
        match handle_command(&ChatCommand::New, &msg) {
            CommandResult::Response(text) => {
                assert!(text.contains("cleared"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
    }

    fn test_ctx() -> CommandContext {
        CommandContext {
            auto_approve: Arc::new(Mutex::new(Vec::new())),
            config_path: None,
        }
    }

    #[test]
    fn approve_adds_tool_to_list() {
        let ctx = test_ctx();
        let msg = test_msg("/approve shell");
        match intercept_command_with_context(&msg, &ctx) {
            CommandResult::Response(text) => {
                assert!(text.contains("Auto-approved tool `shell`"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
        let list = ctx.auto_approve.lock().unwrap();
        assert_eq!(*list, vec!["shell".to_string()]);
    }

    #[test]
    fn approve_duplicate_rejected() {
        let ctx = test_ctx();
        ctx.auto_approve.lock().unwrap().push("shell".to_string());
        let msg = test_msg("/approve shell");
        match intercept_command_with_context(&msg, &ctx) {
            CommandResult::Response(text) => {
                assert!(text.contains("already approved"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
    }

    #[test]
    fn unapprove_removes_tool() {
        let ctx = test_ctx();
        ctx.auto_approve.lock().unwrap().push("shell".to_string());
        let msg = test_msg("/unapprove shell");
        match intercept_command_with_context(&msg, &ctx) {
            CommandResult::Response(text) => {
                assert!(text.contains("Removed auto-approval"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
        let list = ctx.auto_approve.lock().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn unapprove_missing_tool_reports_not_found() {
        let ctx = test_ctx();
        let msg = test_msg("/unapprove shell");
        match intercept_command_with_context(&msg, &ctx) {
            CommandResult::Response(text) => {
                assert!(text.contains("was not in the approval list"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
    }

    #[test]
    fn approvals_lists_current() {
        let ctx = test_ctx();
        {
            let mut list = ctx.auto_approve.lock().unwrap();
            list.push("shell".to_string());
            list.push("browser".to_string());
        }
        let msg = test_msg("/approvals");
        match intercept_command_with_context(&msg, &ctx) {
            CommandResult::Response(text) => {
                assert!(text.contains("shell"));
                assert!(text.contains("browser"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
    }

    #[test]
    fn approvals_empty_shows_none() {
        let ctx = test_ctx();
        let msg = test_msg("/approvals");
        match intercept_command_with_context(&msg, &ctx) {
            CommandResult::Response(text) => {
                assert!(text.contains("(none)"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
    }

    #[test]
    fn approve_persists_to_disk() {
        let dir = std::env::temp_dir().join("agentzero-test-approve");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        std::fs::write(&path, "").unwrap();

        let ctx = CommandContext {
            auto_approve: Arc::new(Mutex::new(Vec::new())),
            config_path: Some(path.clone()),
        };

        let msg = test_msg("/approve shell");
        let result = intercept_command_with_context(&msg, &ctx);
        match result {
            CommandResult::Response(text) => {
                assert!(text.contains("Saved to config"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("auto_approve"));
        assert!(content.contains("shell"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
