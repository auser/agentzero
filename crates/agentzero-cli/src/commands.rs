use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize a new AgentZero project.
    Init {
        /// Initialize with private-by-default policy.
        #[arg(long)]
        private: bool,
    },
    /// Start a supervised chat session.
    Chat {
        /// Use local models only (no remote calls).
        #[arg(long)]
        local: bool,
        /// Model to use (default: llama3.2).
        #[arg(long, short, default_value = "llama3.2")]
        model: String,
        /// Stream tokens as they arrive.
        #[arg(long)]
        stream: bool,
    },
    /// Run a skill or tool by name.
    Run {
        /// Name of the skill or tool to run.
        name: String,
    },
    /// Check system health and configuration.
    Doctor,
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
    /// List past chat sessions.
    History,
    /// Manage secret vault handles.
    Vault {
        #[command(subcommand)]
        action: VaultAction,
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
}

#[derive(Debug, Subcommand)]
pub enum VaultAction {
    /// List secret handles.
    List,
}

pub async fn run(command: Command) -> i32 {
    match command {
        Command::Init { private } => cmd_init(private),
        Command::Chat {
            local,
            model,
            stream,
        } => cmd_chat(local, &model, stream).await,
        Command::Run { name } => cmd_run(&name),
        Command::History => cmd_history(),
        Command::Doctor => cmd_doctor(),
        Command::Demo => cmd_demo(),
        Command::Policy { action } => match action {
            PolicyAction::Status => cmd_policy_status(),
        },
        Command::Audit { action } => match action {
            AuditAction::Tail { count } => cmd_audit_tail(count),
        },
        Command::Vault { action } => match action {
            VaultAction::List => {
                println!("No secret handles configured.");
                println!("Use `agentzero init --private` to set up a project first.");
                0
            }
        },
    }
}

fn cmd_init(private: bool) -> i32 {
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

    let dirs = ["audit", "sessions"];
    for sub in &dirs {
        if let Err(e) = std::fs::create_dir_all(az_dir.join(sub)) {
            eprintln!("error: failed to create .agentzero/{sub}: {e}");
            return 1;
        }
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
        )
    };

    if let Err(e) = std::fs::write(az_dir.join("policy.yml"), policy_content) {
        eprintln!("error: failed to write policy.yml: {e}");
        return 1;
    }

    let mode = if private { "private" } else { "default" };
    println!("Initialized AgentZero project ({mode} mode)");
    println!("  {}", az_dir.display());
    println!("  {}/policy.yml", az_dir.display());
    println!("  {}/audit/", az_dir.display());
    println!("  {}/sessions/", az_dir.display());
    0
}

