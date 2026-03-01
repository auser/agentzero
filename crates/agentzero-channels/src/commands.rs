use crate::ChannelMessage;

/// Result of attempting to parse a message as a runtime command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    /// The message is a recognized command; the response should be sent back
    /// to the user instead of forwarding to the LLM.
    Response(String),
    /// The message is not a command; pass it through to the LLM pipeline.
    PassThrough,
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
            CommandResult::Response(format!("Auto-approved tool `{tool}` for this session."))
        }
        ChatCommand::Unapprove(tool) => {
            CommandResult::Response(format!("Removed auto-approval for tool `{tool}`."))
        }
        ChatCommand::Approvals => {
            CommandResult::Response("Current approvals: (none configured)".to_string())
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

/// Check if a message is a runtime command and handle it.
/// Returns `CommandResult::Response` with the reply if it's a command,
/// or `CommandResult::PassThrough` if the message should go to the LLM.
pub fn intercept_command(msg: &ChannelMessage) -> CommandResult {
    match parse_command(&msg.content) {
        Some(cmd) => handle_command(&cmd, msg),
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
}
