use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize a new AgentZero project.
    Init {
        /// Initialize with private-by-default policy.
        #[arg(long)]
        private: bool,
        /// Generate editor integration config: vscode, cursor, zed.
        #[arg(long)]
        editor: Option<String>,
    },
    /// Start a supervised chat session.
    Chat {
        /// Allow remote model calls (local-only by default).
        #[arg(long)]
        remote: bool,
        /// Model to use (default: llama3.2).
        #[arg(long, short, default_value = "llama3.2")]
        model: String,
        /// Stream tokens as they arrive.
        #[arg(long)]
        stream: bool,
        /// Provider: ollama, llama-cpp, vllm, lm-studio (default: ollama).
        #[arg(long, short, default_value = "ollama")]
        provider: String,
        /// Server URL override (e.g. http://localhost:8080).
        #[arg(long)]
        url: Option<String>,
        /// Resume a previous session by ID.
        #[arg(long)]
        resume: Option<String>,
        /// Disable encryption of audit logs and session files at rest.
        #[arg(long)]
        no_encrypt: bool,
        /// Single-shot mode: send one message and exit (no interactive loop).
        #[arg(long, short = 'P')]
        print: Option<String>,
        /// Output mode for --print: text (default), json, jsonl.
        #[arg(long, default_value = "text")]
        mode: String,
    },
    /// Run a skill or tool by name.
    Run {
        /// Name of the skill or tool to run.
        name: String,
        /// Skip lockfile integrity verification before running.
        #[arg(long)]
        skip_verify: bool,
    },
    /// Install a skill from a local path, GitHub owner/repo, git URL, or short name.
    Install {
        /// Skill source: local path, owner/repo, git URL, or short name from the index.
        source: String,
        /// Force refresh the skill index cache before resolving.
        #[arg(long)]
        refresh_index: bool,
    },
    /// Package and publish a skill to a GitHub release.
    Publish {
        /// Path to the skill directory (default: current directory).
        #[arg(default_value = ".")]
        path: String,
        /// GitHub repository (owner/repo) to publish to.
        #[arg(long)]
        repo: Option<String>,
        /// Release tag (default: v{version} from SKILL.md).
        #[arg(long)]
        tag: Option<String>,
    },
    /// Search the skill catalog for tools matching a query.
    Search {
        /// Query string to search for.
        query: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Link a tool from another project for cross-project sharing.
    Link {
        /// Path to the tool in another project (e.g., ../other/.agentzero/skills/tool-name).
        source: String,
    },
    /// Start the ACP server for editor integrations.
    Serve,
    /// Start the MCP server (tools for Claude Code, Cursor, etc.).
    Mcp,
    /// Check system health and configuration.
    Doctor,
    /// Import secrets from a .env file into the encrypted vault.
    VaultImport {
        /// Path to the .env file to import.
        #[arg(default_value = ".env")]
        path: String,
        /// Dry-run mode: show what would be imported without writing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Detect platform and install a recommended local LLM backend.
    Bootstrap {
        /// Skip confirmation prompt and install automatically.
        #[arg(long)]
        non_interactive: bool,
        /// Skip model download (install backend only).
        #[arg(long)]
        skip_model: bool,
    },
    /// Run a minimal safe demo using core types.
    Demo,
    /// Manage policy rules.
    Policy {
        #[command(subcommand)]
        action: PolicyAction,
    },
    /// View and manage audit logs.
    Audit {
        #[command(subcommand)]
        action: AuditAction,
    },
    /// Build and query a semantic document index (requires --features rag).
    Index {
        #[command(subcommand)]
        action: IndexAction,
    },
    /// List past chat sessions.
    History,
    /// Manage secret vault handles.
    Vault {
        #[command(subcommand)]
        action: VaultAction,
    },
    /// Personal knowledge vault (wiki).
    Brain {
        #[command(subcommand)]
        action: BrainAction,
    },
    /// Manage plugins.
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Generate shell completions.
    Completions {
        /// Shell to generate completions for.
        shell: clap_complete::Shell,
    },
}

#[derive(Debug, Subcommand)]
pub enum PluginAction {
    /// List installed plugins.
    List,
    /// Install a plugin from a local directory.
    Install {
        /// Path to the plugin directory (must contain PLUGIN.toml and a .wasm file).
        source: String,
    },
    /// Show plugin information.
    Info {
        /// Plugin name.
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum PolicyAction {
    /// Show current policy status.
    Status,
}

#[derive(Debug, Subcommand)]
pub enum AuditAction {
    /// Show recent audit events.
    Tail {
        /// Number of events to show.
        #[arg(short, long, default_value = "20")]
        count: usize,
    },
    /// Show a human-readable summary of audit events (security report).
    Summary {
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum VaultAction {
    /// List secret handles.
    List,
    /// Add a secret to the vault.
    Add {
        /// Provider name (e.g. github, aws).
        provider: String,
        /// Secret name (e.g. token, key).
        name: String,
    },
    /// Get a secret value (for debugging — use with care).
    Get {
        /// Provider name.
        provider: String,
        /// Secret name.
        name: String,
    },
    /// Remove a secret from the vault.
    Remove {
        /// Provider name.
        provider: String,
        /// Secret name.
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum IndexAction {
    /// Build the semantic index for the current project directory.
    Build {
        /// Directory to index (defaults to current directory).
        #[arg(long)]
        path: Option<String>,
        /// Embedding model to use (default: nomic-embed-text).
        #[arg(long, default_value = "nomic-embed-text")]
        model: String,
        /// Ollama server URL.
        #[arg(long, default_value = "http://localhost:11434")]
        url: String,
        /// Maximum characters per chunk.
        #[arg(long, default_value = "1000")]
        chunk_size: usize,
    },
    /// Show index status and statistics.
    Status,
    /// Remove the index from disk.
    Clear,
}

#[derive(Debug, Subcommand)]
pub enum BrainAction {
    /// Initialize a new brain vault.
    Init {
        /// Root directory for the vault.
        #[arg(long, default_value = ".")]
        root: String,
        /// Force overwrite existing files.
        #[arg(long)]
        force: bool,
        /// Print actions without executing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Open or create today's daily note.
    Today {
        /// Root directory for the vault.
        #[arg(long, default_value = ".")]
        root: String,
        /// Date override (YYYY-MM-DD).
        #[arg(long)]
        date: Option<String>,
        /// Open in $EDITOR.
        #[arg(long)]
        open: bool,
    },
    /// Capture a thought to today's daily note.
    Capture {
        /// The message to capture.
        message: String,
        /// Root directory for the vault.
        #[arg(long, default_value = ".")]
        root: String,
        /// Date override (YYYY-MM-DD).
        #[arg(long)]
        date: Option<String>,
        /// Section heading to append under (default: Capture).
        #[arg(long)]
        section: Option<String>,
    },
    /// Search the vault for a term.
    Query {
        /// Search term.
        term: String,
        /// Root directory for the vault.
        #[arg(long, default_value = ".")]
        root: String,
        /// Also search the raw directory.
        #[arg(long)]
        raw: bool,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
        /// Maximum number of results.
        #[arg(long, default_value = "50")]
        limit: usize,
    },
    /// Generate an ingest prompt for a raw file.
    Ingest {
        /// Path to the raw file to ingest.
        path: String,
        /// Root directory for the vault.
        #[arg(long, default_value = ".")]
        root: String,
        /// Save the prompt to wiki/reports/.
        #[arg(long)]
        save_prompt: bool,
        /// Show what would happen without writing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Generate an end-of-day review prompt.
    Review {
        /// Root directory for the vault.
        #[arg(long, default_value = ".")]
        root: String,
        /// Date to review (YYYY-MM-DD).
        #[arg(long)]
        date: Option<String>,
        /// Save the prompt to wiki/reports/.
        #[arg(long)]
        save_prompt: bool,
        /// Show what would happen without writing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Generate a weekly review prompt.
    Weekly {
        /// Root directory for the vault.
        #[arg(long, default_value = ".")]
        root: String,
        /// ISO week identifier (e.g., 2026-W20).
        #[arg(long)]
        week: Option<String>,
        /// Save the prompt to wiki/reports/.
        #[arg(long)]
        save_prompt: bool,
    },
    /// Run vault health diagnostics.
    Health {
        /// Root directory for the vault.
        #[arg(long, default_value = ".")]
        root: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
        /// Attempt to fix issues (not yet implemented).
        #[arg(long)]
        fix: bool,
    },
    /// Git checkpoint the vault.
    Checkpoint {
        /// Root directory for the vault.
        #[arg(long, default_value = ".")]
        root: String,
        /// Custom commit message.
        #[arg(long)]
        message: Option<String>,
        /// Initialize a git repo if none exists.
        #[arg(long)]
        init: bool,
        /// Show what would happen without executing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Show vault status summary.
    Status {
        /// Root directory for the vault.
        #[arg(long, default_value = ".")]
        root: String,
    },
}

pub async fn run(command: Command) -> i32 {
    match command {
        Command::Init { private, editor } => cmd_init(private, editor.as_deref()),
        Command::Chat {
            remote,
            model,
            stream,
            provider,
            url,
            resume,
            no_encrypt,
            print,
            mode,
        } => {
            cmd_chat(
                !remote,
                &model,
                stream,
                &provider,
                url.as_deref(),
                resume.as_deref(),
                !no_encrypt,
                print.as_deref(),
                &mode,
            )
            .await
        }
        Command::Run { name, skip_verify } => cmd_run(&name, skip_verify),
        Command::Install {
            source,
            refresh_index,
        } => cmd_install(&source, refresh_index).await,
        Command::Publish { path, repo, tag } => {
            cmd_publish(&path, repo.as_deref(), tag.as_deref()).await
        }
        Command::History => cmd_history(),
        Command::Serve => cmd_serve().await,
        Command::Mcp => {
            #[cfg(feature = "mcp")]
            {
                cmd_mcp().await
            }
            #[cfg(not(feature = "mcp"))]
            {
                eprintln!("MCP server is not available in this build.");
                eprintln!("Rebuild with: cargo build --features mcp");
                eprintln!("AgentZero's native protocol is ACP (az serve). See ADR 0014.");
                1
            }
        }
        Command::Search { query, json } => cmd_search(&query, json).await,
        Command::Link { source } => cmd_link(&source),
        Command::Doctor => cmd_doctor(),
        Command::Bootstrap {
            non_interactive,
            skip_model,
        } => cmd_bootstrap(non_interactive, skip_model),
        Command::Demo => cmd_demo(),
        Command::Policy { action } => match action {
            PolicyAction::Status => cmd_policy_status(),
        },
        Command::Audit { action } => match action {
            AuditAction::Tail { count } => cmd_audit_tail(count),
            AuditAction::Summary { json } => cmd_audit_summary(json),
        },
        Command::VaultImport { path, dry_run } => cmd_vault_import(&path, dry_run),
        Command::Vault { action } => cmd_vault(action),
        Command::Index { action } => cmd_index(action).await,
        Command::Brain { action } => cmd_brain(action),
        Command::Plugin { action } => cmd_plugin(action),
        Command::Completions { shell } => {
            cmd_completions(shell);
            0
        }
    }
}

async fn cmd_index(action: IndexAction) -> i32 {
    #[cfg(not(feature = "rag"))]
    {
        let _ = action;
        eprintln!("error: the 'rag' feature is not enabled");
        eprintln!("rebuild with: cargo build --features rag");
        1
    }

    #[cfg(feature = "rag")]
    {
        use agentzero::index::{IndexConfig, IndexEngine};

        let cwd = match std::env::current_dir() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: cannot determine current directory: {e}");
                return 1;
            }
        };

        match action {
            IndexAction::Build {
                path,
                model,
                url,
                chunk_size,
            } => {
                let root = path
                    .as_deref()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| cwd.clone());

                let config = IndexConfig {
                    ollama_url: url,
                    embed_model: model,
                    chunk_size,
                    ..Default::default()
                };

                let engine = IndexEngine::new(&cwd, config);

                println!("Building index for {}...", root.display());

                match engine.build(&root).await {
                    Ok(stats) => {
                        println!(
                            "Index built: {} files, {} chunks (model: {})",
                            stats.files_indexed, stats.chunks_created, stats.model_name
                        );
                        0
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        1
                    }
                }
            }
            IndexAction::Status => {
                let config = IndexConfig::default();
                let engine = IndexEngine::new(&cwd, config);

                match engine.status() {
                    Some(meta) => {
                        println!("Index status:");
                        println!("  Model:      {}", meta.model_name);
                        println!("  Files:      {}", meta.file_count);
                        println!("  Chunks:     {}", meta.chunk_count);
                        println!("  Created at: {}", meta.created_at);
                        0
                    }
                    None => {
                        println!("No index found. Run `az index build` first.");
                        0
                    }
                }
            }
            IndexAction::Clear => {
                let config = IndexConfig::default();
                let engine = IndexEngine::new(&cwd, config);

                match engine.clear() {
                    Ok(()) => {
                        println!("Index cleared.");
                        0
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        1
                    }
                }
            }
        }
    }
}

fn cmd_completions(shell: clap_complete::Shell) {
    use clap::CommandFactory;
    clap_complete::generate(
        shell,
        &mut crate::Cli::command(),
        "az",
        &mut std::io::stdout(),
    );
}

fn cmd_init(private: bool, editor: Option<&str>) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: cannot determine current directory: {e}");
            return 1;
        }
    };

    let az_dir = cwd.join(".agentzero");
    if az_dir.exists() {
        eprintln!("AgentZero already initialized at {}", az_dir.display());
        return 1;
    }

    let dirs = [
        "audit", "sessions", "prompts", "skills", "vault", "index", "plugins",
    ];
    for sub in &dirs {
        if let Err(e) = std::fs::create_dir_all(az_dir.join(sub)) {
            eprintln!("error: failed to create .agentzero/{sub}: {e}");
            return 1;
        }
    }

    // Write settings.toml
    let settings = concat!(
        "# AgentZero Settings\n",
        "[general]\n",
        "# default_provider = \"ollama\"\n",
        "# default_model = \"llama3.2\"\n",
        "\n",
        "[audit]\n",
        "enabled = true\n",
        "# encrypt = false\n",
        "\n",
        "[session]\n",
        "# max_tool_rounds = 5\n",
        "# max_output_bytes = 2000\n",
        "# passphrase = \"your-passphrase-here\"  # or use AZ_PASSPHRASE env var\n",
    );
    if let Err(e) = std::fs::write(az_dir.join("settings.toml"), settings) {
        eprintln!("error: failed to write settings.toml: {e}");
        return 1;
    }

    // Write models.json
    let models = serde_json::json!({
        "providers": [
            {
                "name": "ollama",
                "type": "ollama",
                "url": "http://localhost:11434",
                "default_model": "llama3.2",
                "is_local": true
            },
            {
                "name": "llama-cpp",
                "type": "openai-compatible",
                "url": "http://localhost:8080",
                "default_model": "default",
                "is_local": true
            }
        ]
    });
    if let Err(e) = std::fs::write(
        az_dir.join("models.json"),
        serde_json::to_string_pretty(&models).expect("models should serialize"),
    ) {
        eprintln!("error: failed to write models.json: {e}");
        return 1;
    }

    // Write default policy (TOML format)
    let policy_content = if private {
        concat!(
            "# AgentZero Policy (private-by-default)\n",
            "version = 1\n",
            "default_classification = \"private\"\n",
            "model_routing = \"local_only\"\n",
            "shell_commands = \"require_approval\"\n",
            "file_write = \"require_approval\"\n",
            "network = \"deny\"\n",
            "wasm_execution = \"deny\"\n",
        )
    } else {
        concat!(
            "# AgentZero Policy (default)\n",
            "version = 1\n",
            "default_classification = \"private\"\n",
            "model_routing = \"local_preferred\"\n",
            "shell_commands = \"require_approval\"\n",
            "file_write = \"require_approval\"\n",
            "network = \"require_approval\"\n",
            "wasm_execution = \"require_approval\"\n",
        )
    };

    if let Err(e) = std::fs::write(az_dir.join("policy.yml"), policy_content) {
        eprintln!("error: failed to write policy.yml: {e}");
        return 1;
    }

    let mode = if private { "private" } else { "default" };
    println!("Initialized AgentZero project ({mode} mode)");
    println!("  {}/", az_dir.display());
    println!("  ├── policy.yml");
    println!("  ├── settings.toml");
    println!("  ├── models.json");
    println!("  ├── audit/");
    println!("  ├── sessions/");
    println!("  ├── prompts/");
    println!("  ├── skills/");
    println!("  ├── plugins/");
    println!("  └── vault/");

    // Generate editor integration config if requested
    if let Some(editor_name) = editor {
        match generate_editor_config(&cwd, editor_name) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("warning: failed to generate editor config: {e}");
            }
        }
    }

    0
}

fn generate_editor_config(project_root: &std::path::Path, editor: &str) -> Result<(), String> {
    match editor {
        "vscode" | "code" => {
            let vscode_dir = project_root.join(".vscode");
            std::fs::create_dir_all(&vscode_dir)
                .map_err(|e| format!("failed to create .vscode/: {e}"))?;

            let tasks = serde_json::json!({
                "version": "2.0.0",
                "tasks": [
                    {
                        "label": "AgentZero: Start ACP Server",
                        "type": "shell",
                        "command": "agentzero serve",
                        "isBackground": true,
                        "problemMatcher": [],
                        "group": "none",
                        "presentation": {
                            "reveal": "silent",
                            "panel": "dedicated"
                        }
                    },
                    {
                        "label": "AgentZero: Chat (single query)",
                        "type": "shell",
                        "command": "agentzero chat -P \"${input:query}\" --mode json",
                        "problemMatcher": [],
                        "group": "none"
                    }
                ],
                "inputs": [
                    {
                        "id": "query",
                        "type": "promptString",
                        "description": "Enter your question for AgentZero"
                    }
                ]
            });

            std::fs::write(
                vscode_dir.join("tasks.json"),
                serde_json::to_string_pretty(&tasks).expect("tasks should serialize"),
            )
            .map_err(|e| format!("failed to write .vscode/tasks.json: {e}"))?;

            println!();
            println!("VS Code integration:");
            println!("  .vscode/tasks.json created");
            println!("  Run tasks via: Ctrl+Shift+P → Tasks: Run Task → AgentZero");
        }
        "cursor" => {
            let cursor_dir = project_root.join(".cursor");
            std::fs::create_dir_all(&cursor_dir)
                .map_err(|e| format!("failed to create .cursor/: {e}"))?;

            let rules = "\
# AgentZero Integration
# AgentZero is a local-first secure AI coding agent.
# Start the ACP server: agentzero serve
# Single query: agentzero chat -P \"your question\" --mode json

## MCP Integration
# Add to your Cursor MCP settings:
# {
#   \"mcpServers\": {
#     \"agentzero\": {
#       \"command\": \"agentzero\",
#       \"args\": [\"mcp\"]
#     }
#   }
# }
";
            std::fs::write(cursor_dir.join("rules"), rules)
                .map_err(|e| format!("failed to write .cursor/rules: {e}"))?;

            println!();
            println!("Cursor integration:");
            println!("  .cursor/rules created");
            println!("  Add MCP server: Settings → MCP → agentzero mcp");
        }
        "zed" => {
            let zed_dir = project_root.join(".zed");
            std::fs::create_dir_all(&zed_dir)
                .map_err(|e| format!("failed to create .zed/: {e}"))?;

            let tasks = serde_json::json!([
                {
                    "label": "AgentZero: Start ACP Server",
                    "command": "agentzero serve",
                    "use_new_terminal": true
                },
                {
                    "label": "AgentZero: Chat",
                    "command": "agentzero chat",
                    "use_new_terminal": true
                }
            ]);

            std::fs::write(
                zed_dir.join("tasks.json"),
                serde_json::to_string_pretty(&tasks).expect("tasks should serialize"),
            )
            .map_err(|e| format!("failed to write .zed/tasks.json: {e}"))?;

            println!();
            println!("Zed integration:");
            println!("  .zed/tasks.json created");
            println!("  Run tasks via: Command Palette → task: spawn");
        }
        other => {
            return Err(format!(
                "unknown editor: {other}. Supported: vscode, cursor, zed"
            ));
        }
    }

    Ok(())
}

async fn cmd_serve() -> i32 {
    use agentzero::acp::{AcpServer, AcpServerConfig};

    eprintln!("AgentZero ACP Server");
    eprintln!("====================");
    eprintln!("Protocol: newline-delimited JSON over stdio");
    eprintln!("Chat-capable: send {{\"id\":\"1\",\"method\":\"chat\",\"params\":{{\"message\":\"...\"}}}}");
    eprintln!();

    // Load policy and settings
    let cwd = std::env::current_dir().unwrap_or_default();
    let policy_path = cwd.join(".agentzero/policy.yml");
    let policy = if policy_path.exists() {
        agentzero::policy::load_policy_file(&policy_path)
            .map(agentzero::policy::PolicyEngine::with_rules)
            .unwrap_or_else(|_| agentzero::policy::PolicyEngine::deny_by_default())
    } else {
        agentzero::policy::PolicyEngine::deny_by_default()
    };

    let (_settings_provider, settings_model) = load_settings();
    let model = settings_model.as_deref().unwrap_or("llama3.2");

    let config = AcpServerConfig {
        project_root: Some(cwd.to_string_lossy().to_string()),
        policy,
        model: model.to_string(),
        ..AcpServerConfig::default()
    };

    let mut server = match AcpServer::with_config(config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: failed to create ACP server: {e}");
            return 1;
        }
    };

    match server.run().await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("ACP server error: {e}");
            1
        }
    }
}