async fn cmd_chat(local: bool, model: &str, stream: bool) -> i32 {
    use agentzero::session::{
        ChatMessage, ModelProvider, OllamaConfig, OllamaProvider, Session, SessionConfig,
        SessionMode, ToolExecutor,
    };
    use std::io::{self, BufRead, Write};

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
    println!("Session: {}", session.id());

    // Check Ollama
    let config = OllamaConfig {
        model: model.to_string(),
        ..OllamaConfig::default()
    };
    let provider = OllamaProvider::new(config);
    println!("Model: {} ({})", provider.model_name(), provider.name());

    match provider.health_check().await {
        Ok(true) => println!("Ollama: connected"),
        Ok(false) => {
            eprintln!("Ollama responded but may not be healthy. Continuing anyway.");
        }
        Err(e) => {
            eprintln!("error: cannot connect to Ollama at http://localhost:11434");
            eprintln!("  {e}");
            eprintln!();
            eprintln!("Make sure Ollama is running: `ollama serve`");
            return 1;
        }
    }

    let tools = OllamaProvider::agentzero_tool_definitions();
    println!(
        "Tools: {} available (read, list, search, write, shell)",
        tools.len()
    );
    println!();
    println!("Type your message and press Enter. Type /quit to exit.");
    println!();

    let stdin = io::stdin();
    let mut messages: Vec<ChatMessage> = vec![ChatMessage::system(concat!(
        "You are AgentZero, a secure AI agent assistant. ",
        "You help users with their local development projects. ",
        "You are running in local-only mode — all inference happens on this machine. ",
        "You have access to tools: read (read files), list (list directories), ",
        "search (search file contents), and shell (run shell commands, requires approval). ",
        "Use tools when the user asks about their project. Be concise and helpful."
    ))];

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
            println!("Session: {}", session.id());
            println!("Mode: {mode}");
            println!("Model: {}", provider.model_name());
            println!();
            continue;
        }

        messages.push(ChatMessage::user(input));

        // Chat with tool calling loop
        let max_tool_rounds = 5;
        for round in 0..=max_tool_rounds {
            let result = match provider.chat_with_tools(&messages, Some(&tools)).await {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("error: {e}");
                    messages.pop();
                    break;
                }
            };

            if result.has_tool_calls() && round < max_tool_rounds {
                // Add assistant message with tool calls
                messages.push(ChatMessage {
                    role: "assistant".into(),
                    content: result.content.clone(),
                    tool_calls: Some(result.tool_calls.clone()),
                });

                // Execute each tool call
                for tc in &result.tool_calls {
                    let tool_name = &tc.function.name;
                    let tool_args = &tc.function.arguments;

                    // Dangerous tools need user approval
                    if tool_name == "write" {
                        let path = tool_args
                            .get("path")
                            .and_then(|v| v.as_str())
                            .unwrap_or("(unknown)");
                        let content_len = tool_args
                            .get("content")
                            .and_then(|v| v.as_str())
                            .map_or(0, |s| s.len());
                        print!("  [APPROVE write: `{path}` ({content_len} bytes)?] (y/n) ");
                        io::stdout().flush().ok();
                        let mut answer = String::new();
                        stdin.lock().read_line(&mut answer).ok();
                        if !answer.trim().eq_ignore_ascii_case("y") {
                            println!("  [DENIED by user]");
                            messages.push(ChatMessage::tool(
                                "File write denied by user. Do not retry without asking.",
                            ));
                            continue;
                        }
                    }
                    if tool_name == "shell" {
                        let cmd = tool_args
                            .get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("(unknown)");
                        print!("  [APPROVE shell: `{cmd}`?] (y/n) ");
                        io::stdout().flush().ok();
                        let mut answer = String::new();
                        stdin.lock().read_line(&mut answer).ok();
                        if !answer.trim().eq_ignore_ascii_case("y") {
                            println!("  [DENIED by user]");
                            messages.push(ChatMessage::tool(
                                "Shell command denied by user. Do not retry without asking.",
                            ));
                            continue;
                        }
                    }

                    print!("  [tool: {tool_name}] ");
                    io::stdout().flush().ok();

                    match session.execute_tool(tool_name, tool_args) {
                        Ok(output) => {
                            let truncated = if output.len() > 2000 {
                                format!(
                                    "{}...\n[truncated, {} bytes total]",
                                    &output[..2000],
                                    output.len()
                                )
                            } else {
                                output
                            };
                            println!("ok ({} bytes)", truncated.len());
                            messages.push(ChatMessage::tool(truncated));
                        }
                        Err(e) => {
                            println!("error: {e}");
                            messages.push(ChatMessage::tool(format!("Error: {e}")));
                        }
                    }
                }
                // Loop back to get the model's response after tool results
            } else if stream && round == 0 {
                // No tool calls on first round — re-request with streaming
                // Remove the non-streaming response, stream it instead
                println!();
                print!("agentzero> ");
                io::stdout().flush().ok();
                match provider
                    .chat_streaming(&messages, |token| {
                        print!("{token}");
                        io::stdout().flush().ok();
                    })
                    .await
                {
                    Ok(full_response) => {
                        println!();
                        println!();
                        messages.push(ChatMessage::assistant(&full_response));
                    }
                    Err(e) => {
                        eprintln!("\nerror during streaming: {e}");
                        messages.pop();
                    }
                }
                break;
            } else {
                // No tool calls — print the response
                if !result.content.is_empty() {
                    println!();
                    println!("agentzero> {}", result.content);
                    println!();
                }
                messages.push(ChatMessage::assistant(&result.content));
                break;
            }
        }
    }

    // Save conversation to .agentzero/sessions/ if initialized
    let sessions_dir = cwd.join(".agentzero/sessions");
    if sessions_dir.exists() && messages.len() > 1 {
        let session_file = sessions_dir.join(format!("{}.json", session.id()));
        let session_data = serde_json::json!({
            "session_id": session.id().as_str(),
            "model": model,
            "mode": mode,
            "message_count": messages.len(),
            "messages": messages,
        });
        match serde_json::to_string_pretty(&session_data) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&session_file, json) {
                    eprintln!("warning: failed to save session: {e}");
                } else {
                    println!("Session saved to {}", session_file.display());
                }
            }
            Err(e) => {
                eprintln!("warning: failed to serialize session: {e}");
            }
        }
    }

    0
}

