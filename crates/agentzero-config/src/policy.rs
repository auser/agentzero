use crate::loader::load;
use agentzero_tools::{ReadFilePolicy, ShellPolicy, ToolSecurityPolicy, WriteFilePolicy};
use anyhow::Context;
use std::path::{Component, Path, PathBuf};

pub fn load_tool_security_policy(
    workspace_root: &Path,
    config_path: &Path,
) -> anyhow::Result<ToolSecurityPolicy> {
    let config = load(config_path)?;
    let allowed_root = resolve_allowed_root(workspace_root, &config.security.allowed_root)?;

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
        },
        enable_write_file: config.security.write_file.enabled,
        enable_mcp: config.security.mcp.enabled,
        allowed_mcp_servers: config.security.mcp.allowed_servers,
        enable_process_plugin: config.security.plugin.enabled,
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