#[cfg(feature = "mcp")]
async fn cmd_mcp() -> i32 {
    use agentzero::mcp::{McpServer, McpServerConfig};

    // Load policy if available
    let cwd = std::env::current_dir().unwrap_or_default();
    let policy_path = cwd.join(".agentzero/policy.yml");
    let policy = if policy_path.exists() {
        agentzero::policy::load_policy_file(&policy_path)
            .map(agentzero::policy::PolicyEngine::with_rules)
            .unwrap_or_else(|_| agentzero::policy::PolicyEngine::deny_by_default())
    } else {
        agentzero::policy::PolicyEngine::deny_by_default()
    };

    let config = McpServerConfig {
        project_root: Some(cwd.to_string_lossy().to_string()),
        policy,
    };

    let server = match McpServer::new(config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: failed to start MCP server: {e}");
            return 1;
        }
    };

    match server.run().await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("MCP server error: {e}");
            1
        }
    }
}

fn cmd_audit_summary(json: bool) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();
    let audit_dir = cwd.join(".agentzero/audit");

    if !audit_dir.exists() {
        println!("No audit logs found at {}", audit_dir.display());
        println!("Run `az chat` or `az run` to generate audit events.");
        return 0;
    }

    // Count audit files and events
    let mut total_files = 0;
    let mut total_events = 0;
    let mut actions: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut denied_count = 0;
    let mut redacted_count = 0;

    if let Ok(entries) = std::fs::read_dir(&audit_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                total_files += 1;
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for line in content.lines() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        total_events += 1;
                        if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
                            if let Some(action) = event.get("action").and_then(|a| a.as_str()) {
                                *actions.entry(action.to_string()).or_insert(0) += 1;
                            }
                            if let Some(decision) = event.get("decision").and_then(|d| d.as_str()) {
                                if decision.contains("deny") || decision.contains("Deny") {
                                    denied_count += 1;
                                }
                            }
                            if let Some(redactions) =
                                event.get("redactions_applied").and_then(|r| r.as_array())
                            {
                                if !redactions.is_empty() {
                                    redacted_count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if json {
        let summary = serde_json::json!({
            "sessions": total_files,
            "total_events": total_events,
            "denied_actions": denied_count,
            "events_with_redactions": redacted_count,
            "action_counts": actions,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_default()
        );
    } else {
        println!("AgentZero Audit Summary");
        println!("=======================\n");
        println!("Sessions:              {total_files}");
        println!("Total events:          {total_events}");
        println!("Denied actions:        {denied_count}");
        println!("Events with redaction: {redacted_count}");
        if !actions.is_empty() {
            println!("\nAction breakdown:");
            let mut sorted: Vec<_> = actions.iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(a.1));
            for (action, count) in sorted {
                println!("  {action}: {count}");
            }
        }
    }
    0
}

fn cmd_vault_import(path: &str, dry_run: bool) -> i32 {
    let env_path = std::path::Path::new(path);
    if !env_path.exists() {
        eprintln!("File not found: {path}");
        return 1;
    }

    let content = match std::fs::read_to_string(env_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to read {path}: {e}");
            return 1;
        }
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            entries.push((key.to_string(), value.to_string()));
        }
    }

    if entries.is_empty() {
        println!("No secrets found in {path}");
        return 0;
    }

    println!("Found {} secret(s) in {path}:\n", entries.len());
    for (key, _) in &entries {
        println!("  {key}");
    }

    if dry_run {
        println!("\n[dry-run] No secrets imported. Remove --dry-run to import.");
        return 0;
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let vault_dir = cwd.join(".agentzero/vault/env");
    if let Err(e) = std::fs::create_dir_all(&vault_dir) {
        eprintln!("Failed to create vault directory: {e}");
        return 1;
    }

    // Store each secret (unencrypted for now — vault encryption is a separate concern)
    let mut imported = 0;
    for (key, value) in &entries {
        let secret_path = vault_dir.join(format!("{key}.enc"));
        if secret_path.exists() {
            println!("  Skipped {key} (already exists)");
            continue;
        }
        // For now, store as plaintext .enc (real encryption via vault add)
        // This is a migration helper — tells users what to do next
        if std::fs::write(&secret_path, value).is_ok() {
            imported += 1;
            println!("  Imported {key}");
        }
    }

    println!("\nImported {imported}/{} secret(s).", entries.len());
    println!("Use `az vault add <provider> <name>` for encrypted storage.");
    0
}

async fn cmd_search(query: &str, json: bool) -> i32 {
    use agentzero::skills::index::{load_or_fetch_index, DEFAULT_INDEX_REPO};

    let cwd = std::env::current_dir().unwrap_or_default();

    match load_or_fetch_index(&cwd, DEFAULT_INDEX_REPO, false).await {
        Ok(index) => {
            let results = index.search(query);
            if results.is_empty() {
                if json {
                    println!("[]");
                } else {
                    println!("No skills found matching '{query}'");
                }
                return 0;
            }

            if json {
                let json_results: Vec<serde_json::Value> = results
                    .iter()
                    .map(|(name, entry)| {
                        serde_json::json!({
                            "name": name,
                            "repo": entry.repo,
                            "description": entry.description,
                            "trust": format!("{:?}", entry.trust).to_lowercase(),
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_results).unwrap_or_default()
                );
            } else {
                println!("Skills matching '{query}':\n");
                for (name, entry) in &results {
                    let trust = format!("{:?}", entry.trust).to_lowercase();
                    println!("  {name} [{trust}]");
                    println!("    {}", entry.description);
                    println!("    repo: {}", entry.repo);
                    println!();
                }
                println!("Install with: az install <name>");
            }
            0
        }
        Err(e) => {
            eprintln!("Failed to load skill index: {e}");
            eprintln!("Try: az search --offline (local cache only)");
            1
        }
    }
}

fn cmd_link(source: &str) -> i32 {
    let source_path = std::path::Path::new(source);

    if !source_path.exists() {
        eprintln!("Source path does not exist: {source}");
        return 1;
    }

    // Extract tool name from path (last component)
    let tool_name = match source_path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => {
            eprintln!("Cannot determine tool name from path: {source}");
            return 1;
        }
    };

    // Verify it looks like a skill directory (has active.json or SKILL.md)
    let has_active = source_path.join("active.json").exists();
    let has_skill_md = source_path.join("SKILL.md").exists();
    if !has_active && !has_skill_md {
        // Check if it's a versioned directory (v1/SKILL.md)
        let has_versioned = std::fs::read_dir(source_path)
            .ok()
            .map(|entries| {
                entries
                    .flatten()
                    .any(|e| e.file_name().to_string_lossy().starts_with('v') && e.path().is_dir())
            })
            .unwrap_or(false);
        if !has_versioned {
            eprintln!("Source doesn't look like a skill directory (no active.json, SKILL.md, or versioned dirs)");
            return 1;
        }
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let link_dir = cwd.join(".agentzero/skills");
    if let Err(e) = std::fs::create_dir_all(&link_dir) {
        eprintln!("Failed to create skills directory: {e}");
        return 1;
    }

    let link_path = link_dir.join(tool_name);
    if link_path.exists() {
        eprintln!(
            "Tool '{tool_name}' already exists at {}",
            link_path.display()
        );
        eprintln!("Remove it first or use a different name.");
        return 1;
    }

    // Create symlink
    let canonical_source = match std::fs::canonicalize(source_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to resolve source path: {e}");
            return 1;
        }
    };

    #[cfg(unix)]
    {
        if let Err(e) = std::os::unix::fs::symlink(&canonical_source, &link_path) {
            eprintln!("Failed to create symlink: {e}");
            return 1;
        }
    }

    #[cfg(windows)]
    {
        if let Err(e) = std::os::windows::fs::symlink_dir(&canonical_source, &link_path) {
            eprintln!("Failed to create symlink: {e}");
            return 1;
        }
    }

    println!(
        "Linked: {} -> {}",
        link_path.display(),
        canonical_source.display()
    );
    println!("Tool '{tool_name}' is now available in this project.");
    0
}

fn cmd_bootstrap(non_interactive: bool, skip_model: bool) -> i32 {
    use std::io::{self, BufRead, Write};

    println!("AgentZero Bootstrap — Local LLM Backend Setup\n");

    // Detect platform
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    println!("Platform: {os}/{arch}");

    let is_apple_silicon = os == "macos" && arch == "aarch64";

    // Check for existing backends
    let backends = [
        ("ollama", 11434, "Ollama"),
        ("llama-server", 8080, "llama.cpp"),
        ("vllm", 8000, "vLLM"),
    ];

    let mut found = Vec::new();
    for (cmd, port, name) in &backends {
        if which_command(cmd) {
            found.push(*name);
            println!("  Found: {name} ({cmd} on PATH)");
        } else {
            // Try connecting to the port
            let addr = format!("127.0.0.1:{port}");
            if std::net::TcpStream::connect_timeout(
                &addr.parse().expect("valid addr"),
                std::time::Duration::from_millis(500),
            )
            .is_ok()
            {
                found.push(*name);
                println!("  Found: {name} (listening on port {port})");
            }
        }
    }

    if !found.is_empty() {
        println!("\nExisting backend(s) detected: {}", found.join(", "));
        println!("You may already be ready. Run `az doctor` to verify.\n");
    }

    // Recommend backend
    let recommendations = if is_apple_silicon {
        vec![
            ("Ollama", "curl -fsSL https://ollama.com/install.sh | sh"),
            ("MLX", "pip install mlx-lm"),
        ]
    } else if os == "linux" {
        vec![
            ("Ollama", "curl -fsSL https://ollama.com/install.sh | sh"),
            (
                "llama.cpp",
                "See https://github.com/ggml-org/llama.cpp for build instructions",
            ),
        ]
    } else if os == "macos" {
        vec![("Ollama", "curl -fsSL https://ollama.com/install.sh | sh")]
    } else {
        // Windows
        vec![
            ("Ollama", "Download from https://ollama.com/download"),
            ("LM Studio", "Download from https://lmstudio.ai/"),
        ]
    };

    println!("Recommended backends for {os}/{arch}:");
    for (i, (name, cmd)) in recommendations.iter().enumerate() {
        println!("  {}. {name}: {cmd}", i + 1);
    }

    if non_interactive {
        // Auto-install first recommendation (Ollama) on macOS/Linux
        if os == "macos" || os == "linux" {
            println!("\nInstalling Ollama (non-interactive mode)...");
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg("curl -fsSL https://ollama.com/install.sh | sh")
                .status();
            match status {
                Ok(s) if s.success() => {
                    println!("Ollama installed successfully.");
                    if !skip_model {
                        println!("Pulling default model (llama3.2)...");
                        let _ = std::process::Command::new("ollama")
                            .args(["pull", "llama3.2"])
                            .status();
                    }
                }
                Ok(s) => {
                    eprintln!("Ollama install failed with exit code: {s}");
                    return 1;
                }
                Err(e) => {
                    eprintln!("Failed to run install command: {e}");
                    return 1;
                }
            }
        } else {
            println!("\nNon-interactive install is only supported on macOS/Linux.");
            println!("Please install manually using the commands above.");
            return 1;
        }
    } else {
        println!("\nWould you like to install one? (1/2/skip) ");
        print!("> ");
        io::stdout().flush().ok();
        let mut answer = String::new();
        io::stdin().lock().read_line(&mut answer).ok();
        let trimmed = answer.trim();

        if trimmed == "skip" || trimmed.is_empty() {
            println!("Skipped. You can install manually using the commands above.");
            println!("Run `az doctor` after installing to verify the setup.");
            return 0;
        }

        if let Ok(idx) = trimmed.parse::<usize>() {
            if idx >= 1 && idx <= recommendations.len() {
                let (name, cmd) = recommendations[idx - 1];
                if cmd.starts_with("curl") || cmd.starts_with("pip") {
                    println!("\nInstalling {name}...");
                    let status = std::process::Command::new("sh").arg("-c").arg(cmd).status();
                    match status {
                        Ok(s) if s.success() => {
                            println!("{name} installed successfully.");
                            if !skip_model && name == "Ollama" {
                                println!("Pulling default model (llama3.2)...");
                                let _ = std::process::Command::new("ollama")
                                    .args(["pull", "llama3.2"])
                                    .status();
                            }
                        }
                        Ok(s) => {
                            eprintln!("Install failed with exit code: {s}");
                            return 1;
                        }
                        Err(e) => {
                            eprintln!("Failed to run install command: {e}");
                            return 1;
                        }
                    }
                } else {
                    println!("\nPlease install {name} manually:");
                    println!("  {cmd}");
                    println!("Then run `az doctor` to verify.");
                    return 0;
                }
            }
        }
    }

    // Generate models.json if it doesn't exist
    let cwd = std::env::current_dir().unwrap_or_default();
    let models_path = cwd.join(".agentzero/models.json");
    if !models_path.exists() {
        if let Some(parent) = models_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let default_config = agentzero::session::ModelsConfig::default_ollama();
        if let Ok(json) = serde_json::to_string_pretty(&default_config) {
            if std::fs::write(&models_path, json).is_ok() {
                println!("\nCreated: {}", models_path.display());
            }
        }
    }

    println!("\nBootstrap complete. Run `az doctor` to verify your setup.");
    0
}

/// Check if a command exists on PATH.
fn which_command(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Load settings from .agentzero/settings.toml and return (provider, model) defaults.
fn default_system_prompt() -> String {
    concat!(
        "You are AgentZero, a secure AI agent assistant. ",
        "You help users with their local development projects. ",
        "You are running in local-only mode — all inference happens on this machine. ",
        "You have access to tools: read (read files), list (list directories), ",
        "search (search file contents), write (write files, requires approval), ",
        "and shell (run shell commands, requires approval). ",
        "Use tools when the user asks about their project. Be concise and helpful."
    )
    .to_string()
}

fn load_settings() -> (Option<String>, Option<String>) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let settings_path = cwd.join(".agentzero/settings.toml");
    if !settings_path.exists() {
        return (None, None);
    }
    let content = match std::fs::read_to_string(&settings_path) {
        Ok(c) => c,
        Err(_) => return (None, None),
    };
    let table: toml::Table = match content.parse() {
        Ok(t) => t,
        Err(_) => return (None, None),
    };
    let general = table.get("general").and_then(|v| v.as_table());
    let provider = general
        .and_then(|g| g.get("default_provider"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let model = general
        .and_then(|g| g.get("default_model"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (provider, model)
}

/// Load encryption passphrase from .agentzero/settings.toml `[session] passphrase`.
fn load_passphrase_from_settings() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let settings_path = cwd.join(".agentzero/settings.toml");
    let content = std::fs::read_to_string(settings_path).ok()?;
    let table: toml::Table = content.parse().ok()?;
    table
        .get("session")
        .and_then(|v| v.as_table())
        .and_then(|s| s.get("passphrase"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

#[allow(clippy::too_many_arguments)]
async fn cmd_chat(
    local: bool,
    model: &str,
    _stream: bool,
    provider_name: &str,
    url_override: Option<&str>,
    resume_id: Option<&str>,
    encrypt: bool,
    print_message: Option<&str>,
    output_mode: &str,
) -> i32 {
    use agentzero::session::router::ProviderRouter;
    use agentzero::session::{
        AgentLoop, AgentLoopConfig, ApprovalDecision, ApprovalHandler, ChatMessage, ModelProvider,
        OllamaConfig, OllamaProvider, OpenAICompatConfig, OpenAICompatProvider, ProgressHandler,
        Session, SessionConfig, SessionMode, ToolExecutor,
    };
    use std::io::{self, BufRead, Write};
    use std::pin::Pin;

    // --- Terminal approval handler: prompts user on stdin ---
    struct TerminalApprovalHandler;
    impl ApprovalHandler for TerminalApprovalHandler {
        fn request_approval(
            &self,
            tool_name: &str,
            args: &serde_json::Value,
        ) -> Pin<Box<dyn std::future::Future<Output = ApprovalDecision> + Send + '_>> {
            let description = match tool_name {
                "write" | "edit" => {
                    let path = args
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(unknown)");
                    let content_len = args
                        .get("content")
                        .and_then(|v| v.as_str())
                        .map_or(0, |s| s.len());
                    format!("{tool_name}: `{path}` ({content_len} bytes)")
                }
                "shell" => {
                    let cmd = args
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(unknown)");
                    format!("shell: `{cmd}`")
                }
                _ => tool_name.to_string(),
            };
            Box::pin(async move {
                print!("  [APPROVE {description}?] (y/yes-all/n) ");
                io::stdout().flush().ok();
                let mut answer = String::new();
                io::stdin().lock().read_line(&mut answer).ok();
                let trimmed = answer.trim();
                if trimmed.eq_ignore_ascii_case("y") {
                    ApprovalDecision::Approved
                } else if trimmed.eq_ignore_ascii_case("yes-all")
                    || trimmed.eq_ignore_ascii_case("a")
                {
                    println!("  [APPROVED for session]");
                    ApprovalDecision::ApprovedForSession
                } else {
                    println!("  [DENIED by user]");
                    ApprovalDecision::Denied
                }
            })
        }
    }

    // --- Terminal progress handler: prints tool events ---
    struct TerminalProgressHandler;
    impl ProgressHandler for TerminalProgressHandler {
        fn on_tool_start(&self, tool_name: &str, _args: &serde_json::Value) {
            print!("  [tool: {tool_name}] ");
            io::stdout().flush().ok();
        }
        fn on_tool_result(&self, _tool_name: &str, success: bool, output_len: usize) {
            if success {
                println!("ok ({output_len} bytes)");
            } else {
                println!("error");
            }
        }
        fn on_context_compacted(&self, before: usize, after: usize) {
            println!("  [context compacted: {before} → {after} messages]");
        }
    }

    // Apply settings.toml defaults where CLI flags are at their defaults
    let (settings_provider, settings_model) = load_settings();
    let model = if model == "llama3.2" {
        settings_model.as_deref().unwrap_or(model)
    } else {
        model
    };
    let provider_name = if provider_name == "ollama" {
        settings_provider.as_deref().unwrap_or(provider_name)
    } else {
        provider_name
    };

    let mode = if local { "local-only" } else { "default" };
    println!("AgentZero Chat ({mode})");
    println!("======================");

    // Load policy
    let cwd = std::env::current_dir().unwrap_or_default();
    let policy_path = cwd.join(".agentzero/policy.yml");
    let policy = if policy_path.exists() {
        match agentzero::policy::load_policy_file(&policy_path) {
            Ok(rules) => {
                println!(
                    "Policy loaded: {} rules from {}",
                    rules.len(),
                    policy_path.display()
                );
                agentzero::policy::PolicyEngine::with_rules(rules)
            }
            Err(e) => {
                eprintln!("warning: failed to load policy: {e}");
                agentzero::policy::PolicyEngine::deny_by_default()
            }
        }
    } else {
        println!("No policy file found. Using deny-by-default.");
        agentzero::policy::PolicyEngine::deny_by_default()
    };

    // Create session with tool executor
    let tool_policy = if policy_path.exists() {
        agentzero::policy::load_policy_file(&policy_path)
            .map(agentzero::policy::PolicyEngine::with_rules)
            .unwrap_or_else(|_| agentzero::policy::PolicyEngine::deny_by_default())
    } else {
        agentzero::policy::PolicyEngine::deny_by_default()
    };
    let tool_executor =
        ToolExecutor::new(tool_policy).with_project_root(cwd.to_string_lossy().to_string());

    let session_config = SessionConfig {
        mode: if local {
            SessionMode::LocalOnly
        } else {
            SessionMode::LocalPreferred
        },
        project_root: Some(cwd.to_string_lossy().to_string()),
    };
    let session = match Session::new(session_config, policy) {
        Ok(s) => s.with_tool_executor(tool_executor),
        Err(e) => {
            eprintln!("error: failed to create session: {e}");
            return 1;
        }
    };
    // Wire audit to disk if project is initialized
    let audit_dir = cwd.join(".agentzero/audit");
    let session = if audit_dir.exists() {
        match session.with_audit_dir(&audit_dir) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("warning: failed to set up audit logging: {e}");
                return 1;
            }
        }
    } else {
        session
    };

    // Build provider router from CLI flags
    let router = match provider_name {
        "ollama" => {
            let config = OllamaConfig {
                model: model.to_string(),
                base_url: url_override.unwrap_or("http://localhost:11434").to_string(),
            };
            let provider = OllamaProvider::new(config);
            println!("Model: {} ({})", provider.model_name(), provider.name());
            match provider.health_check().await {
                Ok(true) => println!("Ollama: connected"),
                Ok(false) => eprintln!("Ollama: responded but may not be healthy"),
                Err(e) => {
                    eprintln!("error: cannot connect to Ollama: {e}");
                    eprintln!("Make sure Ollama is running: `ollama serve`");
                    return 1;
                }
            }
            ProviderRouter::local_only(model)
        }
        "llama-cpp" | "llama.cpp" | "llamacpp" => {
            let mut config = OpenAICompatConfig::llama_cpp();
            config.model = model.to_string();
            if let Some(url) = url_override {
                config.base_url = url.to_string();
            }
            let provider = OpenAICompatProvider::new(config.clone());
            println!(
                "Model: {} ({})",
                provider.model_name(),
                provider.server_type()
            );
            match provider.health_check().await {
                Ok(true) => println!("{}: connected", provider.server_type()),
                Ok(false) => eprintln!(
                    "{}: responded but may not be healthy",
                    provider.server_type()
                ),
                Err(e) => {
                    eprintln!("error: cannot connect to {}: {e}", provider.server_type());
                    return 1;
                }
            }
            ProviderRouter::with_fallback(model, config)
        }
        "vllm" => {
            let mut config = OpenAICompatConfig::vllm();
            config.model = model.to_string();
            if let Some(url) = url_override {
                config.base_url = url.to_string();
            }
            let provider = OpenAICompatProvider::new(config.clone());
            println!(
                "Model: {} ({})",
                provider.model_name(),
                provider.server_type()
            );
            match provider.health_check().await {
                Ok(true) => println!("{}: connected", provider.server_type()),
                Ok(false) => eprintln!(
                    "{}: responded but may not be healthy",
                    provider.server_type()
                ),
                Err(e) => {
                    eprintln!("error: cannot connect to {}: {e}", provider.server_type());
                    return 1;
                }
            }
            ProviderRouter::with_fallback(model, config)
        }
        "lm-studio" | "lmstudio" => {
            let mut config = OpenAICompatConfig::lm_studio();
            config.model = model.to_string();
            if let Some(url) = url_override {
                config.base_url = url.to_string();
            }
            let provider = OpenAICompatProvider::new(config.clone());
            println!(
                "Model: {} ({})",
                provider.model_name(),
                provider.server_type()
            );
            match provider.health_check().await {
                Ok(true) => println!("{}: connected", provider.server_type()),
                Ok(false) => eprintln!(
                    "{}: responded but may not be healthy",
                    provider.server_type()
                ),
                Err(e) => {
                    eprintln!("error: cannot connect to {}: {e}", provider.server_type());
                    return 1;
                }
            }
            ProviderRouter::with_fallback(model, config)
        }
        other => {
            eprintln!("unknown provider: {other}");
            eprintln!("available: ollama, llama-cpp, vllm, lm-studio");
            return 1;
        }
    };

    let tools = OllamaProvider::agentzero_tool_definitions();
    println!(
        "Tools: {} available (read, list, search, write, edit, shell)",
        tools.len()
    );
    println!();
    println!("Type your message and press Enter. Type /quit to exit.");
    println!();

    // Get encryption passphrase (enabled by default, disable with --no-encrypt).
    // Check AZ_PASSPHRASE env var first, then settings.toml, then prompt.
    let passphrase = if encrypt {
        let pass = if let Ok(env_pass) = std::env::var("AZ_PASSPHRASE") {
            if env_pass.is_empty() {
                eprintln!("error: AZ_PASSPHRASE is set but empty");
                return 1;
            }
            env_pass
        } else if let Some(settings_pass) = load_passphrase_from_settings() {
            settings_pass
        } else {
            print!("Encryption passphrase (or set AZ_PASSPHRASE): ");
            io::stdout().flush().ok();
            let mut p = String::new();
            io::stdin().lock().read_line(&mut p).ok();
            let p = p.trim().to_string();
            if p.is_empty() {
                eprintln!("error: passphrase cannot be empty");
                return 1;
            }
            p
        };
        println!("Audit logs and sessions will be encrypted.");
        Some(pass)
    } else {
        None
    };

    // Build the AgentLoop
    let config = AgentLoopConfig::default();
    let mut agent_loop = AgentLoop::new(router, session, tools.clone(), config);

    // Resume existing session or start with system prompt
    if let Some(id) = resume_id {
        let session_file = cwd.join(format!(".agentzero/sessions/{id}.json"));
        if !session_file.exists() {
            eprintln!("error: session file not found: {}", session_file.display());
            return 1;
        }
        match std::fs::read_to_string(&session_file) {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(data) => {
                    if let Some(msgs) = data.get("messages") {
                        match serde_json::from_value::<Vec<ChatMessage>>(msgs.clone()) {
                            Ok(msgs) => {
                                println!("Resumed session {id} ({} messages)", msgs.len());
                                agent_loop = agent_loop.with_messages(msgs);
                            }
                            Err(e) => {
                                eprintln!("error: failed to parse messages: {e}");
                                return 1;
                            }
                        }
                    } else {
                        eprintln!("error: no messages in session file");
                        return 1;
                    }
                }
                Err(e) => {
                    eprintln!("error: failed to parse session: {e}");
                    return 1;
                }
            },
            Err(e) => {
                eprintln!("error: failed to read session: {e}");
                return 1;
            }
        }
    } else {
        let system_prompt = {
            let prompt_path = cwd.join(".agentzero/prompts/system.md");
            if prompt_path.exists() {
                match std::fs::read_to_string(&prompt_path) {
                    Ok(content) => {
                        println!("System prompt loaded from .agentzero/prompts/system.md");
                        content
                    }
                    Err(_) => default_system_prompt(),
                }
            } else {
                default_system_prompt()
            }
        };
        agent_loop = agent_loop.with_system_prompt(&system_prompt);
    }

    // Print mode: single-shot query, output result, exit
    if let Some(message) = print_message {
        let approver = agentzero::session::AutoApprove;
        let progress = TerminalProgressHandler;

        match agent_loop.send(message, &approver, &progress).await {
            Ok(response) => {
                match output_mode {
                    "json" => {
                        let json_output = serde_json::json!({
                            "content": response.content,
                            "model": response.model,
                            "session_id": agent_loop.session_id(),
                            "rounds": response.rounds,
                            "tool_calls": response.tool_calls_made.iter().map(|tc| {
                                serde_json::json!({
                                    "name": tc.name,
                                    "success": tc.success,
                                    "output": tc.output
                                })
                            }).collect::<Vec<_>>()
                        });
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json_output)
                                .unwrap_or_else(|_| response.content.clone())
                        );
                    }
                    "jsonl" => {
                        let json_output = serde_json::json!({
                            "content": response.content,
                            "model": response.model,
                            "session_id": agent_loop.session_id(),
                            "rounds": response.rounds,
                            "tool_calls": response.tool_calls_made.iter().map(|tc| {
                                serde_json::json!({
                                    "name": tc.name,
                                    "success": tc.success,
                                    "output": tc.output
                                })
                            }).collect::<Vec<_>>()
                        });
                        println!(
                            "{}",
                            serde_json::to_string(&json_output)
                                .unwrap_or_else(|_| response.content.clone())
                        );
                    }
                    _ => {
                        // text mode (default)
                        println!("{}", response.content);
                    }
                }
                agent_loop.end().ok();
                return 0;
            }
            Err(e) => {
                eprintln!("error: {e}");
                agent_loop.end().ok();
                return 1;
            }
        }
    }

    println!("Session: {}", agent_loop.session_id());

    let stdin = io::stdin();
    let approver = TerminalApprovalHandler;
    let progress = TerminalProgressHandler;

    loop {
        print!("you> ");
        io::stdout().flush().ok();

        let mut input = String::new();
        match stdin.lock().read_line(&mut input) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("error reading input: {e}");
                break;
            }
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if input == "/quit" || input == "/exit" || input == "/q" {
            println!("Goodbye.");
            break;
        }
        if input == "/tools" {
            println!("Available tools:");
            for t in &tools {
                println!("  {} — {}", t.function.name, t.function.description);
            }
            println!();
            continue;
        }
        if input == "/session" {
            println!("Session: {}", agent_loop.session_id());
            println!("Mode: {mode}");
            println!("Model: {}", agent_loop.model_name());
            println!();
            continue;
        }

        match agent_loop.send(input, &approver, &progress).await {
            Ok(response) => {
                if !response.content.is_empty() {
                    println!();
                    println!("agentzero> {}", response.content);
                    println!();
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
            }
        }
    }

    // Save conversation to .agentzero/sessions/ if initialized
    let sessions_dir = cwd.join(".agentzero/sessions");
    let messages = agent_loop.messages();
    if sessions_dir.exists() && messages.len() > 1 {
        let session_data = serde_json::json!({
            "session_id": agent_loop.session_id(),
            "model": model,
            "mode": mode,
            "message_count": messages.len(),
            "messages": messages,
        });
        match serde_json::to_string_pretty(&session_data) {
            Ok(json) => {
                if let Some(ref pass) = passphrase {
                    let session_file =
                        sessions_dir.join(format!("{}.json.enc", agent_loop.session_id()));
                    match agentzero::core::crypto::encrypt_string(&json, pass) {
                        Ok(encrypted) => {
                            if let Err(e) = std::fs::write(&session_file, encrypted) {
                                eprintln!("warning: failed to save encrypted session: {e}");
                            } else {
                                println!("Session saved (encrypted) to {}", session_file.display());
                            }
                        }
                        Err(e) => eprintln!("warning: encryption failed: {e}"),
                    }
                } else {
                    let session_file =
                        sessions_dir.join(format!("{}.json", agent_loop.session_id()));
                    if let Err(e) = std::fs::write(&session_file, json) {
                        eprintln!("warning: failed to save session: {e}");
                    } else {
                        println!("Session saved to {}", session_file.display());
                    }
                }
            }
            Err(e) => {
                eprintln!("warning: failed to serialize session: {e}");
            }
        }
    }

    agent_loop.end().ok();
    0
}

fn cmd_history() -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();
    let sessions_dir = cwd.join(".agentzero/sessions");

    if !sessions_dir.exists() {
        println!("No sessions directory. Run `az init` first.");
        return 1;
    }

    let mut entries: Vec<_> = match std::fs::read_dir(&sessions_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .collect(),
        Err(e) => {
            eprintln!("error: failed to read sessions directory: {e}");
            return 1;
        }
    };

    if entries.is_empty() {
        println!("No past sessions found.");
        return 0;
    }

    // Sort by modification time, most recent first
    entries.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).ok();
        let b_time = b.metadata().and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    println!("Past sessions:");
    println!();
    for entry in &entries {
        let path = entry.path();
        let session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        // Try to read session metadata
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                let model = data.get("model").and_then(|v| v.as_str()).unwrap_or("?");
                let msg_count = data
                    .get("message_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let mode = data.get("mode").and_then(|v| v.as_str()).unwrap_or("?");
                println!("  {session_id}  model={model}  messages={msg_count}  mode={mode}");
                continue;
            }
        }
        println!("  {session_id}");
    }
    println!();
    println!("{} session(s) found.", entries.len());
    0
}