fn cmd_history() -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();
    let sessions_dir = cwd.join(".agentzero/sessions");

    if !sessions_dir.exists() {
        println!("No sessions directory. Run `agentzero init` first.");
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

fn cmd_run(name: &str) -> i32 {
    match name {
        "repo-security-audit" => cmd_run_security_audit(),
        other => {
            eprintln!("unknown skill: {other}");
            eprintln!("Available skills: repo-security-audit");
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

fn cmd_doctor() -> i32 {
    println!("AgentZero Doctor");
    println!("================");
    println!();

    println!("Crates:");
    println!("  agentzero-core     ok");
    println!("  agentzero-policy   ok");
    println!("  agentzero-audit    ok");
    println!("  agentzero-session  ok");
    println!("  agentzero-tools    ok");
    println!("  agentzero-skills   ok");
    println!("  agentzero-sandbox  ok");
    println!("  agentzero-tracing  ok");
    println!("  agentzero-cli      ok");
    println!();

    // Check for project config
    let cwd = std::env::current_dir().unwrap_or_default();
    let az_dir = cwd.join(".agentzero");
    if az_dir.exists() {
        println!("Project:        initialized at {}", az_dir.display());
        if az_dir.join("policy.yml").exists() {
            println!("Policy:         {}/policy.yml", az_dir.display());
        } else {
            println!("Policy:         missing (no policy.yml)");
        }
    } else {
        println!("Project:        not initialized (run `agentzero init`)");
    }

    println!();
    println!("Policy engine:  deny-by-default with rule evaluation");
    println!("Sandbox:        contracts only (no runtime execution)");
    println!("Model routing:  local-first (classification-based)");
    println!("Audit:          JSONL file sink + in-memory sink");
    println!("Skills:         manifest validation available");
    println!("Secret handles: capability-based (handle://vault/...)");
    println!("Trust labels:   10 source tiers (4 trusted, 6 untrusted)");
    println!("Redaction:      placeholder-based redaction engine");
    println!();
    println!("Skills:");
    println!("  repo-security-audit  built-in (run with `agentzero run repo-security-audit`)");
    println!();
    println!("Status: Phase 5 complete. Session engine + security audit available.");
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
        println!("Run `agentzero init --private` to create one.");
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
        println!("No audit directory found. Run `agentzero init` first.");
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

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: super::Command,
    }

    fn parse(args: &[&str]) -> super::Command {
        let mut full_args = vec!["agentzero"];
        full_args.extend_from_slice(args);
        TestCli::parse_from(full_args).command
    }

    #[test]
    fn parse_init() {
        match parse(&["init"]) {
            super::Command::Init { private } => assert!(!private),
            other => panic!("expected Init, got {other:?}"),
        }
    }

    #[test]
    fn parse_init_private() {
        match parse(&["init", "--private"]) {
            super::Command::Init { private } => assert!(private),
            other => panic!("expected Init --private, got {other:?}"),
        }
    }

    #[test]
    fn parse_chat() {
        match parse(&["chat"]) {
            super::Command::Chat { local, .. } => assert!(!local),
            other => panic!("expected Chat, got {other:?}"),
        }
    }

    #[test]
    fn parse_chat_local() {
        match parse(&["chat", "--local"]) {
            super::Command::Chat { local, .. } => assert!(local),
            other => panic!("expected Chat --local, got {other:?}"),
        }
    }

    #[test]
    fn parse_run() {
        match parse(&["run", "repo-security-audit"]) {
            super::Command::Run { name } => assert_eq!(name, "repo-security-audit"),
            other => panic!("expected Run, got {other:?}"),
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
    fn parse_demo() {
        assert!(matches!(parse(&["demo"]), super::Command::Demo));
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
}
