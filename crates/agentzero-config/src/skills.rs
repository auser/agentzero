//! Skill and workflow pack registry.
//!
//! Skills are the universal extension mechanism for AgentZero. A skill can
//! provide any combination of: agents, tools, channels, `/` commands, and
//! config fragments. Skills live in directory-based packages with a
//! `skill.toml` manifest.
//!
//! **Workflow packs** are the primary unit of distribution in the marketplace.
//! A pack delivers a complete coordination graph — agents, skills, tools,
//! channels, cron schedules, and orchestration wiring — bundled as one
//! installable unit. Packs are self-contained, composable, overridable,
//! versioned, and sandboxed.
//!
//! Discovery order (project-local wins):
//! 1. `$PWD/.agentzero/skills/`
//! 2. `~/.agentzero/skills/`
//! 3. Built-in templates (embedded in binary)

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ─── Manifest types ──────────────────────────────────────────────────────────

/// Parsed `skill.toml` manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub skill: SkillMeta,
    #[serde(default)]
    pub channel: Option<SkillChannel>,
    #[serde(default)]
    pub tools: Vec<SkillToolDef>,
    #[serde(default)]
    pub commands: Vec<SkillCommand>,
    /// Workflow pack coordination graph (optional — present for packs).
    #[serde(default)]
    pub workflow: Option<WorkflowDef>,
    /// Sub-pack dependencies (other packs this one composes).
    #[serde(default)]
    pub dependencies: Vec<PackDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub provides: Vec<String>,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

/// Channel provided by a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillChannel {
    pub name: String,
    /// `builtin` (use compiled-in impl), `wasm`, `script`, or `http`.
    #[serde(rename = "type", default = "default_extension_type")]
    pub kind: String,
    /// Path to WASM/script, or HTTP endpoint URL.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
}

/// Tool provided by a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillToolDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// `wasm`, `script`, or `http`.
    #[serde(rename = "type", default = "default_extension_type")]
    pub kind: String,
    /// Path relative to skill directory (for wasm/script).
    #[serde(default)]
    pub path: Option<String>,
    /// HTTP endpoint (for http bridge).
    #[serde(default)]
    pub endpoint: Option<String>,
}

fn default_extension_type() -> String {
    "builtin".to_string()
}

/// Slash command provided by a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCommand {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// `agent` (route to skill's agent) or `tool` (invoke a tool).
    #[serde(default = "default_command_handler")]
    pub handler: String,
    /// Tool name or agent name to invoke.
    #[serde(default)]
    pub target: Option<String>,
}

fn default_command_handler() -> String {
    "agent".to_string()
}

// ─── Workflow pack types ─────────────────────────────────────────────────────

/// Coordination graph definition for a workflow pack.
///
/// A workflow pack is a runnable workflow definition — a first-class,
/// shareable, installable coordination graph with all dependencies resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    /// Human-readable name for this workflow.
    #[serde(default)]
    pub name: String,
    /// Workflow description.
    #[serde(default)]
    pub description: String,
    /// Nodes in the coordination graph (agents, skills, tools, decision points).
    #[serde(default)]
    pub nodes: Vec<WorkflowNode>,
    /// Edges connecting nodes (data flow, delegation, conditional branching).
    #[serde(default)]
    pub edges: Vec<WorkflowEdge>,
    /// Entry points into the workflow.
    #[serde(default)]
    pub entry_points: Vec<WorkflowEntryPoint>,
    /// Cron schedules that trigger the workflow.
    #[serde(default)]
    pub cron: Vec<WorkflowCron>,
}

/// A node in the coordination graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNode {
    /// Unique node ID within this workflow.
    pub id: String,
    /// Node type: "agent", "skill", "tool", "decision", "transform".
    #[serde(rename = "type")]
    pub kind: String,
    /// Reference to the agent/skill/tool name.
    #[serde(default)]
    pub ref_name: Option<String>,
    /// Node-specific configuration.
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

/// An edge connecting two nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEdge {
    /// Source node ID.
    pub from: String,
    /// Target node ID.
    pub to: String,
    /// Edge type: "delegate", "data", "conditional".
    #[serde(rename = "type", default = "default_edge_type")]
    pub kind: String,
    /// Condition expression (for conditional edges).
    #[serde(default)]
    pub condition: Option<String>,
}

fn default_edge_type() -> String {
    "delegate".to_string()
}