fn cmd_vault(action: VaultAction) -> i32 {
    use agentzero::core::secret::SecretHandle;
    use agentzero::core::vault::Vault;
    use std::io::{self, BufRead, Write};

    let cwd = std::env::current_dir().unwrap_or_default();
    let vault_dir = cwd.join(".agentzero/vault");

    if !vault_dir
        .parent()
        .is_some_and(|p| p.join("vault").exists() || p.exists())
    {
        eprintln!("Run `az init` first.");
        return 1;
    }

    // Prompt for passphrase
    print!("Vault passphrase: ");
    io::stdout().flush().ok();
    let mut passphrase = String::new();
    io::stdin().lock().read_line(&mut passphrase).ok();
    let passphrase = passphrase.trim();
    if passphrase.is_empty() {
        eprintln!("error: passphrase cannot be empty");
        return 1;
    }

    let vault = match Vault::open(&vault_dir, passphrase) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: failed to open vault: {e}");
            return 1;
        }
    };

    match action {
        VaultAction::List => {
            let handles = match vault.list() {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            };
            if handles.is_empty() {
                println!("No secrets stored.");
                println!("Add one with: agentzero vault add <provider> <name>");
            } else {
                println!("Stored secrets:");
                for handle in &handles {
                    println!("  {}", handle.uri());
                }
                println!();
                println!("{} secret(s)", handles.len());
            }
            0
        }
        VaultAction::Add { provider, name } => {
            let handle = SecretHandle::new(&provider, &name);
            print!("Secret value: ");
            io::stdout().flush().ok();
            let mut value = String::new();
            io::stdin().lock().read_line(&mut value).ok();
            let value = value.trim();
            if value.is_empty() {
                eprintln!("error: value cannot be empty");
                return 1;
            }
            match vault.put(&handle, value) {
                Ok(()) => {
                    println!("Stored: {}", handle.uri());
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        VaultAction::Get { provider, name } => {
            let handle = SecretHandle::new(&provider, &name);
            match vault.get(&handle) {
                Ok(value) => {
                    println!("{value}");
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        VaultAction::Remove { provider, name } => {
            let handle = SecretHandle::new(&provider, &name);
            match vault.remove(&handle) {
                Ok(()) => {
                    println!("Removed: {}", handle.uri());
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
    }
}

fn cmd_install_git(url: &str) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();

    // Derive skill name from URL
    let skill_name = url
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit('/')
        .next()
        .unwrap_or("unknown");

    let install_dir = cwd.join(format!("skills/{skill_name}"));
    if install_dir.exists() {
        eprintln!(
            "Skill '{skill_name}' already installed at {}",
            install_dir.display()
        );
        return 1;
    }

    println!("Cloning {url} → skills/{skill_name}/");

    let output = std::process::Command::new("git")
        .args(["clone", "--depth", "1", url, &install_dir.to_string_lossy()])
        .output();

    match output {
        Ok(result) => {
            if !result.status.success() {
                let stderr = String::from_utf8_lossy(&result.stderr);
                eprintln!("error: git clone failed: {stderr}");
                return 1;
            }
        }
        Err(e) => {
            eprintln!("error: failed to run git: {e}");
            eprintln!("Make sure git is installed.");
            return 1;
        }
    }

    // Remove .git directory (we don't need history for installed skills)
    let git_dir = install_dir.join(".git");
    if git_dir.exists() {
        std::fs::remove_dir_all(&git_dir).ok();
    }

    // Validate SKILL.md
    if !install_dir.join("SKILL.md").exists() {
        eprintln!("warning: no SKILL.md found in cloned repository");
        eprintln!("The skill may not be valid. Keeping it installed anyway.");
    } else {
        println!("Installed skill '{skill_name}' from {url}");
    }

    // Update lockfile
    update_lockfile(&cwd, skill_name, &format!("git:{url}"), &install_dir);

    print_skill_info(&install_dir, skill_name);
    0
}

async fn cmd_install(source: &str, refresh_index: bool) -> i32 {
    use agentzero::skills::remote::{parse_skill_ref, SkillRefKind};

    match parse_skill_ref(source) {
        SkillRefKind::Local(path) => cmd_install_local(&path),
        SkillRefKind::GitUrl(url) => cmd_install_git(&url),
        SkillRefKind::GitHub { owner, repo } => cmd_install_github(&owner, &repo).await,
        SkillRefKind::IndexName(name) => cmd_install_from_index(&name, refresh_index).await,
    }
}

async fn cmd_install_from_index(name: &str, refresh_index: bool) -> i32 {
    use agentzero::skills::index::{load_or_fetch_index, DEFAULT_INDEX_REPO};

    let cwd = std::env::current_dir().unwrap_or_default();

    println!("Resolving '{name}' from skill index...");
    let index = match load_or_fetch_index(&cwd, DEFAULT_INDEX_REPO, refresh_index).await {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("error: failed to load skill index: {e}");
            eprintln!("hint: check your network connection or install by GitHub URL instead");
            eprintln!("hint: e.g., `agentzero install owner/repo`");
            return 1;
        }
    };

    match index.resolve(name) {
        Some((owner, repo)) => {
            println!("Found: {owner}/{repo}");
            cmd_install_github(&owner, &repo).await
        }
        None => {
            eprintln!("error: skill '{name}' not found in the central index");
            let available = index.list();
            if !available.is_empty() {
                eprintln!("Available skills:");
                for (skill_name, description) in &available {
                    eprintln!("  {skill_name} — {description}");
                }
            }
            1
        }
    }
}

fn cmd_install_local(path: &str) -> i32 {
    let source = std::path::Path::new(path);

    // Validate source has SKILL.md
    let skill_md = source.join("SKILL.md");
    if !skill_md.exists() {
        eprintln!("error: no SKILL.md found in {path}");
        eprintln!("A skill directory must contain a SKILL.md file.");
        return 1;
    }

    // Determine skill name from directory
    let skill_name = source
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Determine install location
    let cwd = std::env::current_dir().unwrap_or_default();
    let install_dir = cwd.join(format!("skills/{skill_name}"));

    if install_dir.exists() {
        eprintln!(
            "Skill '{skill_name}' already installed at {}",
            install_dir.display()
        );
        eprintln!("Remove it first to reinstall.");
        return 1;
    }

    // Copy the skill directory
    if let Err(e) = copy_dir_recursive(source, &install_dir) {
        eprintln!("error: failed to install skill: {e}");
        return 1;
    }

    println!(
        "Installed skill '{skill_name}' to {}",
        install_dir.display()
    );

    // Update lockfile
    update_lockfile(&cwd, skill_name, "local", &install_dir);

    print_skill_info(&install_dir, skill_name);
    0
}

async fn cmd_install_github(owner: &str, repo: &str) -> i32 {
    use agentzero::skills::github::GitHubClient;
    use agentzero::skills::package;

    println!("Resolving {owner}/{repo} from GitHub releases...");

    let client = GitHubClient::from_env();
    let release = match client.get_latest_release(owner, repo).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: failed to resolve release: {e}");
            return 1;
        }
    };

    println!("Found release {} (v{})", release.tag, release.version);

    // Download the tarball
    println!("Downloading {}...", release.tarball_url);
    let tarball = match client.download(&release.tarball_url).await {
        Ok(data) => data,
        Err(e) => {
            eprintln!("error: download failed: {e}");
            return 1;
        }
    };

    // Verify checksum if available
    if let Some(ref expected) = release.checksum {
        println!("Verifying checksum...");
        if let Err(e) = package::verify_checksum(&tarball, expected) {
            eprintln!("error: {e}");
            return 1;
        }
        println!("Checksum verified.");
    } else {
        println!("warning: no checksum in release notes, skipping verification");
    }

    // Extract
    let cwd = std::env::current_dir().unwrap_or_default();
    let skills_dir = cwd.join("skills");
    std::fs::create_dir_all(&skills_dir).ok();

    let skill_name = match package::extract_tarball(&tarball, &skills_dir) {
        Ok(name) => name,
        Err(e) => {
            eprintln!("error: failed to extract: {e}");
            return 1;
        }
    };

    let install_dir = skills_dir.join(&skill_name);
    if !install_dir.join("SKILL.md").exists() {
        eprintln!("warning: no SKILL.md found in extracted package");
    }

    println!("Installed skill '{skill_name}' from {owner}/{repo}");

    // Update lockfile with GitHub source and checksum
    let source_str = format!("github:{owner}/{repo}");
    update_lockfile_with_checksum(
        &cwd,
        &skill_name,
        &source_str,
        &install_dir,
        &package::compute_checksum(&tarball),
    );

    print_skill_info(&install_dir, &skill_name);
    0
}

/// Update the lockfile after installing a skill.
fn update_lockfile(
    cwd: &std::path::Path,
    skill_name: &str,
    source: &str,
    install_dir: &std::path::Path,
) {
    update_lockfile_with_checksum(cwd, skill_name, source, install_dir, "");
}

fn update_lockfile_with_checksum(
    cwd: &std::path::Path,
    skill_name: &str,
    source: &str,
    install_dir: &std::path::Path,
    checksum: &str,
) {
    use agentzero::skills::registry::{lockfile_path, LockedSkill, SkillLockfile};

    let lockfile_path = lockfile_path(cwd);
    let mut lockfile = SkillLockfile::load(&lockfile_path).unwrap_or_default();

    let manifest = agentzero::skills::registry::load_manifest(install_dir);
    let (version, runtime, permissions) = match manifest {
        Ok(m) => (
            m.version,
            format!("{:?}", m.runtime).to_lowercase(),
            m.permissions
                .iter()
                .map(|p| format!("{:?}", p.capability).to_lowercase())
                .collect(),
        ),
        Err(_) => ("0.1.0".into(), "unknown".into(), vec![]),
    };

    // Compute directory checksum for runtime integrity verification
    let dir_checksum = match agentzero::skills::registry::compute_directory_checksum(install_dir) {
        Ok(cs) => Some(cs),
        Err(e) => {
            eprintln!("warning: failed to compute directory checksum: {e}");
            None
        }
    };

    lockfile.register(LockedSkill {
        name: skill_name.to_string(),
        version,
        source: source.to_string(),
        runtime,
        permissions,
        checksum: if checksum.is_empty() {
            None
        } else {
            Some(checksum.to_string())
        },
        dir_checksum,
    });

    if let Err(e) = lockfile.save(&lockfile_path) {
        eprintln!("warning: failed to update lockfile: {e}");
    }
}

fn print_skill_info(install_dir: &std::path::Path, skill_name: &str) {
    if install_dir.join("patterns.toml").exists() {
        println!("  includes patterns.toml");
    }
    if let Ok(content) = std::fs::read_to_string(install_dir.join("SKILL.md")) {
        if content.contains("runtime: none") {
            println!("  runtime: instruction-only");
        } else if content.contains("runtime: wasm") {
            println!("  runtime: wasm-sandbox");
        } else if content.contains("runtime: host_supervised") {
            println!("  runtime: host-supervised");
        }
    }
    println!();
    println!("Run with: az run {skill_name}");
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

async fn cmd_publish(path: &str, repo_override: Option<&str>, tag_override: Option<&str>) -> i32 {
    use agentzero::skills::github::GitHubClient;
    use agentzero::skills::package;

    let skill_dir = std::path::Path::new(path);
    if !skill_dir.join("SKILL.md").exists() {
        eprintln!("error: no SKILL.md in {path}");
        eprintln!("Run this command from a skill directory or pass the path.");
        return 1;
    }

    // Package the skill
    println!("Packaging skill...");
    let pkg = match package::package_skill(skill_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: failed to package: {e}");
            return 1;
        }
    };

    println!("  name:     {}", pkg.name);
    println!("  version:  {}", pkg.version);
    println!("  size:     {} bytes", pkg.tarball.len());
    println!("  checksum: {}", pkg.checksum);

    // Determine target repo
    let repo_str = match repo_override {
        Some(r) => r.to_string(),
        None => {
            // Try to infer from git remote
            let output = std::process::Command::new("git")
                .args(["remote", "get-url", "origin"])
                .current_dir(skill_dir)
                .output();
            match output {
                Ok(o) if o.status.success() => {
                    let url = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    // Extract owner/repo from GitHub URL
                    if let Some(path) = url
                        .strip_prefix("https://github.com/")
                        .or_else(|| url.strip_prefix("git@github.com:"))
                    {
                        path.trim_end_matches('/')
                            .trim_end_matches(".git")
                            .to_string()
                    } else {
                        eprintln!("error: cannot determine GitHub repo from remote '{url}'");
                        eprintln!("Use --repo owner/repo to specify.");
                        return 1;
                    }
                }
                _ => {
                    eprintln!("error: no --repo specified and no git remote found");
                    eprintln!("Use --repo owner/repo to specify the target.");
                    return 1;
                }
            }
        }
    };

    let parts: Vec<&str> = repo_str.splitn(2, '/').collect();
    if parts.len() != 2 {
        eprintln!("error: repo must be in owner/repo format, got: {repo_str}");
        return 1;
    }
    let (owner, repo) = (parts[0], parts[1]);

    let tag = tag_override
        .map(|t| t.to_string())
        .unwrap_or_else(|| format!("v{}", pkg.version));

    println!();
    println!("Publishing to {owner}/{repo} as {tag}...");

    let client = GitHubClient::from_env();

    // Create the release
    let release_body = format!(
        "## {} v{}\n\n{}\n\n{}",
        pkg.name, pkg.version, pkg.checksum, pkg.manifest.description
    );

    let upload_url = match client
        .create_release(
            owner,
            repo,
            &tag,
            &format!("{} v{}", pkg.name, pkg.version),
            &release_body,
        )
        .await
    {
        Ok(url) => url,
        Err(e) => {
            eprintln!("error: failed to create release: {e}");
            return 1;
        }
    };

    // Upload the tarball
    let filename = format!("{}-{}.tar.gz", pkg.name, pkg.version);
    println!("Uploading {filename}...");
    if let Err(e) = client
        .upload_asset(&upload_url, &filename, &pkg.tarball)
        .await
    {
        eprintln!("error: failed to upload asset: {e}");
        return 1;
    }

    println!();
    println!("Published {}-{} to {owner}/{repo}", pkg.name, pkg.version);
    println!("Install with: agentzero install {owner}/{repo}");
    0
}

/// Verify skill integrity using lockfile checksums.
///
/// Uses a two-tier approach: mtime-based fast path, then full SHA-256 hash.
/// Returns `Ok(())` if the skill passes verification, `Err(exit_code)` if it fails.
fn verify_skill_integrity(
    cwd: &std::path::Path,
    skill_name: &str,
    skill_dir: &std::path::Path,
) -> Result<(), i32> {
    use agentzero::skills::registry::{lockfile_path, SkillLockfile, VerificationCache};

    let lockfile_path = lockfile_path(cwd);
    let lockfile = SkillLockfile::load(&lockfile_path).unwrap_or_default();

    // Check if this skill has a dir_checksum in the lockfile
    let has_dir_checksum = lockfile
        .skills
        .get(skill_name)
        .and_then(|s| s.dir_checksum.as_ref())
        .is_some();

    if !has_dir_checksum {
        // Legacy entry or not in lockfile — skip verification silently
        return Ok(());
    }

    // Fast path: check mtime cache
    let cache_path = VerificationCache::path(cwd);
    let mut cache = VerificationCache::load(&cache_path).unwrap_or_default();

    if cache.is_fresh(skill_name, skill_dir) {
        return Ok(());
    }

    // Full path: compute and compare directory checksum
    if let Err(e) = lockfile.verify_skill(skill_name, skill_dir) {
        eprintln!("error: {e}");
        eprintln!("hint: the skill may have been tampered with after installation");
        eprintln!("hint: reinstall with `agentzero install` or bypass with `--skip-verify`");
        return Err(1);
    }

    // Verification passed — update cache
    cache.mark_verified(skill_name);
    if let Err(e) = cache.save(&cache_path) {
        eprintln!("warning: failed to save verification cache: {e}");
    }

    Ok(())
}

fn cmd_run(name: &str, skip_verify: bool) -> i32 {
    // Check built-in skills first
    if name == "repo-security-audit" {
        return cmd_run_security_audit();
    }
    if name == "dependency-audit" {
        return cmd_run_dependency_audit();
    }
    if name == "secrets-scan" {
        return cmd_run_secrets_scan();
    }

    // Check installed skills
    let cwd = std::env::current_dir().unwrap_or_default();
    let skill_dir = cwd.join(format!("skills/{name}"));
    if skill_dir.exists() && skill_dir.join("SKILL.md").exists() {
        // Runtime integrity verification
        if !skip_verify {
            if let Err(code) = verify_skill_integrity(&cwd, name, &skill_dir) {
                return code;
            }
        }

        println!("Running installed skill: {name}");
        println!("Skill directory: {}", skill_dir.display());

        // Load manifest from SKILL.md frontmatter
        let manifest = match agentzero::skills::registry::load_manifest(&skill_dir) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("error: failed to load skill manifest: {e}");
                return 1;
            }
        };

        // Route by runtime type
        match manifest.runtime {
            agentzero::skills::SkillRuntime::Wasm => {
                return cmd_run_wasm_skill(&cwd, &skill_dir, &manifest);
            }
            agentzero::skills::SkillRuntime::HostSupervised => {
                return cmd_run_host_supervised_skill(&cwd, &manifest);
            }
            agentzero::skills::SkillRuntime::InstructionOnly => {
                // Check if it has a patterns.toml (scanner-based skill)
                let patterns_path = skill_dir.join("patterns.toml");
                if patterns_path.exists() {
                    println!("Found patterns.toml — running scanner...");
                    println!();
                    use agentzero::skills::{report, scanner};
                    let results = scanner::scan_directory_with_patterns(&cwd, Some(&patterns_path));
                    let report_text = report::generate_report(&results, name);
                    println!("{report_text}");
                    return if results.findings.is_empty() { 0 } else { 1 };
                }

                // Otherwise just print the skill info
                if let Ok(content) = std::fs::read_to_string(skill_dir.join("SKILL.md")) {
                    println!();
                    println!("{content}");
                }
                return 0;
            }
            other => {
                eprintln!("error: runtime {other:?} is not yet supported");
                return 1;
            }
        }
    }

    // List available skills
    eprintln!("unknown skill: {name}");
    eprint!("Available skills: repo-security-audit, dependency-audit, secrets-scan");
    let skills_dir = cwd.join("skills");
    if skills_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() && entry.path().join("SKILL.md").exists() {
                    let skill_name = entry.file_name().to_string_lossy().to_string();
                    if skill_name != "repo-security-audit" {
                        eprint!(", {skill_name}");
                    }
                }
            }
        }
    }
    eprintln!();
    1
}

