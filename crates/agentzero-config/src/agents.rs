//! Markdown-based agent discovery and definition.
//!
//! Agents can be defined as simple markdown files in `agents/` directories.
//! Only the `name` field is required — everything else has smart defaults:
//!
//! - `model`: inherits from main config
//! - `tools`: all available tools (only specify to restrict)
//! - `preset`: defaults to `production`
//! - `listens_to`: defaults to `["*"]` (everything)
//! - `talks_to`: defaults to all other agents
//! - `heartbeat`: optional cron expression for autonomous cycles
//! - `budget_usd_monthly`: optional spending cap
//!
//! Discovery order (project-local wins):
//! 1. `$PWD/.agentzero/agents/` and `$PWD/agents/`
//! 2. `~/.agentzero/agents/`
//! 3. Agents provided by installed skills (from AGENT.md)

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ─── Agent definition ────────────────────────────────────────────────────────

/// A discovered agent definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Agent name (required). Used for @-mention routing.
    pub name: String,
    /// LLM model override (inherits from main config if not set).
    #[serde(default)]
    pub model: Option<String>,
    /// Tools this agent can use. Empty = all available tools.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Security preset: "production" (default), "dev", "sandbox", "full".
    #[serde(default = "default_preset")]
    pub preset: String,
    /// Event topics and @-mention patterns this agent responds to.
    /// Default: `["*"]` (everything).
    #[serde(default = "default_listens_to")]
    pub listens_to: Vec<String>,
    /// Agents this one can delegate to. Empty = all other agents.
    #[serde(default)]
    pub talks_to: Vec<String>,
    /// Cron expression for autonomous heartbeat cycles.
    #[serde(default)]
    pub heartbeat: Option<String>,
    /// Monthly budget cap in USD.
    #[serde(default)]
    pub budget_usd_monthly: Option<f64>,
    /// System prompt (the markdown body of the agent file).
    #[serde(default)]
    pub system_prompt: String,
    /// Where this agent was discovered.
    #[serde(skip)]
    pub source: AgentSource,
    /// Path to the agent definition file (if file-based).
    #[serde(skip)]
    pub file_path: Option<PathBuf>,
}

fn default_preset() -> String {
    "production".to_string()
}

fn default_listens_to() -> Vec<String> {
    vec!["*".to_string()]
}

/// Where an agent definition came from.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum AgentSource {
    /// From a file in `agents/` directory.
    #[default]
    File,
    /// From a skill's AGENT.md.
    Skill(String),
    /// From TOML config `[agents.*]` section.
    Config,
}

/// Get all agents that have heartbeat schedules defined.
pub fn agents_with_heartbeats(agents: &[AgentDefinition]) -> Vec<&AgentDefinition> {
    agents.iter().filter(|a| a.heartbeat.is_some()).collect()
}

// ─── Discovery ───────────────────────────────────────────────────────────────

/// Discover all agent definitions from filesystem.
///
/// Scans directories in precedence order. Project-local agents override
/// global agents with the same name.
pub fn discover_agents(
    project_dir: Option<&Path>,
    global_dir: Option<&Path>,
) -> Vec<AgentDefinition> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut agents = Vec::new();

    // 1. Project-local: $PWD/.agentzero/agents/ and $PWD/agents/
    if let Some(project) = project_dir {
        for subdir in &[
            project.join(".agentzero").join("agents"),
            project.join("agents"),
        ] {
            if subdir.is_dir() {
                discover_from_dir(subdir, &mut agents, &mut seen);
            }
        }
    }

    // 2. Global: ~/.agentzero/agents/
    if let Some(global) = global_dir {
        let agents_dir = global.join("agents");
        if agents_dir.is_dir() {
            discover_from_dir(&agents_dir, &mut agents, &mut seen);
        }
    }

    agents
}

fn discover_from_dir(
    dir: &Path,
    agents: &mut Vec<AgentDefinition>,
    seen: &mut HashMap<String, usize>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        // Only .md files
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("md") {
            continue;
        }

        match parse_agent_file(&path) {
            Ok(agent) => {
                if seen.contains_key(&agent.name) {
                    tracing::debug!(
                        agent = agent.name,
                        path = %path.display(),
                        "skipping duplicate agent (higher-precedence already loaded)"
                    );
                } else {
                    seen.insert(agent.name.clone(), agents.len());
                    agents.push(agent);
                }
            }
            Err(e) => {
                tracing::warn!(
                    file = %path.display(),
                    error = %e,
                    "failed to parse agent definition"
                );
            }
        }
    }
}

// ─── Parsing ─────────────────────────────────────────────────────────────────

/// Parse a markdown agent definition file.
pub fn parse_agent_file(path: &Path) -> anyhow::Result<AgentDefinition> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let mut agent = parse_agent_content(&content)?;
    agent.file_path = Some(path.to_path_buf());
    agent.source = AgentSource::File;

    // If no name in frontmatter, derive from filename
    if agent.name.is_empty() {
        agent.name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .to_string();
    }

    Ok(agent)
}