/// Entry point into the workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEntryPoint {
    /// Trigger type: "user", "cron", "webhook", "event".
    #[serde(rename = "type")]
    pub kind: String,
    /// Target node ID to start execution.
    pub target: String,
    /// Pattern for matching (e.g., event topic, webhook path).
    #[serde(default)]
    pub pattern: Option<String>,
}

/// Cron schedule for a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCron {
    /// Cron expression (e.g., "0 9 * * 1-5").
    pub schedule: String,
    /// Target node ID to trigger.
    pub target: String,
    /// Description of what this schedule does.
    #[serde(default)]
    pub description: String,
}

/// Dependency on another pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackDependency {
    /// Name of the required pack.
    pub name: String,
    /// Semver version requirement (e.g., ">=1.0.0").
    #[serde(default)]
    pub version: Option<String>,
    /// Source to install from if not already present.
    #[serde(default)]
    pub source: Option<String>,
}

// ─── Installed skill (manifest + paths) ──────────────────────────────────────

/// A discovered/installed skill with resolved paths.
#[derive(Debug, Clone)]
pub struct InstalledSkill {
    pub manifest: SkillManifest,
    /// Root directory of this skill package.
    pub dir: PathBuf,
    /// Where it was found: "project", "global", or "builtin".
    pub source: String,
    /// Parsed AGENT.md content (system prompt), if present.
    pub agent_prompt: Option<String>,
    /// Parsed AGENT.md frontmatter fields, if present.
    pub agent_frontmatter: Option<HashMap<String, serde_json::Value>>,
    /// Parsed config.toml fragment, if present.
    pub config_fragment: Option<toml::Value>,
}

impl InstalledSkill {
    /// Skill name from manifest.
    pub fn name(&self) -> &str {
        &self.manifest.skill.name
    }
}

// ─── Discovery ───────────────────────────────────────────────────────────────

/// Discover all installed skills, in precedence order.
///
/// Project-local skills override global skills with the same name.
pub fn discover_skills(
    project_dir: Option<&Path>,
    global_dir: Option<&Path>,
) -> Vec<InstalledSkill> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut skills = Vec::new();

    // 1. Project-local: $PWD/.agentzero/skills/
    if let Some(project) = project_dir {
        let skills_dir = project.join(".agentzero").join("skills");
        if skills_dir.is_dir() {
            discover_from_dir(&skills_dir, "project", &mut skills, &mut seen);
        }
    }

    // 2. Global: ~/.agentzero/skills/
    if let Some(global) = global_dir {
        let skills_dir = global.join("skills");
        if skills_dir.is_dir() {
            discover_from_dir(&skills_dir, "global", &mut skills, &mut seen);
        }
    }

    skills
}

fn discover_from_dir(
    skills_dir: &Path,
    source: &str,
    skills: &mut Vec<InstalledSkill>,
    seen: &mut HashMap<String, usize>,
) {
    let entries = match std::fs::read_dir(skills_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        match load_skill_from_dir(&path, source) {
            Ok(skill) => {
                let name = skill.name().to_string();
                if let Some(&idx) = seen.get(&name) {
                    // Higher-precedence source already loaded; skip.
                    tracing::debug!(
                        skill = name,
                        source = source,
                        existing_source = skills[idx].source,
                        "skipping duplicate skill (higher-precedence already loaded)"
                    );
                } else {
                    seen.insert(name, skills.len());
                    skills.push(skill);
                }
            }
            Err(e) => {
                tracing::warn!(
                    dir = %path.display(),
                    error = %e,
                    "failed to load skill"
                );
            }
        }
    }
}

/// Load a single skill from its directory.
pub fn load_skill_from_dir(dir: &Path, source: &str) -> anyhow::Result<InstalledSkill> {
    let manifest_path = dir.join("skill.toml");
    if !manifest_path.exists() {
        bail!("no skill.toml found in {}", dir.display());
    }

    let manifest_str = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest: SkillManifest = toml::from_str(&manifest_str)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;

    // Parse optional AGENT.md
    let agent_md_path = dir.join("AGENT.md");
    let (agent_prompt, agent_frontmatter) = if agent_md_path.exists() {
        parse_agent_md(&agent_md_path)?
    } else {
        (None, None)
    };

    // Parse optional config.toml fragment
    let config_path = dir.join("config.toml");
    let config_fragment = if config_path.exists() {
        let s = std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        Some(
            s.parse::<toml::Value>()
                .with_context(|| format!("failed to parse {}", config_path.display()))?,
        )
    } else {
        None
    };

    Ok(InstalledSkill {
        manifest,
        dir: dir.to_path_buf(),
        source: source.to_string(),
        agent_prompt,
        agent_frontmatter,
        config_fragment,
    })
}

