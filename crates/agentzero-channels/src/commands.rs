use crate::ChannelMessage;
use agentzero_config::skills::InstalledSkill;
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
    /// `/agents` — list available agents and their status.
    Agents,
    /// `/talk <agent>` — start a conversation with a specific agent.
    Talk(String),
    /// `/thread` — show current conversation thread info.
    Thread,
    /// `/broadcast <message>` — send a message to all agents.
    Broadcast(String),
    /// A command provided by an installed skill (e.g. `/review`).
    SkillCommand {
        /// The command name without the leading `/`.
        name: String,
        /// The skill that declares this command.
        skill_name: String,
    },
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
        "/agents" => Some(ChatCommand::Agents),
        "/talk" => arg.map(ChatCommand::Talk),
        "/thread" => Some(ChatCommand::Thread),
        "/broadcast" => arg.map(ChatCommand::Broadcast),
        _ => None,
    }
}

/// Registered skill command: `(command_name, skill_name, description)`.
pub type SkillCommandEntry = (String, String, String);

/// Register commands from installed skills into the command parser.
///
/// Skills declare commands in `skill.toml` like:
/// ```toml
/// [[commands]]
/// name = "review"
/// description = "Start a code review"
/// handler = "agent"
/// ```
///
/// Returns `(command_name, skill_name, description)` triples that can be
/// passed to [`parse_command_with_skills`] for matching.
pub fn register_skill_commands(skills: &[InstalledSkill]) -> Vec<SkillCommandEntry> {
    let mut entries = Vec::new();
    for skill in skills {
        for cmd in &skill.manifest.commands {
            entries.push((
                cmd.name.clone(),
                skill.name().to_string(),
                cmd.description.clone(),
            ));
        }
    }
    entries
}

/// Try to parse a message as a command, also checking skill-provided commands.
///
/// Built-in commands take precedence. If the message is a `/` command that
/// doesn't match any built-in, the skill commands list is consulted.
pub fn parse_command_with_skills(
    text: &str,
    skill_commands: &[SkillCommandEntry],
) -> Option<ChatCommand> {
    // First try built-in commands.
    if let Some(cmd) = parse_command(text) {
        return Some(cmd);
    }

    // If it starts with `/`, check skill commands.
    let trimmed = text.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let parts: Vec<&str> = trimmed.splitn(2, char::is_whitespace).collect();
    let cmd_name = parts[0][1..].to_lowercase(); // strip leading '/'

    for (name, skill_name, _desc) in skill_commands {
        if name.to_lowercase() == cmd_name {
            return Some(ChatCommand::SkillCommand {
                name: name.clone(),
                skill_name: skill_name.clone(),
            });
        }
    }

    None
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
                let list = ctx
                    .auto_approve
                    .lock()
                    .expect("auto_approve mutex poisoned");
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
                /agents - List available agents\n\
                /talk <agent> - Start conversation with an agent\n\
                /thread - Show current thread info\n\
                /broadcast <msg> - Send to all agents\n\
                /approve <tool> - Auto-approve a tool\n\
                /unapprove <tool> - Remove auto-approval\n\
                /approvals - List current approvals\n\
                /help - Show this help"
                .to_string(),
        ),
        ChatCommand::Agents => CommandResult::Response(
            "Available agents: (requires runtime integration — use `agentzero agents list` or check agents/ directory)".to_string(),
        ),
        ChatCommand::Talk(agent) => CommandResult::Response(
            format!("Starting conversation with @{agent}. Send messages and they will be routed to this agent."),
        ),
        ChatCommand::Thread => CommandResult::Response(
            "Current thread: (no active thread — start one with /talk <agent> or @agent <message>)".to_string(),
        ),
        ChatCommand::Broadcast(message) => CommandResult::Response(
            format!("Broadcasting to all agents: \"{message}\" (requires runtime integration)"),
        ),
        ChatCommand::SkillCommand { name, skill_name } => CommandResult::Response(
            format!("Routing to skill `{skill_name}` command `/{name}`."),
        ),
    }
}

fn approve_tool(tool: &str, ctx: &CommandContext) -> CommandResult {
    let mut list = ctx
        .auto_approve
        .lock()
        .expect("auto_approve mutex poisoned");
    if list.contains(&tool.to_string()) {
        return CommandResult::Response(format!("Tool `{tool}` is already approved."));
    }
    list.push(tool.to_string());
    let persist_msg = persist_approvals(&list, ctx.config_path.as_deref());
    CommandResult::Response(format!("Auto-approved tool `{tool}`.{persist_msg}"))
}