/// Parse agent definition from string content (frontmatter + body).
pub fn parse_agent_content(content: &str) -> anyhow::Result<AgentDefinition> {
    let trimmed = content.trim();

    if !trimmed.starts_with("---") {
        // No frontmatter — entire content is the system prompt.
        // Name must be provided externally (e.g., from filename).
        return Ok(AgentDefinition {
            name: String::new(),
            model: None,
            tools: Vec::new(),
            preset: default_preset(),
            listens_to: default_listens_to(),
            talks_to: Vec::new(),
            heartbeat: None,
            budget_usd_monthly: None,
            system_prompt: content.to_string(),
            source: AgentSource::File,
            file_path: None,
        });
    }

    // Find closing ---
    let after_first = &trimmed[3..];
    let end_idx = after_first
        .find("---")
        .context("malformed frontmatter: missing closing ---")?;

    let frontmatter_str = after_first[..end_idx].trim();
    let body = after_first[end_idx + 3..].trim();

    // Parse frontmatter fields
    let fm = parse_frontmatter(frontmatter_str);

    let name = fm
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let model = fm.get("model").and_then(|v| v.as_str()).map(String::from);

    let tools = fm
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let preset = fm
        .get("preset")
        .and_then(|v| v.as_str())
        .unwrap_or("production")
        .to_string();

    let listens_to = fm
        .get("listens_to")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(default_listens_to);

    let talks_to = fm
        .get("talks_to")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let heartbeat = fm
        .get("heartbeat")
        .and_then(|v| v.as_str())
        .map(String::from);

    let budget_usd_monthly = fm.get("budget_usd_monthly").and_then(|v| v.as_f64());

    Ok(AgentDefinition {
        name,
        model,
        tools,
        preset,
        listens_to,
        talks_to,
        heartbeat,
        budget_usd_monthly,
        system_prompt: body.to_string(),
        source: AgentSource::File,
        file_path: None,
    })
}

/// Parse simple YAML-like frontmatter (key: value pairs, one per line).
fn parse_frontmatter(s: &str) -> HashMap<String, serde_json::Value> {
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
                serde_json::Value::String(value.trim_matches('"').to_string())
            };

            map.insert(key, parsed);
        }
    }

    map
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
            "agentzero-agents-{prefix}-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn parse_agent_with_all_fields() {
        let content = r#"---
name: reviewer
model: claude-sonnet-4-6
tools: [read_file, shell, git]
preset: dev
listens_to: ["@reviewer", "code.review.*"]
talks_to: [writer, researcher]
heartbeat: "*/5 * * * *"
budget_usd_monthly: 50.0
---