// ─── AGENT.md parsing ────────────────────────────────────────────────────────

/// Frontmatter key-value map from an AGENT.md file.
type Frontmatter = HashMap<String, serde_json::Value>;

/// Parsed result: (system prompt body, frontmatter map).
type AgentMdParsed = (Option<String>, Option<Frontmatter>);

/// Parse an AGENT.md file into frontmatter + body.
///
/// Format:
/// ```markdown
/// ---
/// name: reviewer
/// model: claude-sonnet-4-6
/// ---
/// System prompt body here.
/// ```
fn parse_agent_md(path: &Path) -> anyhow::Result<AgentMdParsed> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let trimmed = content.trim();
    if !trimmed.starts_with("---") {
        // No frontmatter, entire file is the prompt.
        return Ok((Some(content), None));
    }

    // Find closing ---
    let after_first = &trimmed[3..];
    if let Some(end_idx) = after_first.find("---") {
        let frontmatter_str = &after_first[..end_idx].trim();
        let body = after_first[end_idx + 3..].trim();

        // Parse YAML-style frontmatter as simple key: value pairs
        let frontmatter = parse_simple_frontmatter(frontmatter_str);

        let prompt = if body.is_empty() {
            None
        } else {
            Some(body.to_string())
        };
        Ok((prompt, Some(frontmatter)))
    } else {
        // Malformed frontmatter, treat as plain content
        Ok((Some(content), None))
    }
}

/// Parse simple YAML-like frontmatter (key: value pairs, one per line).
/// Supports string values, arrays (YAML flow syntax), and numbers.
fn parse_simple_frontmatter(s: &str) -> HashMap<String, serde_json::Value> {
    let mut map = HashMap::new();

    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_string();
            let value = value.trim();

            let parsed = if value.starts_with('[') && value.ends_with(']') {
                // Array: [item1, item2, ...]
                let inner = &value[1..value.len() - 1];
                let items: Vec<serde_json::Value> = inner
                    .split(',')
                    .map(|s| serde_json::Value::String(s.trim().trim_matches('"').to_string()))
                    .collect();
                serde_json::Value::Array(items)
            } else if value == "true" {
                serde_json::Value::Bool(true)
            } else if value == "false" {
                serde_json::Value::Bool(false)
            } else if let Ok(n) = value.parse::<f64>() {
                serde_json::json!(n)
            } else {
                // Plain string (strip quotes if present)
                serde_json::Value::String(value.trim_matches('"').to_string())
            };

            map.insert(key, parsed);
        }
    }

    map
}

// ─── Install / Remove ────────────────────────────────────────────────────────

/// Source for installing a skill.
#[derive(Debug, Clone)]
pub enum SkillSource {
    /// Name of a built-in skill template.
    BuiltIn(String),
    /// Git repository URL.
    GitUrl(String),
    /// Local directory path.
    LocalPath(PathBuf),
}

impl SkillSource {
    /// Parse a skill source string. Returns `BuiltIn` for bare names,
    /// `GitUrl` for URLs, `LocalPath` for paths.
    pub fn parse(s: &str) -> Self {
        if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("git@") {
            Self::GitUrl(s.to_string())
        } else if s.contains('/') || s.contains('\\') || s.starts_with('.') {
            Self::LocalPath(PathBuf::from(s))
        } else {
            Self::BuiltIn(s.to_string())
        }
    }
}

/// Install a skill from the given source into the target directory.
pub fn install_skill(source: &SkillSource, target_dir: &Path) -> anyhow::Result<InstalledSkill> {
    match source {
        SkillSource::BuiltIn(name) => install_builtin(name, target_dir),
        SkillSource::LocalPath(path) => install_from_local(path, target_dir),
        SkillSource::GitUrl(url) => install_from_git(url, target_dir),
    }
}

