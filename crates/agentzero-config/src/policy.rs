use crate::loader::load;
use crate::model::{McpServerEntry, McpServersFile};
use agentzero_core::common::paths::MCP_CONFIG_FILE;
use agentzero_tools::{
    McpServerDef, ReadFilePolicy, ShellPolicy, ToolSecurityPolicy, UrlAccessPolicy,
    WebSearchConfig, WriteFilePolicy,
};
use anyhow::Context;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

pub fn load_tool_security_policy(
    workspace_root: &Path,
    config_path: &Path,
) -> anyhow::Result<ToolSecurityPolicy> {
    let config = load(config_path)?;
    let allowed_root = resolve_allowed_root(workspace_root, &config.security.allowed_root)?;

    let url_cfg = &config.security.url_access;
    let mut allow_cidrs = Vec::new();
    for cidr_str in &url_cfg.allow_cidrs {
        allow_cidrs.push(
            agentzero_core::common::url_policy::CidrRange::parse(cidr_str).with_context(|| {
                format!("invalid CIDR in security.url_access.allow_cidrs: {cidr_str}")
            })?,
        );
    }

    let enable_git = config.security.allowed_commands.iter().any(|c| c == "git");

    let mut policy = ToolSecurityPolicy {
        read_file: ReadFilePolicy {
            allowed_root: allowed_root.clone(),
            max_read_bytes: config.security.read_file.max_read_bytes,
            allow_binary: config.security.read_file.allow_binary,
        },
        write_file: WriteFilePolicy {
            allowed_root,
            max_write_bytes: config.security.write_file.max_write_bytes,
        },
        shell: ShellPolicy {
            allowed_commands: config.security.allowed_commands,
            max_args: config.security.shell.max_args,
            max_arg_length: config.security.shell.max_arg_length,
            max_output_bytes: config.security.shell.max_output_bytes,
            forbidden_chars: config.security.shell.forbidden_chars.clone(),
            command_policy: if config.security.shell.context_aware_parsing {
                Some(
                    agentzero_tools::shell::ShellCommandPolicy::from_legacy_forbidden_chars(
                        &config.security.shell.forbidden_chars,
                    ),
                )
            } else {
                None
            },
        },
        url_access: UrlAccessPolicy {
            block_private_ip: url_cfg.block_private_ip,
            allow_loopback: url_cfg.allow_loopback,
            allow_cidrs,
            allow_domains: url_cfg.allow_domains.clone(),
            enforce_domain_allowlist: url_cfg.enforce_domain_allowlist,
            domain_allowlist: url_cfg.domain_allowlist.clone(),
            domain_blocklist: url_cfg.domain_blocklist.clone(),
            approved_domains: url_cfg.approved_domains.clone(),
            require_first_visit_approval: url_cfg.require_first_visit_approval,
        },
        enable_write_file: config.security.write_file.enabled,
        enable_mcp: config.security.mcp.enabled,
        mcp_servers: load_mcp_servers(
            config_path,
            workspace_root,
            &config.security.mcp.allowed_servers,
        ),
        allowed_mcp_servers: config.security.mcp.allowed_servers,
        enable_git,
        enable_cron: true,
        enable_web_search: config.web_search.enabled,
        web_search_config: WebSearchConfig {
            provider: config.web_search.provider.clone(),
            brave_api_key: config.web_search.brave_api_key.clone(),
            jina_api_key: config.web_search.jina_api_key.clone(),
            timeout_secs: config.web_search.timeout_secs,
            user_agent: config.web_search.user_agent.clone(),
        },
        enable_browser: config.browser.enabled,
        enable_browser_open: config.browser.enabled,
        enable_http_request: config.http_request.enabled,
        enable_web_fetch: config.web_fetch.enabled,
        enable_url_validation: true,
        enable_agents_ipc: true,
        enable_html_extract: config.web_fetch.enabled,
        enable_composio: config.composio.enabled,
        enable_pushover: config.pushover.enabled,
        enable_code_interpreter: config.code_interpreter.enabled,
        enable_tts: config.media_gen.tts.enabled,
        enable_image_gen: config.media_gen.image_gen.enabled,
        enable_video_gen: config.media_gen.video_gen.enabled,
        enable_wasm_plugins: config.security.plugin.wasm_enabled,
        wasm_global_plugin_dir: config.security.plugin.global_plugin_dir.map(PathBuf::from),
        wasm_project_plugin_dir: config.security.plugin.project_plugin_dir.map(PathBuf::from),
        wasm_dev_plugin_dir: config.security.plugin.dev_plugin_dir.map(PathBuf::from),
    };

    // Privacy enforcement: local_only mode disables outbound network tools.
    if config.privacy.mode == "local_only" {
        policy.enable_http_request = false;
        policy.enable_web_fetch = false;
        policy.enable_web_search = false;
        policy.enable_html_extract = false;
        policy.enable_composio = false;
        policy.enable_tts = false;
        policy.enable_image_gen = false;
        policy.enable_video_gen = false;
        // Restrict URL access to localhost only.
        policy.url_access.allow_loopback = true;
        policy.url_access.block_private_ip = false;
        policy.url_access.enforce_domain_allowlist = true;
        policy.url_access.domain_allowlist = vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "::1".to_string(),
        ];
    }

    Ok(policy)
}

