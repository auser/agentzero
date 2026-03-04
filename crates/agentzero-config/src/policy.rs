use crate::loader::load;
use agentzero_tools::{
    ReadFilePolicy, ShellPolicy, ToolSecurityPolicy, UrlAccessPolicy, WriteFilePolicy,
};
use anyhow::Context;
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
            agentzero_common::url_policy::CidrRange::parse(cidr_str).with_context(|| {
                format!("invalid CIDR in security.url_access.allow_cidrs: {cidr_str}")
            })?,
        );
    }

    let enable_git = config.security.allowed_commands.iter().any(|c| c == "git");

    Ok(ToolSecurityPolicy {
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
            forbidden_chars: config.security.shell.forbidden_chars,
            command_policy: Default::default(),
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
        },
        enable_write_file: config.security.write_file.enabled,
        enable_mcp: config.security.mcp.enabled,
        allowed_mcp_servers: config.security.mcp.allowed_servers,
        enable_process_plugin: config.security.plugin.enabled,
        enable_git,
        enable_cron: true,
        enable_web_search: config.web_search.enabled,
        enable_browser: config.browser.enabled,
        enable_browser_open: config.browser.enabled,
        enable_composio: false,
        enable_pushover: false,
        enable_wasm_plugins: false,
    })
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