/// Remove an installed skill by name from the target directory.
pub fn remove_skill(name: &str, target_dir: &Path) -> anyhow::Result<()> {
    let skill_dir = target_dir.join("skills").join(name);
    if !skill_dir.exists() {
        bail!(
            "skill `{name}` is not installed in {}",
            target_dir.display()
        );
    }
    std::fs::remove_dir_all(&skill_dir)
        .with_context(|| format!("failed to remove {}", skill_dir.display()))?;
    Ok(())
}

fn install_from_local(source: &Path, target_dir: &Path) -> anyhow::Result<InstalledSkill> {
    let source = source
        .canonicalize()
        .with_context(|| format!("skill source path not found: {}", source.display()))?;

    // Verify it has a skill.toml
    if !source.join("skill.toml").exists() {
        bail!("no skill.toml found in {}", source.display());
    }

    // Load to get name
    let skill = load_skill_from_dir(&source, "local")?;
    let name = skill.name().to_string();

    let dest = target_dir.join("skills").join(&name);
    if dest.exists() {
        bail!("skill `{name}` already installed at {}", dest.display());
    }

    // Copy directory
    copy_dir_recursive(&source, &dest)?;
    load_skill_from_dir(&dest, "local")
}

fn install_from_git(url: &str, target_dir: &Path) -> anyhow::Result<InstalledSkill> {
    // Clone to a temp dir first, then move
    let temp = std::env::temp_dir().join(format!("agentzero-skill-clone-{}", std::process::id()));
    if temp.exists() {
        std::fs::remove_dir_all(&temp)?;
    }

    let status = std::process::Command::new("git")
        .args(["clone", "--depth", "1", url, &temp.to_string_lossy()])
        .status()
        .context("failed to run git clone")?;

    if !status.success() {
        bail!("git clone failed for {url}");
    }

    let result = install_from_local(&temp, target_dir);

    // Cleanup temp
    let _ = std::fs::remove_dir_all(&temp);

    result
}

fn install_builtin(name: &str, target_dir: &Path) -> anyhow::Result<InstalledSkill> {
    let template = get_builtin_template(name)?;
    let dest = target_dir.join("skills").join(name);

    if dest.exists() {
        bail!("skill `{name}` already installed at {}", dest.display());
    }

    std::fs::create_dir_all(&dest)
        .with_context(|| format!("failed to create {}", dest.display()))?;

    // Write skill.toml
    std::fs::write(dest.join("skill.toml"), &template.skill_toml)?;

    // Write AGENT.md if present
    if let Some(agent_md) = &template.agent_md {
        std::fs::write(dest.join("AGENT.md"), agent_md)?;
    }

    // Write config.toml if present
    if let Some(config_toml) = &template.config_toml {
        std::fs::write(dest.join("config.toml"), config_toml)?;
    }

    // Write README.md if present
    if let Some(readme) = &template.readme {
        std::fs::write(dest.join("README.md"), readme)?;
    }

    load_skill_from_dir(&dest, "builtin")
}

// ─── Built-in templates ──────────────────────────────────────────────────────

struct BuiltinTemplate {
    skill_toml: String,
    agent_md: Option<String>,
    config_toml: Option<String>,
    readme: Option<String>,
}

/// List available built-in skill template names.
pub fn list_builtin_skills() -> Vec<&'static str> {
    vec![
        "code-reviewer",
        "scheduler",
        "research-assistant",
        "telegram-bot",
        "discord-bot",
        "slack-bot",
        "devops-monitor",
    ]
}