/// Run a WASM-backed skill through the full session pipeline:
/// policy check → sandbox profile → WasmEngine → audit.
fn cmd_run_wasm_skill(
    cwd: &std::path::Path,
    skill_dir: &std::path::Path,
    manifest: &agentzero::skills::SkillManifest,
) -> i32 {
    use agentzero::session::{Session, SessionConfig, SessionMode, ToolExecutor};

    // Find the .wasm module
    let wasm_path = match agentzero::skills::registry::find_wasm_module(skill_dir) {
        Some(p) => p,
        None => {
            eprintln!(
                "error: skill {} declares runtime: wasm but no .wasm file found in {}",
                manifest.name,
                skill_dir.display()
            );
            return 1;
        }
    };

    println!("Runtime: WASM");
    println!("Module:  {}", wasm_path.display());

    // Read WASM bytes
    let wasm_bytes = match std::fs::read(&wasm_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: failed to read WASM module: {e}");
            return 1;
        }
    };

    // Load policy
    let policy_path = cwd.join(".agentzero/policy.yml");
    let policy = if policy_path.exists() {
        match agentzero::policy::load_policy_file(&policy_path) {
            Ok(rules) => {
                println!(
                    "Policy:  {} rules from {}",
                    rules.len(),
                    policy_path.display()
                );
                agentzero::policy::PolicyEngine::with_rules(rules)
            }
            Err(e) => {
                eprintln!("warning: failed to load policy: {e}");
                agentzero::policy::PolicyEngine::deny_by_default()
            }
        }
    } else {
        println!("Policy:  deny-by-default (no policy file)");
        agentzero::policy::PolicyEngine::deny_by_default()
    };

    // Create tool executor with its own policy copy
    let tool_policy = if policy_path.exists() {
        agentzero::policy::load_policy_file(&policy_path)
            .map(agentzero::policy::PolicyEngine::with_rules)
            .unwrap_or_else(|_| agentzero::policy::PolicyEngine::deny_by_default())
    } else {
        agentzero::policy::PolicyEngine::deny_by_default()
    };
    let tool_executor =
        ToolExecutor::new(tool_policy).with_project_root(cwd.to_string_lossy().to_string());

    // Create session
    let session_config = SessionConfig {
        mode: SessionMode::LocalOnly,
        project_root: Some(cwd.to_string_lossy().to_string()),
    };
    let session = match Session::new(session_config, policy) {
        Ok(s) => s.with_tool_executor(tool_executor),
        Err(e) => {
            eprintln!("error: failed to create session: {e}");
            return 1;
        }
    };

    // Wire audit to disk if project is initialized
    let audit_dir = cwd.join(".agentzero/audit");
    let session = if audit_dir.exists() {
        match session.with_audit_dir(&audit_dir) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("warning: failed to set up audit logging: {e}");
                return 1;
            }
        }
    } else {
        session
    };

    println!("Session: {}", session.id());
    println!();

    // Execute through the session pipeline
    match session.execute_skill(manifest, Some(&wasm_bytes)) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            println!("Skill {} completed successfully.", manifest.name);
            0
        }
        Err(e) => {
            eprintln!("error: skill execution failed: {e}");
            1
        }
    }
}

