//! End-to-end tests for the `az` CLI binary.
//!
//! Each test spawns the real binary via `assert_cmd` and runs in an isolated
//! temporary directory to avoid polluting real state.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a `Command` for the `az` binary.
fn az() -> Command {
    Command::cargo_bin("az").unwrap()
}

/// Build a `Command` for `az` that runs in the given directory.
fn az_in(dir: &std::path::Path) -> Command {
    let mut cmd = az();
    cmd.current_dir(dir);
    cmd
}

/// Run `az init` in a fresh temp directory and return the handle.
fn init_project() -> TempDir {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path()).arg("init").assert().success();
    tmp
}

/// Run `az init --private` in a fresh temp directory and return the handle.
fn init_private_project() -> TempDir {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args(["init", "--private"])
        .assert()
        .success();
    tmp
}

/// Run `az brain init --root <dir>` and return the root path within the temp dir.
fn init_brain(tmp: &TempDir) -> std::path::PathBuf {
    let root = tmp.path().join("vault");
    az_in(tmp.path())
        .args(["brain", "init", "--root", root.to_str().unwrap()])
        .assert()
        .success();
    root
}

// ===========================================================================
// Group A: CLI framework
// ===========================================================================

#[test]
fn test_help() {
    az().arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("AgentZero"));
}

#[test]
fn test_version() {
    az().arg("--version").assert().success();
}

#[test]
fn test_unknown_command() {
    az().arg("nonexistent").assert().failure().code(2);
}

#[test]
fn test_completions_bash() {
    az().args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("az"));
}

#[test]
fn test_completions_zsh() {
    az().args(["completions", "zsh"]).assert().success();
}

#[test]
fn test_completions_fish() {
    az().args(["completions", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("az"));
}

// ===========================================================================
// Group B: az init
// ===========================================================================

#[test]
fn test_init_default() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("default mode"));

    let az_dir = tmp.path().join(".agentzero");
    assert!(az_dir.join("settings.toml").exists());
    assert!(az_dir.join("policy.yml").exists());
    assert!(az_dir.join("models.json").exists());
    for sub in [
        "audit", "sessions", "prompts", "skills", "vault", "index", "plugins",
    ] {
        assert!(az_dir.join(sub).is_dir(), "missing dir: {sub}");
    }
}

#[test]
fn test_init_private() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args(["init", "--private"])
        .assert()
        .success()
        .stdout(predicate::str::contains("private mode"));

    let policy = std::fs::read_to_string(tmp.path().join(".agentzero/policy.yml")).unwrap();
    assert!(policy.contains("network = \"deny\""));
    assert!(policy.contains("wasm_execution = \"deny\""));
}

#[test]
fn test_init_already_initialized() {
    let tmp = init_project();
    az_in(tmp.path())
        .arg("init")
        .assert()
        .failure()
        .stderr(predicate::str::contains("already initialized"));
}

#[test]
fn test_init_editor_vscode() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args(["init", "--editor", "vscode"])
        .assert()
        .success();

    assert!(tmp.path().join(".vscode").is_dir());
}

// ===========================================================================
// Group C: az doctor
// ===========================================================================

#[test]
fn test_doctor_without_init() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("AgentZero Doctor"));
}

#[test]
fn test_doctor_with_init() {
    let tmp = init_project();
    az_in(tmp.path()).arg("doctor").assert().success().stdout(
        predicate::str::contains("AgentZero Doctor").and(predicate::str::contains("Policy")),
    );
}

// ===========================================================================
// Group D: az demo
// ===========================================================================

#[test]
fn test_demo() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path()).arg("demo").assert().success().stdout(
        predicate::str::contains("AgentZero Demo")
            .and(predicate::str::contains("Session:"))
            .and(predicate::str::contains("Policy engine:"))
            .and(predicate::str::contains("Built-in tools"))
            .and(predicate::str::contains("Sandbox:")),
    );
}

// ===========================================================================
// Group E: az history
// ===========================================================================

#[test]
fn test_history_no_init() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .arg("history")
        .assert()
        .failure()
        .stdout(predicate::str::contains("Run `az init` first"));
}

#[test]
fn test_history_empty() {
    let tmp = init_project();
    az_in(tmp.path())
        .arg("history")
        .assert()
        .success()
        .stdout(predicate::str::contains("No past sessions found"));
}

// ===========================================================================
// Group F: Policy tests
// ===========================================================================

#[test]
fn test_policy_status_no_init() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args(["policy", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No policy file found. Using deny-by-default.",
        ));
}