fn get_builtin_template(name: &str) -> anyhow::Result<BuiltinTemplate> {
    match name {
        "code-reviewer" => Ok(BuiltinTemplate {
            skill_toml: r#"[skill]
name = "code-reviewer"
version = "0.1.0"
description = "AI code reviewer that analyzes diffs for bugs, security issues, and style"
keywords = ["code", "review", "git", "security"]
provides = ["agent"]
"#.to_string(),
            agent_md: Some(r#"---
name: code-reviewer
---

You are a senior code reviewer. When asked to review code, analyze it thoroughly for:

1. **Bugs**: Logic errors, off-by-one errors, null/None handling, race conditions
2. **Security**: Injection vulnerabilities, auth issues, data exposure, OWASP top 10
3. **Style**: Naming conventions, code organization, unnecessary complexity
4. **Performance**: N+1 queries, unnecessary allocations, missing caching opportunities

Always cite specific line numbers. Provide concrete fix suggestions, not vague advice.
When reviewing a PR or diff, use git and file reading tools to understand full context.
"#.to_string()),
            config_toml: None,
            readme: Some("# Code Reviewer Skill\n\nAI-powered code review agent.\n\n## Usage\n\n```\nagentzero run \"@code-reviewer review the last commit\"\n```\n".to_string()),
        }),
        "scheduler" => Ok(BuiltinTemplate {
            skill_toml: r#"[skill]
name = "scheduler"
version = "0.1.0"
description = "Natural language task scheduling -- converts plain English to cron jobs"
keywords = ["cron", "schedule", "automation"]
provides = ["agent"]
"#.to_string(),
            agent_md: Some(r#"---
name: scheduler
---

You are a scheduling assistant. When users describe tasks they want to run on a schedule, convert their natural language into cron expressions and create the scheduled tasks.

Examples:
- "Send me a summary every Monday at 9am" -> cron expression `0 9 * * 1`
- "Check disk space every 6 hours" -> cron expression `0 */6 * * *`
- "Run tests at midnight on weekdays" -> cron expression `0 0 * * 1-5`

Use the cron tools to create, list, and manage scheduled tasks. Always confirm the schedule with the user before creating it.
"#.to_string()),
            config_toml: None,
            readme: Some("# Scheduler Skill\n\nNatural language task scheduling.\n\n## Usage\n\n```\nagentzero run \"@scheduler remind me to check logs every weekday at 9am\"\n```\n".to_string()),
        }),
        "research-assistant" => Ok(BuiltinTemplate {
            skill_toml: r#"[skill]
name = "research-assistant"
version = "0.1.0"
description = "Web research agent that searches, summarizes, and synthesizes information"
keywords = ["research", "web", "search", "summary"]
provides = ["agent"]
"#.to_string(),
            agent_md: Some(r#"---
name: research-assistant
---

You are a research assistant. When given a topic or question:

1. Search the web for relevant, authoritative sources
2. Read and analyze the content of top results
3. Synthesize findings into a clear, structured summary
4. Cite your sources with URLs
5. Identify areas of consensus and disagreement
6. Flag any information that seems outdated or unreliable

Always provide balanced perspectives. Distinguish between facts and opinions.
When the user asks follow-up questions, build on previous research context.
"#.to_string()),
            config_toml: None,
            readme: Some("# Research Assistant Skill\n\nWeb research and synthesis agent.\n\n## Usage\n\n```\nagentzero run \"@research-assistant what are the latest developments in WebAssembly?\"\n```\n".to_string()),
        }),
        "telegram-bot" => Ok(BuiltinTemplate {
            skill_toml: r#"[skill]
name = "telegram-bot"
version = "0.1.0"
description = "Telegram bot channel integration for AgentZero"
keywords = ["telegram", "channel", "bot"]
provides = ["channel", "agent"]
"#.to_string(),
            agent_md: Some(r#"---
name: telegram-bot
---

You are a Telegram bot manager. You handle incoming messages from Telegram, route them to the appropriate agent or skill, and send responses back through the Telegram Bot API.

Responsibilities:
1. Process incoming Telegram messages and commands
2. Route conversations to the correct agent based on context
3. Format responses appropriately for Telegram (Markdown, inline keyboards)
4. Manage bot commands and menu configuration
5. Handle media messages (photos, documents, voice) when supported
"#.to_string()),
            config_toml: Some(r#"[channels.telegram]
bot_token = "YOUR_TELEGRAM_BOT_TOKEN"
"#.to_string()),
            readme: Some("# Telegram Bot Skill\n\nTelegram bot channel integration for AgentZero.\n\n## Setup\n\n1. Create a bot via [@BotFather](https://t.me/BotFather) on Telegram\n2. Copy the bot token\n3. Set `bot_token` in the generated `config.toml` or via `TELEGRAM_BOT_TOKEN` env var\n4. Run `agentzero channel add telegram`\n\n## Usage\n\n```\nagentzero skill install telegram-bot\nagentzero channel add telegram\nagentzero daemon start\n```\n".to_string()),
        }),
        "discord-bot" => Ok(BuiltinTemplate {
            skill_toml: r#"[skill]
name = "discord-bot"
version = "0.1.0"
description = "Discord bot channel integration for AgentZero"
keywords = ["discord", "channel", "bot"]
provides = ["channel", "agent"]
"#.to_string(),
            agent_md: Some(r#"---
name: discord-bot
---

You are a Discord bot manager. You handle incoming messages from Discord servers, route them to the appropriate agent or skill, and send responses back through the Discord API.

Responsibilities:
1. Process incoming Discord messages and slash commands
2. Route conversations to the correct agent based on channel and context
3. Format responses with Discord embeds and components when appropriate
4. Manage slash command registration and permissions
5. Handle threads, reactions, and other Discord-specific interactions
"#.to_string()),
            config_toml: Some(r#"[channels.discord]
bot_token = "YOUR_DISCORD_BOT_TOKEN"
"#.to_string()),
            readme: Some("# Discord Bot Skill\n\nDiscord bot channel integration for AgentZero.\n\n## Setup\n\n1. Create a Discord application at https://discord.com/developers/applications\n2. Create a bot user and copy the token\n3. Set `bot_token` in the generated `config.toml` or via `DISCORD_BOT_TOKEN` env var\n4. Invite the bot to your server with the appropriate permissions\n\n## Usage\n\n```\nagentzero skill install discord-bot\nagentzero channel add discord\nagentzero daemon start\n```\n".to_string()),
        }),
        "slack-bot" => Ok(BuiltinTemplate {
            skill_toml: r#"[skill]
name = "slack-bot"
version = "0.1.0"
description = "Slack bot channel integration for AgentZero"
keywords = ["slack", "channel", "bot"]
provides = ["channel", "agent"]
"#.to_string(),
            agent_md: Some(r#"---
name: slack-bot
---

You are a Slack bot manager. You handle incoming messages from Slack workspaces, route them to the appropriate agent or skill, and send responses back through the Slack API.

Responsibilities:
1. Process incoming Slack messages, mentions, and slash commands
2. Route conversations to the correct agent based on channel and context
3. Format responses with Slack Block Kit for rich messaging
4. Manage app home tab and shortcut configurations
5. Handle threads, reactions, and file uploads
"#.to_string()),
            config_toml: Some(r#"[channels.slack]
bot_token = "YOUR_SLACK_BOT_TOKEN"
app_token = "YOUR_SLACK_APP_TOKEN"
"#.to_string()),
            readme: Some("# Slack Bot Skill\n\nSlack bot channel integration for AgentZero.\n\n## Setup\n\n1. Create a Slack app at https://api.slack.com/apps\n2. Enable Socket Mode and generate an app-level token\n3. Install the app to your workspace and copy the bot token\n4. Set `bot_token` and `app_token` in the generated `config.toml` or via environment variables\n\n## Usage\n\n```\nagentzero skill install slack-bot\nagentzero channel add slack\nagentzero daemon start\n```\n".to_string()),
        }),
        "devops-monitor" => Ok(BuiltinTemplate {
            skill_toml: r#"[skill]
name = "devops-monitor"
version = "0.1.0"
description = "DevOps monitoring agent for health checks, alerts, and log analysis"
keywords = ["devops", "monitoring", "health"]
provides = ["agent"]
"#.to_string(),
            agent_md: Some(r#"---
name: devops-monitor
---

You are a DevOps monitoring agent. You perform health checks, analyze logs, detect anomalies, and alert on infrastructure issues.

Responsibilities:
1. **Health checks**: Monitor service endpoints and report availability
2. **Log analysis**: Parse and summarize log files, identify error patterns and anomalies
3. **Alerting**: Notify when services are degraded or thresholds are exceeded
4. **Diagnostics**: Help troubleshoot issues by correlating events across services
5. **Reporting**: Generate status summaries and incident timelines

When analyzing logs, look for:
- Error rate spikes and new error patterns
- Latency increases and timeout patterns
- Resource exhaustion signals (disk, memory, connections)
- Security-relevant events (failed auth, unusual access patterns)
"#.to_string()),
            config_toml: None,
            readme: Some("# DevOps Monitor Skill\n\nDevOps monitoring agent for health checks, alerts, and log analysis.\n\n## Usage\n\n```\nagentzero skill install devops-monitor\nagentzero run \"@devops-monitor check the health of all services\"\nagentzero run \"@devops-monitor analyze the last hour of logs for errors\"\n```\n\n## Scheduling\n\nCombine with the scheduler skill for periodic health checks:\n\n```\nagentzero run \"@scheduler run @devops-monitor health check every 5 minutes\"\n```\n".to_string()),
        }),
        other => bail!(
            "unknown built-in skill `{other}`. Available: {}",
            list_builtin_skills().join(", ")
        ),
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn copy_dir_recursive(src: &Path, dest: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if ty.is_dir() {
            // Skip .git directories
            if entry.file_name() == ".git" {
                continue;
            }
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-{prefix}-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn parse_skill_toml() {
        let toml_str = r#"
[skill]
name = "test-skill"
version = "1.0.0"
description = "A test skill"
keywords = ["test"]
provides = ["agent", "tools"]

[[tools]]
name = "my_tool"
type = "script"
path = "extensions/tool.py"

[[commands]]
name = "test"
description = "Run test"
handler = "agent"
"#;
        let manifest: SkillManifest = toml::from_str(toml_str).expect("should parse");
        assert_eq!(manifest.skill.name, "test-skill");
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].name, "my_tool");
        assert_eq!(manifest.commands.len(), 1);
    }

    #[test]
    fn parse_agent_md_with_frontmatter() {
        let dir = temp_dir("agent-md");
        let path = dir.join("AGENT.md");
        fs::write(&path, "---\nname: reviewer\nmodel: claude-sonnet-4-6\ntools: [read_file, shell]\n---\n\nYou are a code reviewer.\n").expect("write");

        let (prompt, fm) = parse_agent_md(&path).expect("parse");
        assert_eq!(prompt.as_deref(), Some("You are a code reviewer."));
        let fm = fm.expect("frontmatter should exist");
        assert_eq!(fm["name"], serde_json::json!("reviewer"));
        assert_eq!(fm["tools"], serde_json::json!(["read_file", "shell"]));

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn parse_agent_md_without_frontmatter() {
        let dir = temp_dir("agent-md-nofm");
        let path = dir.join("AGENT.md");
        fs::write(&path, "You are a simple agent.").expect("write");

        let (prompt, fm) = parse_agent_md(&path).expect("parse");
        assert_eq!(prompt.as_deref(), Some("You are a simple agent."));
        assert!(fm.is_none());

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn install_builtin_code_reviewer() {
        let dir = temp_dir("install-builtin");
        let skill = install_builtin("code-reviewer", &dir).expect("install should succeed");
        assert_eq!(skill.name(), "code-reviewer");
        assert!(skill.agent_prompt.is_some());
        assert!(skill.dir.join("skill.toml").exists());
        assert!(skill.dir.join("AGENT.md").exists());
        assert!(skill.dir.join("README.md").exists());

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn install_builtin_duplicate_fails() {
        let dir = temp_dir("install-dup");
        install_builtin("code-reviewer", &dir).expect("first install");
        let err = install_builtin("code-reviewer", &dir).expect_err("duplicate should fail");
        assert!(err.to_string().contains("already installed"));

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn discover_skills_precedence() {
        let project = temp_dir("discover-project");
        let global = temp_dir("discover-global");

        // Install same skill in both locations
        install_builtin("code-reviewer", &project.join(".agentzero")).expect("project install");
        install_builtin("code-reviewer", &global).expect("global install");
        install_builtin("scheduler", &global).expect("global scheduler");

        let skills = discover_skills(Some(&project), Some(&global));
        assert_eq!(skills.len(), 2, "should find 2 unique skills");

        // code-reviewer should come from project (higher precedence)
        let cr = skills
            .iter()
            .find(|s| s.name() == "code-reviewer")
            .expect("code-reviewer");
        assert_eq!(cr.source, "project");

        // scheduler should come from global
        let sched = skills
            .iter()
            .find(|s| s.name() == "scheduler")
            .expect("scheduler");
        assert_eq!(sched.source, "global");

        fs::remove_dir_all(project).expect("cleanup");
        fs::remove_dir_all(global).expect("cleanup");
    }

    #[test]
    fn remove_skill_success() {
        let dir = temp_dir("remove-skill");
        install_builtin("code-reviewer", &dir).expect("install");
        assert!(dir.join("skills/code-reviewer/skill.toml").exists());

        remove_skill("code-reviewer", &dir).expect("remove should succeed");
        assert!(!dir.join("skills/code-reviewer").exists());

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn remove_nonexistent_fails() {
        let dir = temp_dir("remove-noexist");
        let err = remove_skill("nonexistent", &dir).expect_err("should fail");
        assert!(err.to_string().contains("not installed"));

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn install_from_local_path() {
        let source = temp_dir("local-source");
        let target = temp_dir("local-target");

        // Create a skill in source dir
        fs::write(
            source.join("skill.toml"),
            "[skill]\nname = \"local-skill\"\ndescription = \"A local skill\"\n",
        )
        .expect("write skill.toml");
        fs::write(source.join("AGENT.md"), "You are a local agent.").expect("write AGENT.md");

        let skill = install_from_local(&source, &target).expect("install should succeed");
        assert_eq!(skill.name(), "local-skill");
        assert!(target.join("skills/local-skill/skill.toml").exists());

        fs::remove_dir_all(source).expect("cleanup");
        fs::remove_dir_all(target).expect("cleanup");
    }

    #[test]
    fn list_builtin_skills_not_empty() {
        let builtins = list_builtin_skills();
        assert!(!builtins.is_empty());
        assert!(builtins.contains(&"code-reviewer"));
        assert!(builtins.contains(&"scheduler"));
        assert!(builtins.contains(&"research-assistant"));
    }

    #[test]
    fn skill_source_parse() {
        assert!(matches!(
            SkillSource::parse("code-reviewer"),
            SkillSource::BuiltIn(_)
        ));
        assert!(matches!(
            SkillSource::parse("https://github.com/user/skill"),
            SkillSource::GitUrl(_)
        ));
        assert!(matches!(
            SkillSource::parse("./my-skill"),
            SkillSource::LocalPath(_)
        ));
        assert!(matches!(
            SkillSource::parse("../other/skill"),
            SkillSource::LocalPath(_)
        ));
    }

    #[test]
    fn parse_workflow_pack_manifest() {
        let toml_str = r#"
[skill]
name = "customer-support"
version = "1.0.0"
description = "Complete customer support workflow"
provides = ["agent", "tools", "workflow"]

[workflow]
name = "Customer Support Pipeline"
description = "Triages, routes, and resolves customer tickets"

[[workflow.nodes]]
id = "triage"
type = "agent"
ref_name = "triage-agent"

[[workflow.nodes]]
id = "resolver"
type = "agent"
ref_name = "resolver-agent"

[[workflow.nodes]]
id = "escalate"
type = "decision"

[[workflow.edges]]
from = "triage"
to = "escalate"
type = "data"

[[workflow.edges]]
from = "escalate"
to = "resolver"
type = "conditional"
condition = "priority <= 3"

[[workflow.entry_points]]
type = "webhook"
target = "triage"
pattern = "/support/*"

[[workflow.cron]]
schedule = "0 9 * * 1-5"
target = "triage"
description = "Process overnight tickets every weekday morning"

[[dependencies]]
name = "email-triage"
version = ">=1.0.0"
"#;
        let manifest: SkillManifest = toml::from_str(toml_str).expect("should parse workflow pack");
        assert_eq!(manifest.skill.name, "customer-support");

        let wf = manifest.workflow.expect("workflow should be present");
        assert_eq!(wf.nodes.len(), 3);
        assert_eq!(wf.edges.len(), 2);
        assert_eq!(wf.entry_points.len(), 1);
        assert_eq!(wf.cron.len(), 1);
        assert_eq!(wf.cron[0].schedule, "0 9 * * 1-5");

        assert_eq!(manifest.dependencies.len(), 1);
        assert_eq!(manifest.dependencies[0].name, "email-triage");
    }

    #[test]
    fn workflow_pack_install_from_local() {
        let source = temp_dir("wfpack-source");
        let target = temp_dir("wfpack-target");

        let skill_toml = r#"[skill]
name = "test-workflow"
description = "A test workflow pack"
provides = ["agent", "workflow"]

[workflow]
name = "Test Flow"

[[workflow.nodes]]
id = "start"
type = "agent"
ref_name = "starter"

[[workflow.entry_points]]
type = "user"
target = "start"
"#;
        fs::write(source.join("skill.toml"), skill_toml).expect("write skill.toml");
        fs::write(
            source.join("AGENT.md"),
            "---\nname: starter\n---\nYou start workflows.",
        )
        .expect("write AGENT.md");

        let skill = install_from_local(&source, &target).expect("install workflow pack");
        assert_eq!(skill.name(), "test-workflow");
        assert!(skill.manifest.workflow.is_some());
        let wf = skill.manifest.workflow.as_ref().expect("workflow");
        assert_eq!(wf.nodes.len(), 1);

        fs::remove_dir_all(source).expect("cleanup");
        fs::remove_dir_all(target).expect("cleanup");
    }
}
