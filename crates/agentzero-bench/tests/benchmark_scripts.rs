use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bench crate parent should exist")
        .parent()
        .expect("workspace root should exist")
        .to_path_buf()
}

#[test]
fn bench_cli_startup_script_help_succeeds() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/bench-cli-startup.sh")
        .arg("--help")
        .current_dir(&root)
        .output()
        .expect("script should execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: scripts/bench-cli-startup.sh"));
}

#[test]
fn bench_single_message_script_fails_without_api_key_by_default() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/bench-single-message.sh")
        .arg("--iterations")
        .arg("1")
        .current_dir(&root)
        .env_remove("OPENAI_API_KEY")
        .output()
        .expect("script should execute");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("OPENAI_API_KEY must be set"));
}

#[test]
fn check_binary_size_script_help_succeeds() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/check-binary-size.sh")
        .arg("--help")
        .current_dir(&root)
        .output()
        .expect("script should execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: scripts/check-binary-size.sh"));
}

#[test]
fn check_binary_size_script_fails_for_missing_binary() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/check-binary-size.sh")
        .arg("--binary")
        .arg("target/release/does-not-exist")
        .current_dir(&root)
        .output()
        .expect("script should execute");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Binary not found"));
}

#[test]
fn check_binary_size_script_passes_with_large_enough_budget() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/check-binary-size.sh")
        .arg("--binary")
        .arg("scripts/check-binary-size.sh")
        .arg("--max-bytes")
        .arg("1000000")
        .current_dir(&root)
        .output()
        .expect("script should execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS:"));
}

#[test]
fn verify_release_version_script_help_succeeds() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/verify-release-version.sh")
        .arg("--help")
        .current_dir(&root)
        .output()
        .expect("script should execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: scripts/verify-release-version.sh"));
}

#[test]
fn verify_release_version_script_fails_without_version() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/verify-release-version.sh")
        .current_dir(&root)
        .output()
        .expect("script should execute");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--version is required"));
}

#[test]
fn run_coverage_script_help_succeeds() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/run-coverage.sh")
        .arg("--help")
        .current_dir(&root)
        .output()
        .expect("script should execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: scripts/run-coverage.sh"));
}

#[test]
fn run_coverage_script_fails_on_unknown_arg() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/run-coverage.sh")
        .arg("--bad-flag")
        .current_dir(&root)
        .output()
        .expect("script should execute");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown argument: --bad-flag"));
}

#[test]
fn run_security_audits_script_help_succeeds() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/run-security-audits.sh")
        .arg("--help")
        .current_dir(&root)
        .output()
        .expect("script should execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: scripts/run-security-audits.sh"));
}

#[test]
fn run_security_audits_script_fails_on_unknown_arg() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/run-security-audits.sh")
        .arg("--unknown")
        .current_dir(&root)
        .output()
        .expect("script should execute");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown argument: --unknown"));
}

#[test]
fn verify_dependency_policy_script_help_succeeds() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/verify-dependency-policy.sh")
        .arg("--help")
        .current_dir(&root)
        .output()
        .expect("script should execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: scripts/verify-dependency-policy.sh"));
}

#[test]
fn verify_dependency_policy_script_fails_on_unknown_arg() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/verify-dependency-policy.sh")
        .arg("--bad-arg")
        .current_dir(&root)
        .output()
        .expect("script should execute");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown argument: --bad-arg"));
}

#[test]
fn verify_dependency_policy_script_passes_for_current_repo_state() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg("scripts/verify-dependency-policy.sh")
        .current_dir(&root)
        .output()
        .expect("script should execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS: dependency update policy is configured and documented"));
}