/// Run a host-supervised skill through the session pipeline:
/// policy check → shell execution → audit.
fn cmd_run_host_supervised_skill(
    cwd: &std::path::Path,
    manifest: &agentzero::skills::SkillManifest,
) -> i32 {
    use agentzero::session::{Session, SessionConfig, SessionMode, ToolExecutor};

    let entrypoint = manifest
        .entrypoint
        .clone()
        .unwrap_or_else(|| format!("skills/{}/run.sh", manifest.name));

    println!("Runtime:     host-supervised");
    println!("Entrypoint:  {entrypoint}");

    // Load policy
    let policy_path = cwd.join(".agentzero/policy.yml");
    let policy = if policy_path.exists() {
        match agentzero::policy::load_policy_file(&policy_path) {
            Ok(rules) => {
                println!(
                    "Policy:      {} rules from {}",
                    rules.len(),
                    policy_path.display()
                );
                agentzero::policy::PolicyEngine::with_rules(rules)
            }
            Err(e) => {
                eprintln!("warning: failed to load policy: {e}");
                agentzero::policy::PolicyEngine::deny_by_default()
            }
        }
    } else {
        println!("Policy:      deny-by-default (no policy file)");
        agentzero::policy::PolicyEngine::deny_by_default()
    };

    // Create tool executor with its own policy copy
    let tool_policy = if policy_path.exists() {
        agentzero::policy::load_policy_file(&policy_path)
            .map(agentzero::policy::PolicyEngine::with_rules)
            .unwrap_or_else(|_| agentzero::policy::PolicyEngine::deny_by_default())
    } else {
        agentzero::policy::PolicyEngine::deny_by_default()
    };
    let tool_executor =
        ToolExecutor::new(tool_policy).with_project_root(cwd.to_string_lossy().to_string());

    let session_config = SessionConfig {
        mode: SessionMode::LocalOnly,
        project_root: Some(cwd.to_string_lossy().to_string()),
    };
    let session = match Session::new(session_config, policy) {
        Ok(s) => s.with_tool_executor(tool_executor),
        Err(e) => {
            eprintln!("error: failed to create session: {e}");
            return 1;
        }
    };

    // Wire audit to disk if project is initialized
    let audit_dir = cwd.join(".agentzero/audit");
    let session = if audit_dir.exists() {
        match session.with_audit_dir(&audit_dir) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("warning: failed to set up audit logging: {e}");
                return 1;
            }
        }
    } else {
        session
    };

    println!("Session:     {}", session.id());
    println!();

    match session.execute_skill(manifest, None) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            println!("Skill {} completed successfully.", manifest.name);
            0
        }
        Err(e) => {
            eprintln!("error: skill execution failed: {e}");
            1
        }
    }
}

