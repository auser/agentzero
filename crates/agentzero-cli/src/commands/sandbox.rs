use crate::cli::SandboxCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
#[cfg(feature = "yaml-policy")]
use agentzero_config::security_policy::SecurityPolicyFile;
use async_trait::async_trait;
use std::path::{Path, PathBuf};

/// Container name used for the sandbox instance.
const SANDBOX_CONTAINER_NAME: &str = "agentzero-sandbox";
/// Default sandbox Docker image.
const SANDBOX_IMAGE: &str = "agentzero-sandbox:latest";

pub struct SandboxCommand;

#[async_trait]
impl AgentZeroCommand for SandboxCommand {
    type Options = SandboxCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            SandboxCommands::Start {
                image,
                port,
                policy,
                detach,
            } => {
                let image = image.unwrap_or_else(|| SANDBOX_IMAGE.to_string());
                let port = port.unwrap_or(8080);
                let policy_path = resolve_policy_path(ctx, policy.as_deref())?;

                validate_policy_file(&policy_path)?;

                let args =
                    build_docker_run_args(&image, port, &ctx.workspace_root, &policy_path, detach);

                println!("Starting sandbox container...");
                println!("  Image:     {image}");
                println!("  Port:      {port}");
                println!("  Policy:    {}", policy_path.display());
                println!("  Workspace: {}", ctx.workspace_root.display());

                let status = std::process::Command::new("docker")
                    .args(&args)
                    .status()
                    .map_err(|e| anyhow::anyhow!("failed to run docker: {e}"))?;

                if !status.success() {
                    anyhow::bail!("docker run exited with status {status}");
                }

                if detach {
                    println!("Sandbox running in background as `{SANDBOX_CONTAINER_NAME}`.");
                    println!("  Gateway: http://localhost:{port}");
                    println!("  Stop:    agentzero sandbox stop");
                }
            }
            SandboxCommands::Stop => {
                println!("Stopping sandbox container...");
                let _ = std::process::Command::new("docker")
                    .args(["stop", SANDBOX_CONTAINER_NAME])
                    .status();
                let status = std::process::Command::new("docker")
                    .args(["rm", "-f", SANDBOX_CONTAINER_NAME])
                    .status()
                    .map_err(|e| anyhow::anyhow!("failed to run docker: {e}"))?;

                if status.success() {
                    println!("Sandbox container `{SANDBOX_CONTAINER_NAME}` removed.");
                } else {
                    anyhow::bail!("failed to remove sandbox container");
                }
            }
            SandboxCommands::Status { json } => {
                let output = std::process::Command::new("docker")
                    .args(["inspect", SANDBOX_CONTAINER_NAME])
                    .output()
                    .map_err(|e| anyhow::anyhow!("failed to run docker: {e}"))?;

                if !output.status.success() {
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "running": false,
                                "container": SANDBOX_CONTAINER_NAME,
                            })
                        );
                    } else {
                        println!("Sandbox is not running.");
                    }
                    return Ok(());
                }

                let inspect_json: serde_json::Value =
                    serde_json::from_slice(&output.stdout).unwrap_or_default();

                let running = inspect_json
                    .as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|c| c.get("State"))
                    .and_then(|s| s.get("Running"))
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);

                let image_name = inspect_json
                    .as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|c| c.get("Config"))
                    .and_then(|cfg| cfg.get("Image"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");

                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "running": running,
                            "container": SANDBOX_CONTAINER_NAME,
                            "image": image_name,
                        })
                    );
                } else {
                    let state = if running { "running" } else { "stopped" };
                    println!("Sandbox `{SANDBOX_CONTAINER_NAME}`: {state}");
                    println!("  Image: {image_name}");
                }
            }
            SandboxCommands::Shell => {
                let status = std::process::Command::new("docker")
                    .args(["exec", "-it", SANDBOX_CONTAINER_NAME, "/bin/sh"])
                    .status()
                    .map_err(|e| anyhow::anyhow!("failed to run docker exec: {e}"))?;

                if !status.success() {
                    anyhow::bail!(
                        "docker exec exited with status {status}; is the sandbox running?"
                    );
                }
            }
        }

        Ok(())
    }
}

/// Resolve the security-policy.yaml path from explicit flag or workspace convention.
fn resolve_policy_path(ctx: &CommandContext, explicit: Option<&str>) -> anyhow::Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(PathBuf::from(p));
    }
    let conventional = ctx
        .workspace_root
        .join(".agentzero")
        .join("security-policy.yaml");
    Ok(conventional)
}

/// Validate that the policy YAML file exists and is parseable.
fn validate_policy_file(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!(
            "security policy not found at {}; create .agentzero/security-policy.yaml or pass --policy",
            path.display()
        );
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read policy file {}: {e}", path.display()))?;

    // When yaml-policy feature is enabled, fully validate the structure.
    // Otherwise, just check the file is readable.
    #[cfg(feature = "yaml-policy")]
    {
        let _policy: SecurityPolicyFile = serde_yaml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("invalid security policy in {}: {e}", path.display()))?;
    }
    #[cfg(not(feature = "yaml-policy"))]
    {
        let _ = content;
    }

    Ok(())
}