fn unapprove_tool(tool: &str, ctx: &CommandContext) -> CommandResult {
    let mut list = ctx
        .auto_approve
        .lock()
        .expect("auto_approve mutex poisoned");
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
            privacy_boundary: String::new(),
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

    // --- Conversation command tests ---

    #[test]
    fn parse_agents_command() {
        assert_eq!(parse_command("/agents"), Some(ChatCommand::Agents));
    }

    #[test]
    fn parse_talk_command() {
        assert_eq!(
            parse_command("/talk reviewer"),
            Some(ChatCommand::Talk("reviewer".into()))
        );
    }

    #[test]
    fn parse_talk_without_agent_returns_none() {
        assert_eq!(parse_command("/talk"), None);
    }

    #[test]
    fn parse_thread_command() {
        assert_eq!(parse_command("/thread"), Some(ChatCommand::Thread));
    }

    #[test]
    fn parse_broadcast_command() {
        assert_eq!(
            parse_command("/broadcast hello everyone"),
            Some(ChatCommand::Broadcast("hello everyone".into()))
        );
    }

    #[test]
    fn handle_agents_returns_response() {
        let msg = test_msg("/agents");
        match intercept_command(&msg) {
            CommandResult::Response(text) => {
                assert!(text.contains("agents"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
    }

    #[test]
    fn handle_talk_mentions_agent_name() {
        let msg = test_msg("/talk writer");
        match intercept_command(&msg) {
            CommandResult::Response(text) => {
                assert!(text.contains("@writer"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
    }

    #[test]
    fn handle_thread_returns_info() {
        let msg = test_msg("/thread");
        match intercept_command(&msg) {
            CommandResult::Response(text) => {
                assert!(text.contains("thread"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
    }

    #[test]
    fn help_includes_conversation_commands() {
        let msg = test_msg("/help");
        match intercept_command(&msg) {
            CommandResult::Response(text) => {
                assert!(text.contains("/agents"));
                assert!(text.contains("/talk"));
                assert!(text.contains("/thread"));
                assert!(text.contains("/broadcast"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
    }

    // --- Skill-provided command tests ---

    fn test_skill_commands() -> Vec<SkillCommandEntry> {
        vec![
            (
                "review".to_string(),
                "code-reviewer".to_string(),
                "Start a code review".to_string(),
            ),
            (
                "schedule".to_string(),
                "scheduler".to_string(),
                "Schedule a task".to_string(),
            ),
        ]
    }

    #[test]
    fn skill_command_parsed_correctly() {
        let skill_cmds = test_skill_commands();
        let result = parse_command_with_skills("/review", &skill_cmds);
        assert_eq!(
            result,
            Some(ChatCommand::SkillCommand {
                name: "review".to_string(),
                skill_name: "code-reviewer".to_string(),
            })
        );
    }

    #[test]
    fn skill_command_case_insensitive() {
        let skill_cmds = test_skill_commands();
        let result = parse_command_with_skills("/REVIEW", &skill_cmds);
        assert_eq!(
            result,
            Some(ChatCommand::SkillCommand {
                name: "review".to_string(),
                skill_name: "code-reviewer".to_string(),
            })
        );
    }

    #[test]
    fn unknown_skill_command_falls_through_to_none() {
        let skill_cmds = test_skill_commands();
        let result = parse_command_with_skills("/unknown-cmd", &skill_cmds);
        assert_eq!(result, None);
    }

    #[test]
    fn builtin_command_takes_precedence_over_skill() {
        // Even if a skill declares "help", the built-in /help wins.
        let skill_cmds = vec![(
            "help".to_string(),
            "my-skill".to_string(),
            "Skill help".to_string(),
        )];
        let result = parse_command_with_skills("/help", &skill_cmds);
        assert_eq!(result, Some(ChatCommand::Help));
    }

    #[test]
    fn non_command_returns_none_with_skills() {
        let skill_cmds = test_skill_commands();
        let result = parse_command_with_skills("hello world", &skill_cmds);
        assert_eq!(result, None);
    }

    #[test]
    fn register_skill_commands_extracts_entries() {
        use agentzero_config::skills::{
            InstalledSkill, SkillCommand as SkillCmd, SkillManifest, SkillMeta,
        };

        let skill = InstalledSkill {
            manifest: SkillManifest {
                skill: SkillMeta {
                    name: "test-skill".to_string(),
                    version: "1.0.0".to_string(),
                    description: String::new(),
                    author: String::new(),
                    keywords: vec![],
                    requires: vec![],
                    provides: vec![],
                },
                channel: None,
                tools: vec![],
                commands: vec![
                    SkillCmd {
                        name: "deploy".to_string(),
                        description: "Deploy the app".to_string(),
                        handler: "agent".to_string(),
                        target: None,
                    },
                    SkillCmd {
                        name: "rollback".to_string(),
                        description: "Rollback last deploy".to_string(),
                        handler: "tool".to_string(),
                        target: Some("rollback_tool".to_string()),
                    },
                ],
                workflow: None,
                dependencies: vec![],
            },
            dir: std::path::PathBuf::from("/tmp/test"),
            source: "project".to_string(),
            agent_prompt: None,
            agent_frontmatter: None,
            config_fragment: None,
        };

        let entries = register_skill_commands(&[skill]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "deploy");
        assert_eq!(entries[0].1, "test-skill");
        assert_eq!(entries[0].2, "Deploy the app");
        assert_eq!(entries[1].0, "rollback");
        assert_eq!(entries[1].1, "test-skill");
    }

    #[test]
    fn skill_command_handle_returns_routing_response() {
        let cmd = ChatCommand::SkillCommand {
            name: "review".to_string(),
            skill_name: "code-reviewer".to_string(),
        };
        let msg = test_msg("/review");
        match handle_command(&cmd, &msg) {
            CommandResult::Response(text) => {
                assert!(text.contains("code-reviewer"));
                assert!(text.contains("/review"));
            }
            CommandResult::PassThrough => panic!("expected Response"),
        }
    }
}