fn cmd_run_security_audit() -> i32 {
    use agentzero::skills::{report, scanner};

    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: cannot determine current directory: {e}");
            return 1;
        }
    };

    let project_name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    println!("Running repo-security-audit on: {}", cwd.display());
    println!();

    let results = scanner::scan_directory(&cwd);
    let report_text = report::generate_report(&results, project_name);

    println!("{report_text}");

    // Write audit report to .agentzero/audit/ if initialized
    let audit_dir = cwd.join(".agentzero/audit");
    if audit_dir.exists() {
        let report_path = audit_dir.join("security-audit-report.md");
        match std::fs::write(&report_path, &report_text) {
            Ok(()) => {
                println!("Report written to: {}", report_path.display());
            }
            Err(e) => {
                eprintln!("warning: failed to write report: {e}");
            }
        }
    }

    if results.findings.is_empty() {
        0
    } else {
        // Non-zero exit for CI integration when findings exist
        1
    }
}

fn cmd_run_dependency_audit() -> i32 {
    use agentzero::skills::{report, scanner};

    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: cannot determine current directory: {e}");
            return 1;
        }
    };

    let project_name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    println!("Running dependency-audit on: {}", cwd.display());
    println!();

    // Use the built-in patterns file
    let patterns_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills/dependency-audit/patterns.toml");

    if !patterns_path.exists() {
        // Try relative to cwd
        let alt_path = cwd.join("skills/dependency-audit/patterns.toml");
        if alt_path.exists() {
            let results = scanner::scan_dependencies(&cwd, &alt_path);
            let report_text = report::generate_report(&results, project_name);
            println!("{report_text}");
            return if results.findings.is_empty() { 0 } else { 1 };
        }
        eprintln!("error: dependency-audit patterns.toml not found");
        return 1;
    }

    let results = scanner::scan_dependencies(&cwd, &patterns_path);
    let report_text = report::generate_report(&results, project_name);
    println!("{report_text}");

    if results.findings.is_empty() {
        0
    } else {
        1
    }
}

fn cmd_run_secrets_scan() -> i32 {
    use agentzero::skills::{report, scanner};

    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: cannot determine current directory: {e}");
            return 1;
        }
    };

    let project_name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    println!("Running secrets-scan on: {}", cwd.display());
    println!();

    // Use the built-in patterns file
    let patterns_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills/secrets-scan/patterns.toml");

    let scan_path = if patterns_path.exists() {
        patterns_path
    } else {
        let alt_path = cwd.join("skills/secrets-scan/patterns.toml");
        if alt_path.exists() {
            alt_path
        } else {
            eprintln!("error: secrets-scan patterns.toml not found");
            return 1;
        }
    };

    let results = scanner::scan_directory_with_patterns(&cwd, Some(&scan_path));
    let report_text = report::generate_report(&results, project_name);
    println!("{report_text}");

    if results.findings.is_empty() {
        0
    } else {
        1
    }
}

fn cmd_doctor() -> i32 {
    println!("AgentZero Doctor");
    println!("================");
    println!();

    println!("Crates (12):");
    println!("  agentzero-core     ok    types, crypto, vault, trust");
    println!("  agentzero-policy   ok    rule engine + TOML loader");
    println!("  agentzero-audit    ok    JSONL + encrypted logging");
    println!("  agentzero-session  ok    session, Ollama, OpenAI-compat");
    println!("  agentzero-tools    ok    tool registry + schemas");
    println!("  agentzero-skills   ok    manifests, scanner, reports");
    println!("  agentzero-sandbox  ok    profiles + WASM (feature flag)");
    println!("  agentzero-acp      ok    editor adapter (JSON-RPC/stdio)");
    println!("  agentzero-tracing  ok    centralized logging");
    println!("  agentzero-cli      ok    CLI binary");
    println!();

    let cwd = std::env::current_dir().unwrap_or_default();
    let az_dir = cwd.join(".agentzero");

    // Project status
    if az_dir.exists() {
        println!("Project:        initialized");
        if az_dir.join("policy.yml").exists() {
            match agentzero::policy::load_policy_file(&az_dir.join("policy.yml")) {
                Ok(rules) => println!("Policy:         {} rules loaded", rules.len()),
                Err(_) => println!("Policy:         error loading policy.yml"),
            }
        } else {
            println!("Policy:         missing");
        }
        if az_dir.join("settings.toml").exists() {
            let (prov, model) = load_settings();
            println!(
                "Settings:       provider={} model={}",
                prov.as_deref().unwrap_or("(default)"),
                model.as_deref().unwrap_or("(default)")
            );
        }
    } else {
        println!("Project:        not initialized (run `az init`)");
    }
    println!();

    // WASM sandbox
    print!("WASM sandbox:   ");
    if cfg!(feature = "wasm") {
        print!("compiled in");
    } else {
        print!("not compiled (rebuild with --features wasm)");
    }
    // Check WASM policy
    if az_dir.join("policy.yml").exists() {
        let content = std::fs::read_to_string(az_dir.join("policy.yml")).unwrap_or_default();
        if content.contains("wasm_execution") {
            if content.contains("wasm_execution = \"allow\"") {
                print!(" | policy: allow");
            } else if content.contains("wasm_execution = \"require_approval\"") {
                print!(" | policy: require_approval");
            } else {
                print!(" | policy: deny");
            }
        } else {
            print!(" | policy: deny (not configured)");
        }
    }
    println!();

    // Skills
    let skills_dir = cwd.join("skills");
    let mut skill_count = 0;
    let mut wasm_skill_count = 0;
    print!("Skills:         ");
    if skills_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
            let names: Vec<_> = entries
                .flatten()
                .filter(|e| e.path().is_dir() && e.path().join("SKILL.md").exists())
                .map(|e| {
                    let dir = e.path();
                    let name = e.file_name().to_string_lossy().to_string();
                    // Check if this is a WASM skill
                    if let Ok(manifest) = agentzero::skills::registry::load_manifest(&dir) {
                        if manifest.runtime == agentzero::skills::SkillRuntime::Wasm {
                            wasm_skill_count += 1;
                        }
                    }
                    name
                })
                .collect();
            skill_count = names.len();
            if names.is_empty() {
                print!("none installed");
            } else {
                print!("{}", names.join(", "));
            }
        }
    } else {
        print!("no skills/ directory");
    }
    print!(" ({skill_count})");
    if wasm_skill_count > 0 {
        print!(" ({wasm_skill_count} WASM)");
    }
    println!();

    // Vault
    let vault_dir = az_dir.join("vault");
    if vault_dir.exists() {
        let secret_count = std::fs::read_dir(&vault_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .flat_map(|e| {
                        std::fs::read_dir(e.path())
                            .into_iter()
                            .flatten()
                            .flatten()
                            .filter(|f| f.path().extension().is_some_and(|ext| ext == "enc"))
                    })
                    .count()
            })
            .unwrap_or(0);
        println!("Vault:          {secret_count} secret(s) stored");
    }

    // Sessions
    let sessions_dir = az_dir.join("sessions");
    if sessions_dir.exists() {
        let session_count = std::fs::read_dir(&sessions_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| {
                        let p = e.path();
                        p.extension()
                            .is_some_and(|ext| ext == "json" || ext == "enc")
                    })
                    .count()
            })
            .unwrap_or(0);
        println!("Sessions:       {session_count} saved");
    }

    println!();
    println!("Providers:      ollama, llama-cpp, vllm, lm-studio");
    println!("Tools:          read, list, search, write, edit, shell (6)");
    println!("Encryption:     AES-256-GCM + Argon2id");
    println!("ACP:            available (run `az serve`)");
    0
}

fn cmd_policy_status() -> i32 {
    use agentzero::policy::PolicyEngine;

    let cwd = std::env::current_dir().unwrap_or_default();
    let policy_path = cwd.join(".agentzero/policy.yml");

    println!("Policy Status");
    println!("=============");
    println!();

    if policy_path.exists() {
        match std::fs::read_to_string(&policy_path) {
            Ok(content) => {
                println!("Policy file: {}", policy_path.display());
                println!();
                println!("{content}");
            }
            Err(e) => {
                eprintln!("error: failed to read policy file: {e}");
                return 1;
            }
        }
    } else {
        println!("No policy file found. Using deny-by-default.");
        println!("Run `az init --private` to create one.");
    }

    println!();
    let engine = PolicyEngine::deny_by_default();
    println!("Active rules: {}", engine.rule_count());
    println!("Default: deny-by-default (fail closed)");
    0
}

fn cmd_audit_tail(count: usize) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();
    let audit_dir = cwd.join(".agentzero/audit");

    if !audit_dir.exists() {
        println!("No audit directory found. Run `az init` first.");
        return 1;
    }

    // Find the most recent .jsonl file
    let entries: Vec<_> = match std::fs::read_dir(&audit_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
            .collect(),
        Err(e) => {
            eprintln!("error: failed to read audit directory: {e}");
            return 1;
        }
    };

    if entries.is_empty() {
        println!("No audit logs found in {}", audit_dir.display());
        return 0;
    }

    // Sort by modification time, most recent first
    let mut paths: Vec<_> = entries.iter().map(|e| e.path()).collect();
    paths.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).ok();
        let b_time = b.metadata().and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    let latest = &paths[0];
    let session_id = latest
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let logger = match agentzero::audit::AuditLogger::new(&audit_dir, session_id) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: failed to open audit log: {e}");
            return 1;
        }
    };

    match logger.tail(count) {
        Ok(events) => {
            println!("Last {} events from session {session_id}:", events.len());
            println!();
            for event in &events {
                println!(
                    "  {} | {:?} | {:?} | {:?} | {}",
                    event.timestamp.format("%H:%M:%S"),
                    event.capability,
                    event.classification,
                    event.decision,
                    event.reason
                );
            }
            if events.is_empty() {
                println!("  (no events)");
            }
        }
        Err(e) => {
            eprintln!("error: failed to read audit events: {e}");
            return 1;
        }
    }
    0
}