#[test]
fn test_policy_status_after_init() {
    let tmp = init_project();
    az_in(tmp.path())
        .args(["policy", "status"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Policy file:").and(predicate::str::contains("Active rules")),
        );
}

#[test]
fn test_policy_default_shows_correct_values() {
    let tmp = init_project();
    let output = az_in(tmp.path())
        .args(["policy", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("model_routing = \"local_preferred\""));
    assert!(stdout.contains("network = \"require_approval\""));
    assert!(stdout.contains("wasm_execution = \"require_approval\""));
}

#[test]
fn test_policy_private_shows_correct_values() {
    let tmp = init_private_project();
    let output = az_in(tmp.path())
        .args(["policy", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("model_routing = \"local_only\""));
    assert!(stdout.contains("network = \"deny\""));
    assert!(stdout.contains("wasm_execution = \"deny\""));
}

#[test]
fn test_policy_custom_allow_all() {
    let tmp = init_project();
    let custom_policy = concat!(
        "version = 1\n",
        "default_classification = \"private\"\n",
        "model_routing = \"local_preferred\"\n",
        "shell_commands = \"allow\"\n",
        "file_write = \"allow\"\n",
        "file_read = \"allow\"\n",
        "network = \"allow\"\n",
        "wasm_execution = \"allow\"\n",
    );
    std::fs::write(tmp.path().join(".agentzero/policy.yml"), custom_policy).unwrap();

    az_in(tmp.path())
        .args(["policy", "status"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("shell_commands = \"allow\"")
                .and(predicate::str::contains("network = \"allow\""))
                .and(predicate::str::contains("wasm_execution = \"allow\"")),
        );

    // Doctor should also load it without error
    az_in(tmp.path()).arg("doctor").assert().success();
}

#[test]
fn test_policy_custom_deny_all() {
    let tmp = init_project();
    let custom_policy = concat!(
        "version = 1\n",
        "default_classification = \"private\"\n",
        "model_routing = \"local_only\"\n",
        "shell_commands = \"deny\"\n",
        "file_write = \"deny\"\n",
        "file_read = \"deny\"\n",
        "network = \"deny\"\n",
        "wasm_execution = \"deny\"\n",
    );
    std::fs::write(tmp.path().join(".agentzero/policy.yml"), custom_policy).unwrap();

    az_in(tmp.path())
        .args(["policy", "status"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("shell_commands = \"deny\"")
                .and(predicate::str::contains("network = \"deny\""))
                .and(predicate::str::contains("wasm_execution = \"deny\"")),
        );
}

#[test]
fn test_policy_malformed() {
    let tmp = init_project();
    std::fs::write(
        tmp.path().join(".agentzero/policy.yml"),
        "this is not valid toml {{{{",
    )
    .unwrap();

    // Doctor should handle gracefully (still exit 0, show error/warning for policy)
    az_in(tmp.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("AgentZero Doctor"));
}

// ===========================================================================
// Group G: az audit
// ===========================================================================

#[test]
fn test_audit_tail_no_init() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args(["audit", "tail"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("No audit directory found"));
}

#[test]
fn test_audit_tail_empty() {
    let tmp = init_project();
    az_in(tmp.path())
        .args(["audit", "tail"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No audit logs found"));
}

#[test]
fn test_audit_summary_json() {
    let tmp = init_project();
    let output = az_in(tmp.path())
        .args(["audit", "summary", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should be valid JSON
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
    assert!(
        parsed.is_ok(),
        "audit summary --json should produce valid JSON, got: {stdout}"
    );
}

// ===========================================================================
// Group H: az vault
// ===========================================================================

#[test]
fn test_vault_no_init() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args(["vault", "list"])
        .write_stdin("testpass\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Run `az init` first"));
}

#[test]
fn test_vault_list_empty() {
    let tmp = init_project();
    az_in(tmp.path())
        .args(["vault", "list"])
        .write_stdin("testpass\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("No secrets stored"));
}

#[test]
fn test_vault_roundtrip() {
    let tmp = init_project();

    // Add a secret
    az_in(tmp.path())
        .args(["vault", "add", "github", "token"])
        .write_stdin("testpass\nmy_secret_value\n")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Stored: handle://vault/github/token",
        ));

    // Get the secret
    az_in(tmp.path())
        .args(["vault", "get", "github", "token"])
        .write_stdin("testpass\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("my_secret_value"));

    // Remove the secret
    az_in(tmp.path())
        .args(["vault", "remove", "github", "token"])
        .write_stdin("testpass\n")
        .assert()
        .success();

    // Verify it's gone
    az_in(tmp.path())
        .args(["vault", "list"])
        .write_stdin("testpass\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("No secrets stored"));
}

#[test]
fn test_vault_empty_passphrase() {
    let tmp = init_project();
    az_in(tmp.path())
        .args(["vault", "list"])
        .write_stdin("\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("passphrase cannot be empty"));
}

// ===========================================================================
// Group I: az vault-import
// ===========================================================================

#[test]
fn test_vault_import_dry_run() {
    let tmp = TempDir::new().unwrap();
    let env_file = tmp.path().join(".env");
    std::fs::write(&env_file, "FOO=bar\nBAZ=qux\n").unwrap();

    az_in(tmp.path())
        .args(["vault-import", ".env", "--dry-run"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("FOO")
                .and(predicate::str::contains("BAZ"))
                .and(predicate::str::contains("dry-run")),
        );
}

#[test]
fn test_vault_import_missing_file() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args(["vault-import", "nonexistent.env"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("File not found"));
}

// ===========================================================================
// Group J: az chat (single-shot --print mode)
// ===========================================================================

#[test]
fn test_chat_print_no_server() {
    let tmp = TempDir::new().unwrap();
    // Use an unreachable URL to guarantee connection failure regardless of
    // whether Ollama is running on this machine.
    az_in(tmp.path())
        .args(["chat", "-P", "hello", "--url", "http://127.0.0.1:19998"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("cannot connect to Ollama")
                .or(predicate::str::contains("Make sure Ollama is running")),
        );
}

#[test]
fn test_chat_print_unreachable_url() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args(["chat", "-P", "hello", "--url", "http://127.0.0.1:19999"])
        .assert()
        .failure();
}

#[test]
fn test_chat_print_bad_provider() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args([
            "chat",
            "-P",
            "hello",
            "--provider",
            "llama-cpp",
            "--url",
            "http://127.0.0.1:19999",
        ])
        .assert()
        .failure();
}

#[test]
fn test_chat_shows_header() {
    let tmp = TempDir::new().unwrap();
    let output = az_in(tmp.path())
        .args(["chat", "-P", "hello"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("AgentZero Chat"),
        "chat should print header before failing, got stdout: {stdout}"
    );
}

#[test]
fn test_chat_loads_policy() {
    let tmp = init_project();
    let output = az_in(tmp.path())
        .args(["chat", "-P", "hello"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Policy loaded:"),
        "chat with init should show 'Policy loaded:', got stdout: {stdout}"
    );
}

#[test]
fn test_chat_no_policy() {
    let tmp = TempDir::new().unwrap();
    let output = az_in(tmp.path())
        .args(["chat", "-P", "hello"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No policy file found"),
        "chat without init should show 'No policy file found', got stdout: {stdout}"
    );
}

// ===========================================================================
// Group K: az run (built-in skills)
// ===========================================================================

#[test]
fn test_run_security_audit() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args(["run", "repo-security-audit"])
        .assert()
        .stdout(predicate::str::contains("Running repo-security-audit"));
}

#[test]
fn test_run_secrets_scan() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args(["run", "secrets-scan"])
        .assert()
        .stdout(predicate::str::contains("Running secrets-scan"));
}

#[test]
fn test_run_unknown_skill() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args(["run", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown skill"));
}

// ===========================================================================
// Group L: az install
// ===========================================================================

#[test]
fn test_install_nonexistent_local() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args(["install", "./nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no SKILL.md found"));
}

#[test]
fn test_install_local_no_skill_md() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp.path().join("my-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();

    az_in(tmp.path())
        .args(["install", skill_dir.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no SKILL.md found"));
}

// ===========================================================================
// Group M: az link
// ===========================================================================

#[test]
fn test_link_nonexistent() {
    let tmp = TempDir::new().unwrap();
    az_in(tmp.path())
        .args(["link", "/nonexistent/path"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn test_link_invalid_source() {
    let tmp = TempDir::new().unwrap();
    let bad_dir = tmp.path().join("not-a-skill");
    std::fs::create_dir_all(&bad_dir).unwrap();

    az_in(tmp.path())
        .args(["link", bad_dir.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "doesn't look like a skill directory",
        ));
}

// ===========================================================================
// Group N: az plugin
// ===========================================================================

#[test]
fn test_plugin_list_empty() {
    let tmp = init_project();
    az_in(tmp.path())
        .args(["plugin", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No plugins installed"));
}

#[test]
fn test_plugin_info_nonexistent() {
    let tmp = init_project();
    az_in(tmp.path())
        .args(["plugin", "info", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

// ===========================================================================
// Group O: az brain
// ===========================================================================

#[test]
fn test_brain_init() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    az_in(tmp.path())
        .args(["brain", "init", "--root", root.to_str().unwrap()])
        .assert()
        .success();

    assert!(root.exists());
}

#[test]
fn test_brain_init_dry_run() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    az_in(tmp.path())
        .args([
            "brain",
            "init",
            "--root",
            root.to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("dry-run"));
}

#[test]
fn test_brain_status() {
    let tmp = TempDir::new().unwrap();
    let root = init_brain(&tmp);
    az_in(tmp.path())
        .args(["brain", "status", "--root", root.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn test_brain_today() {
    let tmp = TempDir::new().unwrap();
    let root = init_brain(&tmp);
    az_in(tmp.path())
        .args(["brain", "today", "--root", root.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn test_brain_capture_and_query() {
    let tmp = TempDir::new().unwrap();
    let root = init_brain(&tmp);
    let root_str = root.to_str().unwrap();

    // Capture a thought
    az_in(tmp.path())
        .args([
            "brain",
            "capture",
            "e2e test thought zebra",
            "--root",
            root_str,
        ])
        .assert()
        .success();

    // Query for it
    az_in(tmp.path())
        .args(["brain", "query", "zebra", "--root", root_str])
        .assert()
        .success()
        .stdout(predicate::str::contains("zebra"));
}

#[test]
fn test_brain_health() {
    let tmp = TempDir::new().unwrap();
    let root = init_brain(&tmp);
    az_in(tmp.path())
        .args(["brain", "health", "--root", root.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn test_brain_health_json() {
    let tmp = TempDir::new().unwrap();
    let root = init_brain(&tmp);
    let output = az_in(tmp.path())
        .args([
            "brain",
            "health",
            "--root",
            root.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
    assert!(
        parsed.is_ok(),
        "brain health --json should produce valid JSON, got: {stdout}"
    );
}

#[test]
fn test_brain_review() {
    let tmp = TempDir::new().unwrap();
    let root = init_brain(&tmp);
    let root_str = root.to_str().unwrap();

    // Capture something so there's content to review
    az_in(tmp.path())
        .args([
            "brain",
            "capture",
            "review test thought",
            "--root",
            root_str,
        ])
        .assert()
        .success();

    az_in(tmp.path())
        .args(["brain", "review", "--root", root_str])
        .assert()
        .success();
}

#[test]
fn test_brain_weekly() {
    let tmp = TempDir::new().unwrap();
    let root = init_brain(&tmp);
    az_in(tmp.path())
        .args(["brain", "weekly", "--root", root.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn test_brain_ingest() {
    let tmp = TempDir::new().unwrap();
    let root = init_brain(&tmp);
    let root_str = root.to_str().unwrap();

    // Create a file inside the vault root (path validator rejects paths outside root)
    let ingest_file = root.join("notes.txt");
    std::fs::write(&ingest_file, "Some raw notes to ingest into the vault.\n").unwrap();

    az_in(tmp.path())
        .args([
            "brain",
            "ingest",
            ingest_file.to_str().unwrap(),
            "--root",
            root_str,
        ])
        .assert()
        .success();
}

#[test]
fn test_brain_checkpoint() {
    let tmp = TempDir::new().unwrap();
    let root = init_brain(&tmp);
    let root_str = root.to_str().unwrap();

    az_in(tmp.path())
        .args([
            "brain",
            "checkpoint",
            "--root",
            root_str,
            "--init",
            "--message",
            "e2e test checkpoint",
        ])
        .assert()
        .success();
}

// ===========================================================================
// Group P: az search
// ===========================================================================

#[test]
fn test_search_no_network() {
    let tmp = TempDir::new().unwrap();
    // Without a cached index, this should fail gracefully
    az_in(tmp.path())
        .args(["search", "test"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Failed to load skill index"));
}

// ===========================================================================
// Group Q: az bootstrap
// ===========================================================================

#[test]
fn test_bootstrap_detection() {
    let tmp = TempDir::new().unwrap();
    // Use piped stdin with empty input to hit the "skip" path in interactive mode
    az_in(tmp.path())
        .arg("bootstrap")
        .write_stdin("\n")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("AgentZero Bootstrap")
                .and(predicate::str::contains("Platform:")),
        );
}

// ===========================================================================
// Group R: az serve
// ===========================================================================

#[test]
fn test_serve_header() {
    let tmp = TempDir::new().unwrap();
    // The serve command reads stdin for JSON-RPC messages.
    // Closing stdin immediately (empty input) should cause it to exit.
    let output = az_in(tmp.path())
        .arg("serve")
        .write_stdin("")
        .timeout(std::time::Duration::from_secs(5))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("AgentZero ACP Server"),
        "serve should print header, got stderr: {stderr}"
    );
}