You are a senior code reviewer. Always cite line numbers.
"#;
        let agent = parse_agent_content(content).expect("should parse");
        assert_eq!(agent.name, "reviewer");
        assert_eq!(agent.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(agent.tools, vec!["read_file", "shell", "git"]);
        assert_eq!(agent.preset, "dev");
        assert_eq!(agent.listens_to, vec!["@reviewer", "code.review.*"]);
        assert_eq!(agent.talks_to, vec!["writer", "researcher"]);
        assert_eq!(agent.heartbeat.as_deref(), Some("*/5 * * * *"));
        assert_eq!(agent.budget_usd_monthly, Some(50.0));
        assert!(agent.system_prompt.contains("senior code reviewer"));
    }

    #[test]
    fn parse_agent_minimal_only_name() {
        let content = r#"---
name: simple
---

You are a helpful assistant.
"#;
        let agent = parse_agent_content(content).expect("should parse");
        assert_eq!(agent.name, "simple");
        assert!(agent.model.is_none());
        assert!(agent.tools.is_empty(), "empty tools = all tools");
        assert_eq!(agent.preset, "production");
        assert_eq!(agent.listens_to, vec!["*"]);
        assert!(agent.talks_to.is_empty(), "empty talks_to = all agents");
        assert!(agent.heartbeat.is_none());
        assert!(agent.budget_usd_monthly.is_none());
        assert!(agent.system_prompt.contains("helpful assistant"));
    }

    #[test]
    fn parse_agent_no_frontmatter() {
        let content = "You are a bare agent with no frontmatter.";
        let agent = parse_agent_content(content).expect("should parse");
        assert!(
            agent.name.is_empty(),
            "name should be empty (set from filename)"
        );
        assert_eq!(agent.system_prompt, content);
    }

    #[test]
    fn parse_agent_file_derives_name_from_filename() {
        let dir = temp_dir("filename-name");
        let path = dir.join("my-agent.md");
        fs::write(&path, "You are an agent.").expect("write");

        let agent = parse_agent_file(&path).expect("should parse");
        assert_eq!(agent.name, "my-agent");
        assert!(agent.file_path.is_some());

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn parse_agent_file_frontmatter_name_wins() {
        let dir = temp_dir("fm-name");
        let path = dir.join("filename.md");
        fs::write(&path, "---\nname: frontmatter-name\n---\nPrompt.").expect("write");

        let agent = parse_agent_file(&path).expect("should parse");
        assert_eq!(
            agent.name, "frontmatter-name",
            "frontmatter name should take precedence"
        );

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn discover_agents_from_directories() {
        let project = temp_dir("discover-project");
        let global = temp_dir("discover-global");

        // Create project-local agent
        let proj_agents = project.join("agents");
        fs::create_dir_all(&proj_agents).expect("mkdir");
        fs::write(
            proj_agents.join("reviewer.md"),
            "---\nname: reviewer\n---\nReview code.",
        )
        .expect("write");

        // Create global agent
        let global_agents = global.join("agents");
        fs::create_dir_all(&global_agents).expect("mkdir");
        fs::write(
            global_agents.join("reviewer.md"),
            "---\nname: reviewer\n---\nGlobal reviewer.",
        )
        .expect("write");
        fs::write(
            global_agents.join("writer.md"),
            "---\nname: writer\n---\nWrite code.",
        )
        .expect("write");

        let agents = discover_agents(Some(&project), Some(&global));
        assert_eq!(agents.len(), 2, "reviewer (project) + writer (global)");

        let reviewer = agents
            .iter()
            .find(|a| a.name == "reviewer")
            .expect("reviewer");
        assert!(
            reviewer.system_prompt.contains("Review code"),
            "project-local should win"
        );

        let writer = agents.iter().find(|a| a.name == "writer").expect("writer");
        assert!(writer.system_prompt.contains("Write code"));

        fs::remove_dir_all(project).expect("cleanup");
        fs::remove_dir_all(global).expect("cleanup");
    }

    #[test]
    fn discover_agents_dot_agentzero_dir() {
        let project = temp_dir("discover-dotdir");

        // Create agent in .agentzero/agents/
        let dot_agents = project.join(".agentzero").join("agents");
        fs::create_dir_all(&dot_agents).expect("mkdir");
        fs::write(
            dot_agents.join("helper.md"),
            "---\nname: helper\n---\nI help.",
        )
        .expect("write");

        let agents = discover_agents(Some(&project), None);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "helper");

        fs::remove_dir_all(project).expect("cleanup");
    }

    #[test]
    fn agents_with_heartbeats_returns_only_heartbeat_agents() {
        let agents = vec![
            AgentDefinition {
                name: "watcher".to_string(),
                heartbeat: Some("*/5 * * * *".to_string()),
                ..make_default_agent("watcher")
            },
            AgentDefinition {
                name: "helper".to_string(),
                heartbeat: None,
                ..make_default_agent("helper")
            },
            AgentDefinition {
                name: "monitor".to_string(),
                heartbeat: Some("0 * * * *".to_string()),
                ..make_default_agent("monitor")
            },
        ];

        let result = agents_with_heartbeats(&agents);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "watcher");
        assert_eq!(result[0].heartbeat.as_deref(), Some("*/5 * * * *"));
        assert_eq!(result[1].name, "monitor");
        assert_eq!(result[1].heartbeat.as_deref(), Some("0 * * * *"));
    }

    #[test]
    fn agents_with_heartbeats_empty_when_none_have_heartbeats() {
        let agents = vec![make_default_agent("a"), make_default_agent("b")];
        let result = agents_with_heartbeats(&agents);
        assert!(result.is_empty());
    }

    #[test]
    fn agents_with_heartbeats_empty_input() {
        let agents: Vec<AgentDefinition> = Vec::new();
        let result = agents_with_heartbeats(&agents);
        assert!(result.is_empty());
    }

    fn make_default_agent(name: &str) -> AgentDefinition {
        AgentDefinition {
            name: name.to_string(),
            model: None,
            tools: Vec::new(),
            preset: default_preset(),
            listens_to: default_listens_to(),
            talks_to: Vec::new(),
            heartbeat: None,
            budget_usd_monthly: None,
            system_prompt: String::new(),
            source: AgentSource::File,
            file_path: None,
        }
    }

    #[test]
    fn discover_ignores_non_md_files() {
        let project = temp_dir("discover-nonmd");
        let agents_dir = project.join("agents");
        fs::create_dir_all(&agents_dir).expect("mkdir");
        fs::write(agents_dir.join("agent.md"), "---\nname: valid\n---\nOk.").expect("write");
        fs::write(agents_dir.join("notes.txt"), "not an agent").expect("write");
        fs::write(agents_dir.join("data.json"), "{}").expect("write");

        let agents = discover_agents(Some(&project), None);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "valid");

        fs::remove_dir_all(project).expect("cleanup");
    }
}