#[derive(Debug, Clone)]
pub struct AuditPolicy {
    pub enabled: bool,
    pub path: PathBuf,
}

pub fn load_audit_policy(workspace_root: &Path, config_path: &Path) -> anyhow::Result<AuditPolicy> {
    let config = load(config_path)?;

    if !config.security.audit.enabled {
        return Ok(AuditPolicy {
            enabled: false,
            path: resolve_path(workspace_root, "./agentzero-audit.log"),
        });
    }

    Ok(AuditPolicy {
        enabled: true,
        path: resolve_path(workspace_root, &config.security.audit.path),
    })
}

fn resolve_allowed_root(workspace_root: &Path, configured_root: &str) -> anyhow::Result<PathBuf> {
    let configured_path = Path::new(configured_root);
    if configured_path.is_relative()
        && configured_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        anyhow::bail!("security.allowed_root must not contain parent directory traversal");
    }

    let path = Path::new(configured_root);
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    };

    candidate.canonicalize().with_context(|| {
        format!(
            "security.allowed_root does not exist: {}",
            candidate.display()
        )
    })
}

fn resolve_path(workspace_root: &Path, configured: &str) -> PathBuf {
    let path = Path::new(configured);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

/// Load MCP server definitions from global and project `mcp.json` files,
/// with an optional env-var override layer.
///
/// Merge order (later overrides earlier by server name):
/// 1. Global: `{config_path.parent()}/mcp.json`
/// 2. Project: `{workspace_root}/.agentzero/mcp.json`
/// 3. `AGENTZERO_MCP_SERVERS` env var
fn load_mcp_servers(
    config_path: &Path,
    workspace_root: &Path,
    allowed_servers: &[String],
) -> HashMap<String, McpServerDef> {
    let mut servers: HashMap<String, McpServerEntry> = HashMap::new();

    // 1. Global mcp.json (in the same directory as agentzero.toml).
    if let Some(data_dir) = config_path.parent() {
        let global_path = data_dir.join(MCP_CONFIG_FILE);
        if let Some(file) = read_mcp_json(&global_path) {
            servers.extend(file.mcp_servers);
        }
    }

    // 2. Project mcp.json.
    let project_path = workspace_root.join(".agentzero").join(MCP_CONFIG_FILE);
    if let Some(file) = read_mcp_json(&project_path) {
        servers.extend(file.mcp_servers);
    }

    // 3. AGENTZERO_MCP_SERVERS env var (legacy / override).
    if let Ok(raw) = std::env::var("AGENTZERO_MCP_SERVERS") {
        if let Ok(env_servers) = serde_json::from_str::<HashMap<String, McpServerEntry>>(&raw) {
            servers.extend(env_servers);
        } else {
            eprintln!("warning: ignoring invalid AGENTZERO_MCP_SERVERS env var");
        }
    }

    // Filter by allowed_servers if the list is non-empty.
    if !allowed_servers.is_empty() {
        servers.retain(|name, _| allowed_servers.contains(name));
    }

    // Convert McpServerEntry → McpServerDef.
    servers
        .into_iter()
        .map(|(name, entry)| {
            (
                name,
                McpServerDef {
                    command: entry.command,
                    args: entry.args,
                    env: entry.env,
                },
            )
        })
        .collect()
}

/// Read and parse an `mcp.json` file, returning `None` if it doesn't exist
/// or is invalid (with a warning log).
fn read_mcp_json(path: &Path) -> Option<McpServersFile> {
    if !path.exists() {
        return None;
    }
    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(_) => return None,
    };
    match serde_json::from_str::<McpServersFile>(&raw) {
        Ok(file) => Some(file),
        Err(_) => {
            eprintln!("warning: ignoring invalid {}", path.display());
            None
        }
    }
}