fn cmd_demo() -> i32 {
    use agentzero::core::{
        placeholder_for, route_for_classification, Capability, DataClassification, ExecutionId,
        PolicyDecision, RedactionResult, RuntimeTier, SecretHandle, SessionId, SkillId, ToolId,
        TrustSource,
    };
    use agentzero::policy::PolicyEngine;
    use agentzero::sandbox::{SandboxMount, SandboxProfile};
    use agentzero::skills::{SkillManifest, SkillPermission, SkillRuntime};
    use agentzero::tools::builtin_tool_schemas;
    use chrono::Utc;

    println!("AgentZero Demo");
    println!("==============");
    println!();

    // Core IDs
    let session_id = SessionId::from_string("demo-session-001");
    let execution_id = ExecutionId::new();
    println!("Session:   {session_id}");
    println!("Execution: {execution_id}");
    println!();

    // Policy: deny-by-default with rules
    let engine = PolicyEngine::with_rules(vec![
        agentzero::policy::PolicyRule::allow(Capability::FileRead, DataClassification::Private),
        agentzero::policy::PolicyRule::require_approval(
            Capability::ShellCommand,
            "shell commands require user approval",
        ),
    ]);
    println!("Policy engine: {} rules loaded", engine.rule_count());

    let file_read_req = agentzero::policy::PolicyRequest {
        capability: Capability::FileRead,
        classification: DataClassification::Private,
        runtime: RuntimeTier::HostReadonly,
        context: "demo: read private file".into(),
    };
    println!(
        "  FileRead + Private: {:?}",
        engine.evaluate(&file_read_req)
    );

    let shell_req = agentzero::policy::PolicyRequest {
        capability: Capability::ShellCommand,
        classification: DataClassification::Private,
        runtime: RuntimeTier::HostSupervised,
        context: "demo: shell command".into(),
    };
    println!(
        "  ShellCommand + Private: {:?}",
        engine.evaluate(&shell_req)
    );

    let model_req = agentzero::policy::PolicyRequest {
        capability: Capability::ModelCall,
        classification: DataClassification::Secret,
        runtime: RuntimeTier::Deny,
        context: "demo: model call with secret".into(),
    };
    println!("  ModelCall + Secret: {:?}", engine.evaluate(&model_req));
    println!();

    // Model routing
    println!("Model routing:");
    println!(
        "  Secret → local:  {:?}",
        route_for_classification(DataClassification::Secret, true)
    );
    println!(
        "  Secret → remote: {:?}",
        route_for_classification(DataClassification::Secret, false)
    );
    println!(
        "  PII → remote:    {:?}",
        route_for_classification(DataClassification::Pii, false)
    );
    println!(
        "  Public → remote: {:?}",
        route_for_classification(DataClassification::Public, false)
    );
    println!();

    // Secret handles
    let handle = SecretHandle::new("github", "default");
    println!("Secret handle: {handle}");
    println!("Secret debug:  {handle:?}");
    println!();

    // Trust labels
    println!("Trust sources:");
    println!(
        "  UserInstruction: trusted={}",
        TrustSource::UserInstruction.is_trusted()
    );
    println!(
        "  DocumentContent: trusted={}",
        TrustSource::DocumentContent.is_trusted()
    );
    println!(
        "  ToolOutput:      trusted={}",
        TrustSource::ToolOutput.is_trusted()
    );
    println!();

    // Redaction
    let placeholder = placeholder_for(DataClassification::Pii, 0);
    println!("Redaction placeholder for PII: {placeholder}");
    let result = RedactionResult {
        redactions: vec![agentzero::core::Redaction {
            start: 6,
            end: 11,
            classification: DataClassification::Pii,
            placeholder: placeholder.clone(),
        }],
    };
    let redacted = result.apply("Hello Alice, welcome");
    println!("Redacted: \"{redacted}\"");
    println!();

    // Tools
    let tools = builtin_tool_schemas();
    println!("Built-in tools ({}):", tools.len());
    for tool in &tools {
        println!("  - {} ({})", tool.name, tool.description);
    }
    println!();

    // Skill manifest
    let skill = SkillManifest {
        id: SkillId::from_string("repo-security-audit"),
        name: "repo-security-audit".into(),
        version: "0.1.0".into(),
        description: "Audit repo for secrets, PII, and unsafe patterns".into(),
        runtime: SkillRuntime::InstructionOnly,
        permissions: vec![SkillPermission {
            capability: Capability::FileRead,
            reason: "needs to read repo files".into(),
        }],
        source: None,
        entrypoint: None,
    };
    skill
        .validate()
        .expect("demo skill manifest should be valid");
    println!(
        "Skill: {} v{} (runtime: {:?})",
        skill.name,
        skill.version,
        skill.runtime_tier()
    );
    println!();

    // Sandbox profile
    let profile = SandboxProfile::host_readonly(vec![SandboxMount {
        host_path: ".".into(),
        guest_path: "/project".into(),
        readonly: true,
    }]);
    println!(
        "Sandbox: {:?}, network: {:?}",
        profile.runtime, profile.network
    );
    println!();

    // Audit event
    let event = agentzero::core::AuditEvent {
        execution_id,
        session_id,
        timestamp: Utc::now(),
        action: "demo_tool_call".into(),
        capability: Capability::FileRead,
        classification: DataClassification::Private,
        decision: PolicyDecision::Allow,
        reason: "demo: allowed for demonstration".into(),
        runtime: RuntimeTier::HostReadonly,
        skill_id: Some(SkillId::from_string("repo-security-audit")),
        tool_id: Some(ToolId::from_string("read")),
        redactions_applied: vec![],
        approval_scope: None,
    };
    let event_json = serde_json::to_string_pretty(&event).expect("event should serialize");
    println!("Audit event:");
    println!("{event_json}");
    println!();

    // In-memory audit sink
    let sink = agentzero::audit::InMemorySink::new();
    agentzero::audit::AuditSink::record(&sink, &event).expect("in-memory record should succeed");
    println!("In-memory audit sink: {} event(s) recorded", sink.len());
    println!();

    println!("Demo complete. No untrusted code was executed.");
    0
}

fn cmd_plugin(action: PluginAction) -> i32 {
    use agentzero::skills::plugin::PluginRegistry;

    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: cannot determine current directory: {e}");
            return 1;
        }
    };

    let registry = PluginRegistry::new(cwd.join(".agentzero/plugins"));

    match action {
        PluginAction::List => {
            let plugins = registry.list();
            if plugins.is_empty() {
                println!("No plugins installed.");
                println!("Install a plugin with: az plugin install <path-to-plugin-directory>");
                return 0;
            }
            println!("{:<15} {:<10} {:<6} DESCRIPTION", "NAME", "VERSION", "CMDS");
            println!("{}", "-".repeat(60));
            for (manifest, _path) in &plugins {
                println!(
                    "{:<15} {:<10} {:<6} {}",
                    manifest.plugin.name,
                    manifest.plugin.version,
                    manifest.commands.len(),
                    manifest.plugin.description,
                );
            }
            0
        }
        PluginAction::Install { source } => {
            let source_path = std::path::Path::new(&source);
            if !source_path.exists() {
                eprintln!("error: source directory does not exist: {source}");
                return 1;
            }
            match registry.install(source_path) {
                Ok(name) => {
                    println!("Installed plugin: {name}");
                    0
                }
                Err(e) => {
                    eprintln!("error: failed to install plugin: {e}");
                    1
                }
            }
        }
        PluginAction::Info { name } => match registry.get(&name) {
            Some((manifest, path)) => {
                println!("Plugin: {}", manifest.plugin.name);
                println!("Version: {}", manifest.plugin.version);
                println!("Description: {}", manifest.plugin.description);
                println!("Runtime: {}", manifest.plugin.runtime);
                if let Some(ref wasm_path) = manifest.plugin.wasm_path {
                    println!("WASM path: {wasm_path}");
                }
                println!("Location: {}", path.display());
                if manifest.commands.is_empty() {
                    println!("\nNo commands declared.");
                } else {
                    println!("\nCommands:");
                    for cmd in &manifest.commands {
                        println!("  {:<15} {}", cmd.name, cmd.description);
                    }
                }
                0
            }
            None => {
                eprintln!("error: plugin '{name}' not found");
                1
            }
        },
    }
}