/// Build the `docker run` argument list for the sandbox container.
fn build_docker_run_args(
    image: &str,
    port: u16,
    workspace_root: &Path,
    policy_path: &Path,
    detach: bool,
) -> Vec<String> {
    let mut args = vec!["run".to_string()];

    if detach {
        args.push("-d".to_string());
    }

    args.extend([
        "--name".to_string(),
        SANDBOX_CONTAINER_NAME.to_string(),
        // Mount workspace as read-only
        "-v".to_string(),
        format!("{}:/workspace:ro", workspace_root.display()),
        // Mount policy file into /data if it lives outside workspace
        "-v".to_string(),
        format!("{}:/data/security-policy.yaml:ro", policy_path.display()),
        // Port mapping
        "-p".to_string(),
        format!("{port}:8080"),
        // Security: drop all capabilities, add back only NET_ADMIN for iptables
        "--cap-drop=ALL".to_string(),
        "--cap-add=NET_ADMIN".to_string(),
        // Tmpfs for /tmp (writable, not persisted)
        "--tmpfs".to_string(),
        "/tmp:rw,noexec,nosuid,size=64m".to_string(),
        // Writable /sandbox volume
        "--tmpfs".to_string(),
        "/sandbox:rw,noexec,nosuid,size=256m".to_string(),
        // Resource limits
        "--memory=512m".to_string(),
        "--cpus=1.0".to_string(),
        // Read-only root filesystem
        "--read-only".to_string(),
        // Image
        image.to_string(),
    ]);

    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-sandbox-cmd-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn write_valid_policy(dir: &Path) -> PathBuf {
        let agentzero_dir = dir.join(".agentzero");
        fs::create_dir_all(&agentzero_dir).expect("should create .agentzero dir");
        let policy_path = agentzero_dir.join("security-policy.yaml");
        fs::write(
            &policy_path,
            r#"default: deny
rules:
  - tool: http_request
    egress:
      - api.openai.com
    action: allow
"#,
        )
        .expect("should write policy");
        policy_path
    }

    #[test]
    fn validate_policy_accepts_valid_file_success_path() {
        let dir = temp_dir();
        let policy_path = write_valid_policy(&dir);
        let result = validate_policy_file(&policy_path);
        assert!(
            result.is_ok(),
            "valid policy should be accepted: {result:?}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn validate_policy_rejects_missing_file_negative_path() {
        let dir = temp_dir();
        let missing = dir.join("nonexistent.yaml");
        let err = validate_policy_file(&missing).expect_err("missing file should be rejected");
        assert!(
            err.to_string().contains("security policy not found"),
            "error should mention missing policy: {err}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn validate_policy_rejects_invalid_yaml_negative_path() {
        let dir = temp_dir();
        let path = dir.join("bad.yaml");
        fs::write(&path, "not: [valid: yaml: {{{{").expect("should write file");
        let err = validate_policy_file(&path).expect_err("invalid YAML should be rejected");
        assert!(
            err.to_string().contains("invalid security policy"),
            "error should mention invalid policy: {err}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn validate_policy_rejects_missing_default_key_negative_path() {
        let dir = temp_dir();
        let path = dir.join("no-default.yaml");
        fs::write(&path, "rules:\n  - tool: shell\n    action: allow\n")
            .expect("should write file");
        let err = validate_policy_file(&path).expect_err("missing default key should be rejected");
        assert!(
            err.to_string().contains("invalid security policy"),
            "error should mention invalid policy: {err}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn docker_run_args_correct_mounts_and_ports_success_path() {
        let workspace = PathBuf::from("/home/user/project");
        let policy = PathBuf::from("/home/user/project/.agentzero/security-policy.yaml");
        let args =
            build_docker_run_args("agentzero-sandbox:latest", 9090, &workspace, &policy, true);

        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"-d".to_string()));
        assert!(args.contains(&"--name".to_string()));
        assert!(args.contains(&SANDBOX_CONTAINER_NAME.to_string()));

        // Check workspace mount
        let ws_mount = format!("{}:/workspace:ro", workspace.display());
        assert!(
            args.contains(&ws_mount),
            "should contain workspace mount: {ws_mount}"
        );

        // Check port mapping
        assert!(
            args.contains(&"9090:8080".to_string()),
            "should contain port mapping"
        );

        // Check image is last
        assert_eq!(
            args.last().expect("args should not be empty"),
            "agentzero-sandbox:latest"
        );

        // Check security flags
        assert!(
            args.contains(&"--cap-drop=ALL".to_string()),
            "should drop all capabilities"
        );
        assert!(
            args.contains(&"--cap-add=NET_ADMIN".to_string()),
            "should add NET_ADMIN for iptables"
        );
        assert!(
            args.contains(&"--read-only".to_string()),
            "should have read-only root filesystem"
        );
    }

    #[test]
    fn docker_run_args_without_detach_has_no_dash_d_success_path() {
        let workspace = PathBuf::from("/tmp/ws");
        let policy = PathBuf::from("/tmp/ws/.agentzero/security-policy.yaml");
        let args = build_docker_run_args("img:test", 8080, &workspace, &policy, false);

        assert!(
            !args.contains(&"-d".to_string()),
            "should not contain -d when detach is false"
        );
    }

    #[test]
    fn resolve_policy_path_uses_explicit_when_provided_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };
        let result =
            resolve_policy_path(&ctx, Some("/custom/path/policy.yaml")).expect("should resolve");
        assert_eq!(result, PathBuf::from("/custom/path/policy.yaml"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn resolve_policy_path_falls_back_to_workspace_convention_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };
        let result = resolve_policy_path(&ctx, None).expect("should resolve");
        assert_eq!(result, dir.join(".agentzero").join("security-policy.yaml"));
        let _ = fs::remove_dir_all(dir);
    }
}