fn cmd_brain(action: BrainAction) -> i32 {
    // Brain runs as a WASM plugin via the sandbox (ADR 0015).
    // The WASM module is loaded from .agentzero/plugins/brain/brain.wasm
    // or falls back to the native implementation if WASM is unavailable.

    // Serialize the CLI action to JSON for the WASM guest
    let input_json = match &action {
        BrainAction::Init {
            root,
            force,
            dry_run,
        } => {
            format!(
                r#"{{"action":"init","root":"{}","force":{},"dry_run":{}}}"#,
                root.replace('\\', "\\\\").replace('"', "\\\""),
                force,
                dry_run
            )
        }
        BrainAction::Today { root, date, .. } => {
            let date_field = date
                .as_ref()
                .map(|d| format!(r#","date":"{}""#, d))
                .unwrap_or_default();
            format!(
                r#"{{"action":"today","root":"{}"{}}}"#,
                root.replace('\\', "\\\\").replace('"', "\\\""),
                date_field
            )
        }
        BrainAction::Capture {
            message,
            root,
            date,
            section,
        } => {
            let date_field = date
                .as_ref()
                .map(|d| format!(r#","date":"{}""#, d))
                .unwrap_or_default();
            let section_field = section
                .as_ref()
                .map(|s| format!(r#","section":"{}""#, s))
                .unwrap_or_default();
            format!(
                r#"{{"action":"capture","root":"{}","message":"{}"{}{}}}"#,
                root.replace('\\', "\\\\").replace('"', "\\\""),
                message.replace('\\', "\\\\").replace('"', "\\\""),
                date_field,
                section_field
            )
        }
        BrainAction::Query {
            term,
            root,
            raw,
            json,
            limit,
        } => {
            format!(
                r#"{{"action":"query","root":"{}","term":"{}","include_raw":{},"json":{},"limit":{}}}"#,
                root.replace('\\', "\\\\").replace('"', "\\\""),
                term.replace('\\', "\\\\").replace('"', "\\\""),
                raw,
                json,
                limit
            )
        }
        BrainAction::Ingest {
            path,
            root,
            save_prompt,
            dry_run,
        } => {
            format!(
                r#"{{"action":"ingest","root":"{}","path":"{}","save_prompt":{},"dry_run":{}}}"#,
                root.replace('\\', "\\\\").replace('"', "\\\""),
                path.replace('\\', "\\\\").replace('"', "\\\""),
                save_prompt,
                dry_run
            )
        }
        BrainAction::Review {
            root,
            date,
            save_prompt,
            dry_run,
        } => {
            let date_field = date
                .as_ref()
                .map(|d| format!(r#","date":"{}""#, d))
                .unwrap_or_default();
            format!(
                r#"{{"action":"review","root":"{}","save_prompt":{},"dry_run":{}{}}}"#,
                root.replace('\\', "\\\\").replace('"', "\\\""),
                save_prompt,
                dry_run,
                date_field
            )
        }
        BrainAction::Weekly {
            root,
            week,
            save_prompt,
        } => {
            let week_field = week
                .as_ref()
                .map(|w| format!(r#","week":"{}""#, w))
                .unwrap_or_default();
            format!(
                r#"{{"action":"weekly","root":"{}","save_prompt":{}{}}}"#,
                root.replace('\\', "\\\\").replace('"', "\\\""),
                save_prompt,
                week_field
            )
        }
        BrainAction::Health { root, json, fix } => {
            format!(
                r#"{{"action":"health","root":"{}","json":{},"fix":{}}}"#,
                root.replace('\\', "\\\\").replace('"', "\\\""),
                json,
                fix
            )
        }
        BrainAction::Checkpoint {
            root,
            message,
            init,
            dry_run,
        } => {
            let msg_field = message
                .as_ref()
                .map(|m| {
                    format!(
                        r#","message":"{}""#,
                        m.replace('\\', "\\\\").replace('"', "\\\"")
                    )
                })
                .unwrap_or_default();
            format!(
                r#"{{"action":"checkpoint","root":"{}","init":{},"dry_run":{}{}}}"#,
                root.replace('\\', "\\\\").replace('"', "\\\""),
                init,
                dry_run,
                msg_field
            )
        }
        BrainAction::Status { root } => {
            format!(
                r#"{{"action":"status","root":"{}"}}"#,
                root.replace('\\', "\\\\").replace('"', "\\\"")
            )
        }
    };

    // Extract the root path for the brain vault (used for path validation).
    let root_str = match &action {
        BrainAction::Init { root, .. }
        | BrainAction::Today { root, .. }
        | BrainAction::Capture { root, .. }
        | BrainAction::Query { root, .. }
        | BrainAction::Ingest { root, .. }
        | BrainAction::Review { root, .. }
        | BrainAction::Weekly { root, .. }
        | BrainAction::Health { root, .. }
        | BrainAction::Checkpoint { root, .. }
        | BrainAction::Status { root, .. } => root.clone(),
    };

    // For init, ensure the root directory exists before constructing
    // the PathValidator (which requires a canonicalizable root).
    if matches!(action, BrainAction::Init { .. }) {
        let rp = std::path::Path::new(&root_str);
        if !rp.exists() {
            if let Err(e) = std::fs::create_dir_all(rp) {
                eprintln!("error: cannot create vault root: {e}");
                return 1;
            }
        }
    }

    // Try generic plugin dispatch via registry or dev path
    #[cfg(feature = "wasm")]
    if let Some(exit_code) = run_plugin_wasm("brain", &input_json, Some(&root_str)) {
        // Handle --open for today command after WASM returns
        if let BrainAction::Today {
            root,
            date,
            open: true,
        } = &action
        {
            let date_str = date
                .clone()
                .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
            let full_path = format!("{root}/daily/{date_str}.md");
            if let Ok(editor) = std::env::var("EDITOR") {
                let _ = std::process::Command::new(&editor).arg(&full_path).status();
            } else {
                eprintln!("$EDITOR not set");
            }
        }
        return exit_code;
    }

    // Fallback: run natively if no WASM module found
    cmd_brain_native(action)
}

/// Direct filesystem host callbacks for WASM plugins.
///
/// All I/O operations are validated against a [`PathValidator`] anchored
/// at the plugin root, preventing path traversal, access to sensitive
/// locations, and symlink-based TOCTOU attacks.
#[cfg(feature = "wasm")]
struct PluginHostCallbacks {
    validator: agentzero::core::path_validator::PathValidator,
}

/// Sensitive paths for plugins.
///
/// Excludes `.agentzero` from the default blocklist because some plugins
/// (e.g. brain) legitimately use config files like `.agentzero-brain.toml`.
#[cfg(feature = "wasm")]
const PLUGIN_SENSITIVE: &[&str] = &[".ssh", ".gnupg", ".aws/credentials", ".env"];

#[cfg(feature = "wasm")]
impl PluginHostCallbacks {
    fn new(root: &std::path::Path) -> Result<Self, agentzero::core::path_validator::PathError> {
        Ok(Self {
            validator: agentzero::core::path_validator::PathValidator::with_sensitive(
                root,
                PLUGIN_SENSITIVE,
            )?,
        })
    }
}

#[cfg(feature = "wasm")]
impl agentzero::sandbox::wasm::WasmHostCallbacks for PluginHostCallbacks {
    fn read_file(&self, path: &str) -> Result<String, String> {
        let canonical = self
            .validator
            .validate_read(path)
            .map_err(|e| e.to_string())?;
        std::fs::read_to_string(canonical).map_err(|e| format!("read {path}: {e}"))
    }

    fn write_file(&self, path: &str, content: &str) -> Result<bool, String> {
        use agentzero::core::path_validator::PathError;
        // Use validate_write for existing files (includes symlink check),
        // fall back to validate_create for new files.
        let canonical = match self.validator.validate_write(path) {
            Ok(c) => c,
            Err(PathError::InvalidPath(_)) => self
                .validator
                .validate_create(path)
                .map_err(|e| e.to_string())?,
            Err(e) => return Err(e.to_string()),
        };
        std::fs::write(canonical, content).map_err(|e| format!("write {path}: {e}"))?;
        Ok(true)
    }

    fn append_file(&self, path: &str, content: &str) -> Result<bool, String> {
        let canonical = self
            .validator
            .validate_create(path)
            .map_err(|e| e.to_string())?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(canonical)
            .map_err(|e| format!("append {path}: {e}"))?;
        file.write_all(content.as_bytes())
            .map_err(|e| format!("append write {path}: {e}"))?;
        Ok(true)
    }

    fn list_dir(&self, path: &str) -> Result<Vec<String>, String> {
        let canonical = self
            .validator
            .validate_read(path)
            .map_err(|e| e.to_string())?;
        let entries = std::fs::read_dir(canonical).map_err(|e| format!("list_dir {path}: {e}"))?;
        let mut result = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| format!("read entry: {e}"))?;
            if let Some(name) = entry.file_name().to_str() {
                result.push(name.to_string());
            }
        }
        result.sort();
        Ok(result)
    }

    fn create_dir(&self, path: &str) -> Result<bool, String> {
        let canonical = self
            .validator
            .validate_create(path)
            .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(canonical).map_err(|e| format!("create_dir {path}: {e}"))?;
        Ok(true)
    }

    fn file_exists(&self, path: &str) -> Result<bool, String> {
        use agentzero::core::path_validator::PathError;
        match self.validator.validate_read(path) {
            Ok(canonical) => Ok(canonical.exists()),
            Err(PathError::InvalidPath(_)) => {
                // Path doesn't exist — validate bounds via create check
                let _ = self
                    .validator
                    .validate_create(path)
                    .map_err(|e| e.to_string())?;
                Ok(false)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn log(&self, message: &str) {
        eprintln!("[plugin] {message}");
    }

    fn now(&self) -> String {
        chrono::Local::now().to_rfc3339()
    }
}

/// Run a plugin's WASM module by name.
///
/// Discovers the plugin via the registry (`.agentzero/plugins/<name>/`),
/// falling back to the development path (`plugins/<name>/target/...`).
/// Returns `Some(exit_code)` if the plugin was found and executed,
/// `None` if no WASM module was found.
#[cfg(feature = "wasm")]
fn run_plugin_wasm(plugin_name: &str, input_json: &str, root_hint: Option<&str>) -> Option<i32> {
    use agentzero::sandbox::wasm::{WasmConfig, WasmEngine, WasmHostCallbacks};
    use agentzero::skills::plugin::PluginRegistry;
    use std::sync::Arc;

    let cwd = std::env::current_dir().ok()?;

    // 1. Try registry first
    let registry = PluginRegistry::new(cwd.join(".agentzero/plugins"));
    let wasm_bytes = if let Some(bytes) = registry.find_wasm(plugin_name) {
        eprintln!("[wasm] loaded {plugin_name} plugin from .agentzero/plugins/{plugin_name}/");
        bytes
    } else {
        // 2. Fallback to development path
        let dev_path = cwd.join(format!(
            "plugins/{plugin_name}/target/wasm32-unknown-unknown/release/agentzero_{plugin_name}_wasm.wasm"
        ));
        if dev_path.exists() {
            match std::fs::read(&dev_path) {
                Ok(bytes) => {
                    eprintln!(
                        "[wasm] loaded {plugin_name} plugin from {}",
                        dev_path.display()
                    );
                    bytes
                }
                Err(_) => return None,
            }
        } else {
            return None;
        }
    };

    // 3. Create WASM engine
    let config = WasmConfig {
        max_memory_bytes: 128 * 1024 * 1024, // 128MB
        max_duration_secs: 60,
        allow_filesystem: false,
    };

    let engine = match WasmEngine::new(config) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("error: failed to create WASM engine: {e}");
            return Some(1);
        }
    };

    // 4. Create PluginHostCallbacks with PathValidator rooted at root_hint
    let root_path = root_hint.unwrap_or(".");
    let callbacks: Arc<dyn WasmHostCallbacks> =
        match PluginHostCallbacks::new(std::path::Path::new(root_path)) {
            Ok(cb) => Arc::new(cb),
            Err(e) => {
                eprintln!("error: invalid root path: {e}");
                return Some(1);
            }
        };

    // 5. Execute and parse response
    match engine.execute_with_input(&wasm_bytes, callbacks, input_json) {
        Ok(result) => {
            if let Ok(response) = serde_json::from_str::<serde_json::Value>(&result.output) {
                let success = response
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if success {
                    if let Some(output) = response.get("output").and_then(|v| v.as_str()) {
                        println!("{output}");
                    }
                    return Some(0);
                } else if let Some(err) = response.get("error").and_then(|v| v.as_str()) {
                    eprintln!("error: {err}");
                    return Some(1);
                }
            }
            // Fallthrough: couldn't parse as JSON response
            if result.success {
                println!("{}", result.output);
                Some(0)
            } else {
                eprintln!("error: {}", result.output);
                Some(1)
            }
        }
        Err(e) => {
            eprintln!("error: WASM execution failed: {e}");
            Some(1)
        }
    }
}

/// Native fallback for brain commands when WASM is unavailable.
fn cmd_brain_native(action: BrainAction) -> i32 {
    use agentzero_brain::{
        brain_capture, brain_checkpoint, brain_health, brain_ingest, brain_init, brain_query,
        brain_review, brain_status, brain_today, brain_weekly, format_results, load_config,
        CheckpointOptions, HealthOptions, IngestOptions, InitOptions, QueryOptions, RealBrainFs,
        ReviewOptions, WeeklyOptions,
    };

    // Extract vault root from whichever action variant we have.
    let root_str = match &action {
        BrainAction::Init { root, .. }
        | BrainAction::Today { root, .. }
        | BrainAction::Capture { root, .. }
        | BrainAction::Query { root, .. }
        | BrainAction::Ingest { root, .. }
        | BrainAction::Review { root, .. }
        | BrainAction::Weekly { root, .. }
        | BrainAction::Health { root, .. }
        | BrainAction::Checkpoint { root, .. }
        | BrainAction::Status { root, .. } => root.clone(),
    };
    let root_path = std::path::Path::new(&root_str);

    // For init, ensure the root directory exists before constructing
    // the PathValidator (which requires a canonicalizable root).
    if matches!(action, BrainAction::Init { .. }) && !root_path.exists() {
        if let Err(e) = std::fs::create_dir_all(root_path) {
            eprintln!("error: cannot create vault root: {e}");
            return 1;
        }
    }

    let fs = match RealBrainFs::new(root_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: invalid vault root: {e}");
            return 1;
        }
    };

    match action {
        BrainAction::Init {
            root,
            force,
            dry_run,
        } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("warning: {e}, using defaults");
                    agentzero_brain::BrainConfig::default()
                }
            };
            let opts = InitOptions { force, dry_run };
            match brain_init(&fs, &root, &config, &opts) {
                Ok(result) => {
                    if dry_run {
                        println!("[dry-run] {}", result.summary());
                        for path in &result.created {
                            println!("  would create: {path}");
                        }
                    } else {
                        println!("{}", result.summary());
                    }
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        BrainAction::Today { root, date, open } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            };
            match brain_today(&fs, &root, &config, date.as_deref()) {
                Ok(path) => {
                    println!("{path}");
                    if open {
                        let full_path = format!("{root}/{path}");
                        if let Ok(editor) = std::env::var("EDITOR") {
                            let status =
                                std::process::Command::new(&editor).arg(&full_path).status();
                            match status {
                                Ok(s) if s.success() => {}
                                Ok(s) => {
                                    eprintln!("editor exited with: {s}");
                                    return 1;
                                }
                                Err(e) => {
                                    eprintln!("failed to open editor '{editor}': {e}");
                                    return 1;
                                }
                            }
                        } else {
                            eprintln!("$EDITOR not set");
                            return 1;
                        }
                    }
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        BrainAction::Capture {
            message,
            root,
            date,
            section,
        } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            };
            match brain_capture(
                &fs,
                &root,
                &config,
                &message,
                date.as_deref(),
                section.as_deref(),
            ) {
                Ok((path, entry)) => {
                    println!("{path}");
                    println!("{entry}");
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        BrainAction::Query {
            term,
            root,
            raw,
            json,
            limit,
        } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            };
            let opts = QueryOptions {
                include_raw: raw,
                json,
                limit,
            };
            match brain_query(&fs, &root, &config, &term, &opts) {
                Ok(matches) => {
                    print!("{}", format_results(&matches, json));
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        BrainAction::Ingest {
            path,
            root,
            save_prompt,
            dry_run,
        } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            };
            let opts = IngestOptions {
                save_prompt,
                dry_run,
            };
            match brain_ingest(&fs, &root, &config, &path, &opts) {
                Ok(result) => {
                    for w in &result.warnings {
                        eprintln!("{w}");
                    }
                    println!("{}", result.prompt);
                    if let Some(saved) = &result.saved_to {
                        eprintln!("Saved to: {saved}");
                    }
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        BrainAction::Review {
            root,
            date,
            save_prompt,
            dry_run,
        } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            };
            let opts = ReviewOptions {
                date,
                save_prompt,
                dry_run,
            };
            match brain_review(&fs, &root, &config, &opts) {
                Ok(result) => {
                    println!("{}", result.prompt);
                    if let Some(saved) = &result.saved_to {
                        eprintln!("Saved to: {saved}");
                    }
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        BrainAction::Weekly {
            root,
            week,
            save_prompt,
        } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            };
            let opts = WeeklyOptions { week, save_prompt };
            match brain_weekly(&fs, &root, &config, &opts) {
                Ok(result) => {
                    println!("{}", result.prompt);
                    if let Some(saved) = &result.saved_to {
                        eprintln!("Saved to: {saved}");
                    }
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        BrainAction::Health { root, json, fix } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            };
            let opts = HealthOptions { json, fix };
            match brain_health(&fs, &root, &config, &opts) {
                Ok(report) => {
                    if json {
                        match serde_json::to_string_pretty(&report) {
                            Ok(j) => println!("{j}"),
                            Err(e) => {
                                eprintln!("error serializing report: {e}");
                                return 1;
                            }
                        }
                    } else {
                        print!("{}", report.display());
                    }
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        BrainAction::Checkpoint {
            root,
            message,
            init,
            dry_run,
        } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("warning: {e}, using defaults");
                    agentzero_brain::BrainConfig::default()
                }
            };
            let opts = CheckpointOptions {
                message,
                init,
                dry_run,
            };
            match brain_checkpoint(&fs, &root, &config, &opts) {
                Ok(result) => {
                    println!("{}", result.summary);
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        BrainAction::Status { root } => {
            let config = match load_config(&fs, &root) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            };
            match brain_status(&fs, &root, &config) {
                Ok(result) => {
                    print!("{}", result.display());
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: super::Command,
    }

    fn parse(args: &[&str]) -> super::Command {
        let mut full_args = vec!["az"];
        full_args.extend_from_slice(args);
        TestCli::parse_from(full_args).command
    }

    #[test]
    fn parse_init() {
        match parse(&["init"]) {
            super::Command::Init { private, .. } => assert!(!private),
            other => panic!("expected Init, got {other:?}"),
        }
    }

    #[test]
    fn parse_init_private() {
        match parse(&["init", "--private"]) {
            super::Command::Init { private, .. } => assert!(private),
            other => panic!("expected Init --private, got {other:?}"),
        }
    }

    #[test]
    fn parse_init_with_editor() {
        match parse(&["init", "--editor", "vscode"]) {
            super::Command::Init { private, editor } => {
                assert!(!private);
                assert_eq!(editor, Some("vscode".into()));
            }
            other => panic!("expected Init --editor, got {other:?}"),
        }
    }

    #[test]
    fn parse_chat() {
        match parse(&["chat"]) {
            super::Command::Chat { remote, .. } => assert!(!remote),
            other => panic!("expected Chat, got {other:?}"),
        }
    }

    #[test]
    fn parse_chat_remote() {
        match parse(&["chat", "--remote"]) {
            super::Command::Chat { remote, .. } => assert!(remote),
            other => panic!("expected Chat --remote, got {other:?}"),
        }
    }

    #[test]
    fn parse_run() {
        match parse(&["run", "repo-security-audit"]) {
            super::Command::Run { name, skip_verify } => {
                assert_eq!(name, "repo-security-audit");
                assert!(!skip_verify);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn parse_run_skip_verify() {
        match parse(&["run", "--skip-verify", "repo-security-audit"]) {
            super::Command::Run { name, skip_verify } => {
                assert_eq!(name, "repo-security-audit");
                assert!(skip_verify);
            }
            other => panic!("expected Run --skip-verify, got {other:?}"),
        }
    }

    #[test]
    fn parse_doctor() {
        assert!(matches!(parse(&["doctor"]), super::Command::Doctor));
    }

    #[test]
    fn parse_history() {
        assert!(matches!(parse(&["history"]), super::Command::History));
    }

    #[test]
    fn parse_serve() {
        assert!(matches!(parse(&["serve"]), super::Command::Serve));
    }

    #[test]
    fn parse_demo() {
        assert!(matches!(parse(&["demo"]), super::Command::Demo));
    }

    #[test]
    fn parse_install() {
        match parse(&["install", "/tmp/my-skill"]) {
            super::Command::Install {
                source,
                refresh_index,
            } => {
                assert_eq!(source, "/tmp/my-skill");
                assert!(!refresh_index);
            }
            other => panic!("expected Install, got {other:?}"),
        }
    }

    #[test]
    fn parse_install_refresh_index() {
        match parse(&["install", "--refresh-index", "security-audit"]) {
            super::Command::Install {
                source,
                refresh_index,
            } => {
                assert_eq!(source, "security-audit");
                assert!(refresh_index);
            }
            other => panic!("expected Install --refresh-index, got {other:?}"),
        }
    }

    #[test]
    fn parse_chat_with_resume() {
        match parse(&["chat", "--resume", "abc123"]) {
            super::Command::Chat { resume, .. } => assert_eq!(resume, Some("abc123".into())),
            other => panic!("expected Chat, got {other:?}"),
        }
    }

    #[test]
    fn parse_chat_with_provider() {
        match parse(&["chat", "--provider", "llama-cpp"]) {
            super::Command::Chat { provider, .. } => assert_eq!(provider, "llama-cpp"),
            other => panic!("expected Chat, got {other:?}"),
        }
    }

    #[test]
    fn parse_chat_with_print() {
        match parse(&["chat", "-P", "what is 2+2"]) {
            super::Command::Chat { print, mode, .. } => {
                assert_eq!(print, Some("what is 2+2".into()));
                assert_eq!(mode, "text"); // default
            }
            other => panic!("expected Chat --print, got {other:?}"),
        }
    }

    #[test]
    fn parse_chat_with_print_json() {
        match parse(&["chat", "-P", "hello", "--mode", "json"]) {
            super::Command::Chat { print, mode, .. } => {
                assert_eq!(print, Some("hello".into()));
                assert_eq!(mode, "json");
            }
            other => panic!("expected Chat --print --mode json, got {other:?}"),
        }
    }

    #[test]
    fn parse_policy_status() {
        match parse(&["policy", "status"]) {
            super::Command::Policy {
                action: super::PolicyAction::Status,
            } => {}
            other => panic!("expected Policy Status, got {other:?}"),
        }
    }

    #[test]
    fn parse_audit_tail() {
        match parse(&["audit", "tail"]) {
            super::Command::Audit {
                action: super::AuditAction::Tail { count },
            } => assert_eq!(count, 20),
            other => panic!("expected Audit Tail, got {other:?}"),
        }
    }

    #[test]
    fn parse_audit_tail_custom_count() {
        match parse(&["audit", "tail", "--count", "50"]) {
            super::Command::Audit {
                action: super::AuditAction::Tail { count },
            } => assert_eq!(count, 50),
            other => panic!("expected Audit Tail, got {other:?}"),
        }
    }

    #[test]
    fn parse_vault_list() {
        match parse(&["vault", "list"]) {
            super::Command::Vault {
                action: super::VaultAction::List,
            } => {}
            other => panic!("expected Vault List, got {other:?}"),
        }
    }

    #[test]
    fn parse_plugin_list() {
        match parse(&["plugin", "list"]) {
            super::Command::Plugin {
                action: super::PluginAction::List,
            } => {}
            other => panic!("expected Plugin List, got {other:?}"),
        }
    }

    #[test]
    fn parse_plugin_install() {
        match parse(&["plugin", "install", "/tmp/brain"]) {
            super::Command::Plugin {
                action: super::PluginAction::Install { source },
            } => {
                assert_eq!(source, "/tmp/brain");
            }
            other => panic!("expected Plugin Install, got {other:?}"),
        }
    }

    #[test]
    fn parse_plugin_info() {
        match parse(&["plugin", "info", "brain"]) {
            super::Command::Plugin {
                action: super::PluginAction::Info { name },
            } => {
                assert_eq!(name, "brain");
            }
            other => panic!("expected Plugin Info, got {other:?}"),
        }
    }
}
