//! Integration tests for CLI commands.
//!
//! Each test exercises a command through the public `parse_cli_from` + `execute` API.
//! Tests use isolated temp directories via `--data-dir` to avoid cross-test interference.
//!
//! NOTE: Tests avoid passing `--json` as it triggers global stdout capture via `gag::BufferRedirect`
//! which is process-global and cannot be used concurrently across parallel tests. The `--json`
//! behavior on individual subcommands is covered by their respective unit tests.

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "az-integ-{prefix}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("temp dir should be created");
    dir
}

fn cleanup(dir: std::path::PathBuf) {
    let _ = fs::remove_dir_all(dir);
}

async fn run_cmd(args: &[&str]) -> anyhow::Result<()> {
    let cli = agentzero_cli::parse_cli_from(args)?;
    agentzero_cli::execute(cli).await
}

// ──────────────────────────────────────────────────────────────
// T1 commands — no config file needed, filesystem-only state
// ──────────────────────────────────────────────────────────────

// ── completions ──

#[tokio::test]
async fn completions_bash_success_path() {
    run_cmd(&["agentzero", "completions", "--shell", "bash"])
        .await
        .expect("bash completions should succeed");
}

#[tokio::test]
async fn completions_zsh_success_path() {
    run_cmd(&["agentzero", "completions", "--shell", "zsh"])
        .await
        .expect("zsh completions should succeed");
}

#[test]
fn completions_invalid_shell_negative_path() {
    let result =
        agentzero_cli::parse_cli_from(["agentzero", "completions", "--shell", "invalid-shell"]);
    assert!(result.is_err(), "invalid shell should fail to parse");
}

// ── config schema ──

#[tokio::test]
async fn config_schema_toml_success_path() {
    let dir = temp_dir("cfg-schema");
    run_cmd(&[
        "agentzero",
        "--data-dir",
        dir.to_str().unwrap(),
        "config",
        "schema",
    ])
    .await
    .expect("config schema should succeed");
    cleanup(dir);
}

// ── config set + get ──

#[tokio::test]
async fn config_set_and_get_round_trip_success_path() {
    let dir = temp_dir("cfg-setget");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "config",
        "set",
        "provider.model",
        "gpt-4o",
    ])
    .await
    .expect("config set should succeed");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "config",
        "get",
        "provider.model",
    ])
    .await
    .expect("config get should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn config_get_missing_key_negative_path() {
    let dir = temp_dir("cfg-getmissing");
    let d = dir.to_str().unwrap();

    fs::write(
        dir.join("agentzero.toml"),
        "[provider]\nkind = \"openrouter\"\n",
    )
    .expect("write config");

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "config",
        "get",
        "nonexistent.key",
    ])
    .await;
    assert!(result.is_err(), "get missing key should fail");

    cleanup(dir);
}

// ── goals ──

#[tokio::test]
async fn goals_add_complete_list_success_path() {
    let dir = temp_dir("goals");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "goals",
        "add",
        "--id",
        "g1",
        "--title",
        "Ship it",
    ])
    .await
    .expect("goals add should succeed");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "goals",
        "complete",
        "--id",
        "g1",
    ])
    .await
    .expect("goals complete should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "goals", "list"])
        .await
        .expect("goals list should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn goals_complete_missing_negative_path() {
    let dir = temp_dir("goals-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "goals",
        "complete",
        "--id",
        "missing",
    ])
    .await;
    assert!(result.is_err(), "completing missing goal should fail");

    cleanup(dir);
}

// ── cost ──

#[tokio::test]
async fn cost_record_status_reset_success_path() {
    let dir = temp_dir("cost");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "cost",
        "record",
        "--tokens",
        "500",
        "--usd",
        "0.01",
    ])
    .await
    .expect("cost record should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "cost", "status"])
        .await
        .expect("cost status should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "cost", "reset"])
        .await
        .expect("cost reset should succeed");

    cleanup(dir);
}

// ── coordination ──

#[tokio::test]
async fn coordination_set_and_status_success_path() {
    let dir = temp_dir("coord");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "coordination",
        "set",
        "--active-workers",
        "3",
        "--queued-tasks",
        "10",
    ])
    .await
    .expect("coordination set should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "coordination", "status"])
        .await
        .expect("coordination status should succeed");

    cleanup(dir);
}

// ── identity ──

#[tokio::test]
async fn identity_upsert_get_add_role_success_path() {
    let dir = temp_dir("identity");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "identity",
        "upsert",
        "--id",
        "alice",
        "--name",
        "Alice",
        "--kind",
        "human",
    ])
    .await
    .expect("identity upsert should succeed");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "identity",
        "get",
        "--id",
        "alice",
    ])
    .await
    .expect("identity get should succeed");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "identity",
        "add-role",
        "--id",
        "alice",
        "--role",
        "admin",
    ])
    .await
    .expect("identity add-role should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn identity_get_missing_negative_path() {
    let dir = temp_dir("identity-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "identity",
        "get",
        "--id",
        "nobody",
    ])
    .await;
    assert!(result.is_err(), "get missing identity should fail");

    cleanup(dir);
}

// ── approval ──

#[tokio::test]
async fn approval_evaluate_success_path() {
    let dir = temp_dir("approval");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "approval",
        "evaluate",
        "--actor",
        "agent-1",
        "--action",
        "deploy",
        "--risk",
        "low",
    ])
    .await
    .expect("approval evaluate should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn approval_evaluate_deny_returns_error_negative_path() {
    let dir = temp_dir("approval-deny");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "approval",
        "evaluate",
        "--actor",
        "agent-1",
        "--action",
        "delete-prod",
        "--risk",
        "critical",
        "--approver",
        "admin-1",
        "--decision",
        "deny",
        "--reason",
        "too risky",
    ])
    .await;
    assert!(result.is_err(), "denied approval should return error");
    assert!(
        result.unwrap_err().to_string().contains("denied"),
        "error should mention denial"
    );

    cleanup(dir);
}

#[tokio::test]
async fn approval_evaluate_allow_success_path() {
    let dir = temp_dir("approval-allow");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "approval",
        "evaluate",
        "--actor",
        "agent-1",
        "--action",
        "deploy-staging",
        "--risk",
        "high",
        "--approver",
        "admin-1",
        "--decision",
        "allow",
    ])
    .await
    .expect("allowed approval should succeed");

    cleanup(dir);
}

// ── cron ──

#[tokio::test]
async fn cron_full_lifecycle_success_path() {
    let dir = temp_dir("cron");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "cron",
        "add",
        "--id",
        "backup",
        "--schedule",
        "0 2 * * *",
        "--command",
        "run-backup.sh",
    ])
    .await
    .expect("cron add should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "cron", "list"])
        .await
        .expect("cron list should succeed");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "cron",
        "update",
        "--id",
        "backup",
        "--schedule",
        "0 3 * * *",
    ])
    .await
    .expect("cron update should succeed");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "cron",
        "pause",
        "--id",
        "backup",
    ])
    .await
    .expect("cron pause should succeed");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "cron",
        "resume",
        "--id",
        "backup",
    ])
    .await
    .expect("cron resume should succeed");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "cron",
        "remove",
        "--id",
        "backup",
    ])
    .await
    .expect("cron remove should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn cron_remove_missing_negative_path() {
    let dir = temp_dir("cron-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "cron",
        "remove",
        "--id",
        "nope",
    ])
    .await;
    assert!(result.is_err(), "removing missing cron task should fail");

    cleanup(dir);
}

// ── hooks ──

#[tokio::test]
async fn hooks_list_empty_success_path() {
    let dir = temp_dir("hooks");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "hooks", "list"])
        .await
        .expect("hooks list should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn hooks_enable_missing_negative_path() {
    let dir = temp_dir("hooks-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "hooks",
        "enable",
        "--name",
        "nope",
    ])
    .await;
    assert!(result.is_err(), "enabling unknown hook should fail");

    cleanup(dir);
}

// ── estop ──

#[tokio::test]
async fn estop_status_success_path() {
    let dir = temp_dir("estop");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "estop", "status"])
        .await
        .expect("estop status should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn estop_engage_and_resume_success_path() {
    let dir = temp_dir("estop-engage");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "estop", "--level", "kill-all"])
        .await
        .expect("estop engage should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "estop", "resume"])
        .await
        .expect("estop resume should succeed");

    cleanup(dir);
}

// ── skill ──

#[tokio::test]
async fn skill_list_empty_success_path() {
    let dir = temp_dir("skill");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "skill", "list"])
        .await
        .expect("skill list should succeed");

    cleanup(dir);
}

// ── integrations ──

#[tokio::test]
async fn integrations_list_success_path() {
    let dir = temp_dir("integ-list");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "integrations", "list"])
        .await
        .expect("integrations list should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn integrations_search_success_path() {
    let dir = temp_dir("integ-search");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "integrations",
        "search",
        "--query",
        "slack",
    ])
    .await
    .expect("integrations search should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn integrations_info_success_path() {
    let dir = temp_dir("integ-info");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "integrations", "info"])
        .await
        .expect("integrations info should succeed");

    cleanup(dir);
}

// ── plugin ──

#[tokio::test]
async fn plugin_new_and_list_success_path() {
    let dir = temp_dir("plugin");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "plugin",
        "new",
        "--id",
        "test-plugin",
        "--out-dir",
        d,
        "--force",
    ])
    .await
    .expect("plugin new should succeed");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "plugin",
        "list",
        "--install-dir",
        d,
    ])
    .await
    .expect("plugin list should succeed");

    cleanup(dir);
}

// ── service ──

#[tokio::test]
async fn service_lifecycle_success_path() {
    let dir = temp_dir("service");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "service", "install"])
        .await
        .expect("service install should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "service", "status"])
        .await
        .expect("service status should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "service", "start"])
        .await
        .expect("service start should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "service", "stop"])
        .await
        .expect("service stop should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "service", "uninstall"])
        .await
        .expect("service uninstall should succeed");

    cleanup(dir);
}

// ── update ──

#[tokio::test]
async fn update_status_success_path() {
    let dir = temp_dir("update-status");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "update", "status"])
        .await
        .expect("update status should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn update_check_success_path() {
    let dir = temp_dir("update-check");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "update", "check"])
        .await
        .expect("update check should succeed");

    cleanup(dir);
}

// ── migrate ──

#[tokio::test]
async fn migrate_import_dry_run_success_path() {
    let dir = temp_dir("migrate");
    let d = dir.to_str().unwrap();

    // Create a fake source directory with at least one known migration file
    let source = dir.join("fake-source");
    fs::create_dir_all(&source).expect("create fake source");
    fs::write(
        source.join("agentzero.toml"),
        "[provider]\nkind = \"test\"\n",
    )
    .expect("write migration file");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "migrate",
        "import",
        "--source",
        source.to_str().unwrap(),
        "--dry-run",
    ])
    .await
    .expect("migrate dry-run should succeed");

    cleanup(dir);
}

// ── rag (feature-gated) ──

#[cfg(feature = "rag")]
#[tokio::test]
async fn rag_ingest_and_query_success_path() {
    let dir = temp_dir("rag");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "rag",
        "ingest",
        "--id",
        "doc1",
        "--text",
        "The quick brown fox jumps over the lazy dog",
    ])
    .await
    .expect("rag ingest should succeed");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "rag",
        "query",
        "--query",
        "fox",
    ])
    .await
    .expect("rag query should succeed");

    cleanup(dir);
}

// ── hardware (feature-gated) ──

#[cfg(feature = "hardware")]
#[tokio::test]
async fn hardware_discover_success_path() {
    let dir = temp_dir("hw-discover");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "hardware", "discover"])
        .await
        .expect("hardware discover should succeed");

    cleanup(dir);
}

#[cfg(feature = "hardware")]
#[tokio::test]
async fn hardware_info_success_path() {
    let dir = temp_dir("hw-info");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "hardware", "info"])
        .await
        .expect("hardware info should succeed");

    cleanup(dir);
}

// ── peripheral (feature-gated) ──

#[cfg(feature = "hardware")]
#[tokio::test]
async fn peripheral_list_empty_success_path() {
    let dir = temp_dir("periph-list");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "peripheral", "list"])
        .await
        .expect("peripheral list should succeed");

    cleanup(dir);
}

#[cfg(feature = "hardware")]
#[tokio::test]
async fn peripheral_add_and_list_success_path() {
    let dir = temp_dir("periph-add");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "peripheral",
        "add",
        "--id",
        "sensor-1",
        "--kind",
        "temperature",
        "--connection",
        "i2c:0x48",
    ])
    .await
    .expect("peripheral add should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "peripheral", "list"])
        .await
        .expect("peripheral list should succeed");

    cleanup(dir);
}

// ── template ──

#[tokio::test]
async fn template_list_success_path() {
    let dir = temp_dir("tpl-list");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "template", "list"])
        .await
        .expect("template list should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn template_list_json_success_path() {
    let dir = temp_dir("tpl-list-json");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "template", "list", "--json"])
        .await
        .expect("template list --json should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn template_show_known_success_path() {
    let dir = temp_dir("tpl-show");
    let d = dir.to_str().unwrap();

    // Must init template first so the file exists on disk
    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "template",
        "init",
        "--name",
        "AGENTS",
        "--dir",
        d,
    ])
    .await
    .expect("template init should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "template", "show", "AGENTS"])
        .await
        .expect("template show AGENTS should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn template_show_unknown_negative_path() {
    let dir = temp_dir("tpl-show-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "template",
        "show",
        "NONEXISTENT",
    ])
    .await;
    assert!(result.is_err(), "show unknown template should fail");

    cleanup(dir);
}

#[tokio::test]
async fn template_init_all_success_path() {
    let dir = temp_dir("tpl-init-all");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "template",
        "init",
        "--dir",
        d,
        "--force",
    ])
    .await
    .expect("template init --force should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn template_init_single_success_path() {
    let dir = temp_dir("tpl-init-one");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "template",
        "init",
        "--name",
        "AGENTS",
        "--dir",
        d,
    ])
    .await
    .expect("template init --name AGENTS should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn template_init_no_overwrite_skips_existing_success_path() {
    let dir = temp_dir("tpl-init-noover");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "template",
        "init",
        "--name",
        "AGENTS",
        "--dir",
        d,
    ])
    .await
    .expect("first template init should succeed");

    // Second init without --force skips existing (does not error)
    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "template",
        "init",
        "--name",
        "AGENTS",
        "--dir",
        d,
    ])
    .await
    .expect("second template init should succeed (skip existing)");

    cleanup(dir);
}

#[tokio::test]
async fn template_validate_success_path() {
    let dir = temp_dir("tpl-validate");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "template",
        "init",
        "--dir",
        d,
        "--force",
    ])
    .await
    .expect("template init should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "template", "validate"])
        .await
        .expect("template validate should succeed");

    cleanup(dir);
}

// ── skill lifecycle ──

#[tokio::test]
async fn skill_new_typescript_success_path() {
    let dir = temp_dir("skill-new-ts");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "skill",
        "new",
        "test-skill",
        "--template",
        "typescript",
        "--dir",
        d,
    ])
    .await
    .expect("skill new typescript should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn skill_new_rust_success_path() {
    let dir = temp_dir("skill-new-rs");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "skill",
        "new",
        "test-skill",
        "--template",
        "rust",
        "--dir",
        d,
    ])
    .await
    .expect("skill new rust should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn skill_templates_success_path() {
    let dir = temp_dir("skill-tpls");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "skill", "templates"])
        .await
        .expect("skill templates should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn skill_test_missing_negative_path() {
    let dir = temp_dir("skill-test-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "skill",
        "test",
        "--name",
        "nonexistent",
    ])
    .await;
    assert!(result.is_err(), "testing missing skill should fail");

    cleanup(dir);
}

#[tokio::test]
async fn skill_audit_missing_negative_path() {
    let dir = temp_dir("skill-audit-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "skill",
        "audit",
        "--name",
        "nonexistent",
    ])
    .await;
    assert!(result.is_err(), "auditing missing skill should fail");

    cleanup(dir);
}

#[tokio::test]
async fn skill_remove_missing_negative_path() {
    let dir = temp_dir("skill-rm-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "skill",
        "remove",
        "--name",
        "nonexistent",
    ])
    .await;
    assert!(result.is_err(), "removing missing skill should fail");

    cleanup(dir);
}

// ── hooks (additional) ──

#[tokio::test]
async fn hooks_disable_missing_negative_path() {
    let dir = temp_dir("hooks-dis-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "hooks",
        "disable",
        "--name",
        "nope",
    ])
    .await;
    assert!(result.is_err(), "disabling unknown hook should fail");

    cleanup(dir);
}

#[tokio::test]
async fn hooks_test_missing_negative_path() {
    let dir = temp_dir("hooks-test-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "hooks",
        "test",
        "--name",
        "nope",
    ])
    .await;
    assert!(result.is_err(), "testing unknown hook should fail");

    cleanup(dir);
}

// ── plugin (additional) ──

#[tokio::test]
async fn plugin_validate_after_new_success_path() {
    let dir = temp_dir("plugin-val");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "plugin",
        "new",
        "--id",
        "val-plugin",
        "--out-dir",
        d,
        "--force",
    ])
    .await
    .expect("plugin new should succeed");

    let manifest = dir.join("manifest.json");
    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "plugin",
        "validate",
        "--manifest",
        manifest.to_str().unwrap(),
    ])
    .await
    .expect("plugin validate should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn plugin_validate_missing_negative_path() {
    let dir = temp_dir("plugin-val-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "plugin",
        "validate",
        "--manifest",
        "/nonexistent/plugin.toml",
    ])
    .await;
    assert!(result.is_err(), "validating missing manifest should fail");

    cleanup(dir);
}

#[tokio::test]
async fn plugin_remove_missing_success_path() {
    let dir = temp_dir("plugin-rm-neg");
    let d = dir.to_str().unwrap();

    // Plugin remove for missing id prints a message but does not error
    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "plugin",
        "remove",
        "--id",
        "nope",
        "--install-dir",
        d,
    ])
    .await
    .expect("plugin remove missing id should succeed (idempotent)");

    cleanup(dir);
}

#[tokio::test]
async fn plugin_list_json_success_path() {
    let dir = temp_dir("plugin-json");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "plugin",
        "list",
        "--json",
        "--install-dir",
        d,
    ])
    .await
    .expect("plugin list --json should succeed");

    cleanup(dir);
}

// ── cron variants ──

#[tokio::test]
async fn cron_add_at_success_path() {
    let dir = temp_dir("cron-addat");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "cron",
        "add-at",
        "--id",
        "job1",
        "--schedule",
        "2026-04-01T12:00:00",
        "--command",
        "echo hi",
    ])
    .await
    .expect("cron add-at should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn cron_add_every_success_path() {
    let dir = temp_dir("cron-addevery");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "cron",
        "add-every",
        "--id",
        "job2",
        "--schedule",
        "5m",
        "--command",
        "echo hi",
    ])
    .await
    .expect("cron add-every should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn cron_once_success_path() {
    let dir = temp_dir("cron-once");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "cron",
        "once",
        "--id",
        "job3",
        "--schedule",
        "2026-04-01T12:00:00",
        "--command",
        "echo hi",
    ])
    .await
    .expect("cron once should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn cron_update_missing_negative_path() {
    let dir = temp_dir("cron-upd-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "cron",
        "update",
        "--id",
        "nonexistent",
        "--schedule",
        "0 4 * * *",
    ])
    .await;
    assert!(result.is_err(), "updating missing cron task should fail");

    cleanup(dir);
}

#[tokio::test]
async fn cron_pause_missing_negative_path() {
    let dir = temp_dir("cron-pause-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "cron",
        "pause",
        "--id",
        "nonexistent",
    ])
    .await;
    assert!(result.is_err(), "pausing missing cron task should fail");

    cleanup(dir);
}

// ── estop levels ──

#[tokio::test]
async fn estop_network_kill_and_resume_success_path() {
    let dir = temp_dir("estop-netkill");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "estop",
        "--level",
        "network-kill",
    ])
    .await
    .expect("estop network-kill should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "estop", "resume", "--network"])
        .await
        .expect("estop resume --network should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn estop_domain_block_and_resume_success_path() {
    let dir = temp_dir("estop-domain");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "estop",
        "--level",
        "domain-block",
        "--domain",
        "evil.com",
    ])
    .await
    .expect("estop domain-block should succeed");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "estop",
        "resume",
        "--domain",
        "evil.com",
    ])
    .await
    .expect("estop resume --domain should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn estop_tool_freeze_and_resume_success_path() {
    let dir = temp_dir("estop-tool");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "estop",
        "--level",
        "tool-freeze",
        "--tool",
        "shell",
    ])
    .await
    .expect("estop tool-freeze should succeed");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "estop",
        "resume",
        "--tool",
        "shell",
    ])
    .await
    .expect("estop resume --tool should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn estop_engage_with_require_otp_success_path() {
    let dir = temp_dir("estop-otp");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "estop",
        "--level",
        "kill-all",
        "--require-otp",
    ])
    .await
    .expect("estop with --require-otp should succeed");

    cleanup(dir);
}

// ── auth token flows ──

#[tokio::test]
async fn auth_paste_token_and_list_success_path() {
    let dir = temp_dir("auth-paste-list");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "auth",
        "paste-token",
        "--provider",
        "anthropic",
        "--token",
        "sk-test-123",
    ])
    .await
    .expect("auth paste-token should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "auth", "list"])
        .await
        .expect("auth list after paste-token should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn auth_paste_token_and_logout_success_path() {
    let dir = temp_dir("auth-paste-logout");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "auth",
        "paste-token",
        "--provider",
        "anthropic",
        "--token",
        "sk-test-456",
    ])
    .await
    .expect("auth paste-token should succeed");

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "auth",
        "logout",
        "--provider",
        "anthropic",
    ])
    .await
    .expect("auth logout should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn auth_use_missing_profile_negative_path() {
    let dir = temp_dir("auth-use-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "auth",
        "use",
        "--provider",
        "anthropic",
        "--profile",
        "nonexistent",
    ])
    .await;
    assert!(result.is_err(), "auth use missing profile should fail");

    cleanup(dir);
}

#[tokio::test]
async fn auth_logout_missing_negative_path() {
    let dir = temp_dir("auth-logout-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "auth",
        "logout",
        "--provider",
        "nonexistent",
    ])
    .await;
    // May be idempotent (ok) or error — either is acceptable
    let _ = result;

    cleanup(dir);
}

#[tokio::test]
async fn auth_setup_token_success_path() {
    let dir = temp_dir("auth-setup-tok");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "auth",
        "setup-token",
        "--provider",
        "openrouter",
        "--token",
        "sk-test-789",
    ])
    .await
    .expect("auth setup-token should succeed");

    cleanup(dir);
}

// ── channel add/remove ──

#[tokio::test]
async fn channel_add_and_list_success_path() {
    let dir = temp_dir("chan-add-list");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "channel", "add", "telegram"])
        .await
        .expect("channel add telegram should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "channel", "list"])
        .await
        .expect("channel list after add should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn channel_add_and_remove_success_path() {
    let dir = temp_dir("chan-add-rm");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "channel", "add", "discord"])
        .await
        .expect("channel add discord should succeed");

    run_cmd(&["agentzero", "--data-dir", d, "channel", "remove", "discord"])
        .await
        .expect("channel remove discord should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn channel_remove_missing_negative_path() {
    let dir = temp_dir("chan-rm-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "channel",
        "remove",
        "nonexistent",
    ])
    .await;
    assert!(result.is_err(), "removing non-existent channel should fail");

    cleanup(dir);
}

// ── doctor ──

#[tokio::test]
async fn doctor_traces_success_path() {
    let dir = temp_dir("doctor-traces");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "doctor",
        "traces",
        "--limit",
        "5",
    ])
    .await
    .expect("doctor traces should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn doctor_traces_with_event_filter_success_path() {
    let dir = temp_dir("doctor-traces-ev");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "doctor",
        "traces",
        "--event",
        "tool",
        "--limit",
        "5",
    ])
    .await
    .expect("doctor traces with event filter should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn doctor_traces_with_contains_filter_success_path() {
    let dir = temp_dir("doctor-traces-ct");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "doctor",
        "traces",
        "--contains",
        "test",
        "--limit",
        "5",
    ])
    .await
    .expect("doctor traces with contains filter should succeed");

    cleanup(dir);
}

// ── completions (additional shells) ──

#[tokio::test]
async fn completions_fish_success_path() {
    run_cmd(&["agentzero", "completions", "--shell", "fish"])
        .await
        .expect("fish completions should succeed");
}

#[tokio::test]
async fn completions_powershell_success_path() {
    run_cmd(&["agentzero", "completions", "--shell", "power-shell"])
        .await
        .expect("powershell completions should succeed");
}

#[tokio::test]
async fn completions_elvish_success_path() {
    run_cmd(&["agentzero", "completions", "--shell", "elvish"])
        .await
        .expect("elvish completions should succeed");
}

// ──────────────────────────────────────────────────────────────
// T2 commands — need minimal config file
// ──────────────────────────────────────────────────────────────

fn write_minimal_config(dir: &std::path::Path) {
    let config = r#"[provider]
kind = "openrouter"
base_url = "https://openrouter.ai/api/v1"
model = "anthropic/claude-sonnet-4-6"

[memory]
backend = "sqlite"
sqlite_path = "agentzero.db"

[agent]
max_tool_iterations = 20
"#;
    fs::write(dir.join("agentzero.toml"), config).expect("write config");
}

#[tokio::test]
async fn providers_list_success_path() {
    let dir = temp_dir("providers");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "providers"])
        .await
        .expect("providers should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn providers_no_color_success_path() {
    let dir = temp_dir("providers-nc");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "providers", "--no-color"])
        .await
        .expect("providers --no-color should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn providers_quota_success_path() {
    let dir = temp_dir("prov-quota");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "providers-quota"])
        .await
        .expect("providers-quota should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn config_show_success_path() {
    let dir = temp_dir("cfg-show");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "config", "show"])
        .await
        .expect("config show should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn config_get_provider_kind_success_path() {
    let dir = temp_dir("cfg-get-kind");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "config",
        "get",
        "provider.kind",
    ])
    .await
    .expect("config get provider.kind should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn models_status_success_path() {
    let dir = temp_dir("models-status");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "models", "status"])
        .await
        .expect("models status should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn models_set_success_path() {
    let dir = temp_dir("models-set");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "models", "set", "gpt-4o"])
        .await
        .expect("models set should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn status_with_config_success_path() {
    let dir = temp_dir("status");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "status"])
        .await
        .expect("status should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn memory_stats_success_path() {
    let dir = temp_dir("mem-stats");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "memory", "stats"])
        .await
        .expect("memory stats should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn memory_list_success_path() {
    let dir = temp_dir("mem-list");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "memory", "list"])
        .await
        .expect("memory list should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn memory_clear_success_path() {
    let dir = temp_dir("mem-clear");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "memory", "clear", "--yes"])
        .await
        .expect("memory clear should succeed");

    cleanup(dir);
}

// ── config (additional) ──

#[tokio::test]
async fn config_schema_json_success_path() {
    let dir = temp_dir("cfg-schema-json");
    run_cmd(&[
        "agentzero",
        "--data-dir",
        dir.to_str().unwrap(),
        "config",
        "schema",
        "--json",
    ])
    .await
    .expect("config schema --json should succeed");
    cleanup(dir);
}

#[tokio::test]
async fn config_show_raw_success_path() {
    let dir = temp_dir("cfg-show-raw");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "config", "show", "--raw"])
        .await
        .expect("config show --raw should succeed");

    cleanup(dir);
}

// ── memory (additional) ──

#[tokio::test]
async fn memory_get_empty_negative_path() {
    let dir = temp_dir("mem-get-empty");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    let result = run_cmd(&["agentzero", "--data-dir", d, "memory", "get"]).await;
    assert!(
        result.is_err(),
        "memory get on empty db should return not-found error"
    );

    cleanup(dir);
}

#[tokio::test]
async fn memory_get_missing_key_negative_path() {
    let dir = temp_dir("mem-get-miss");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "memory",
        "get",
        "--key",
        "nonexistent",
    ])
    .await;
    assert!(
        result.is_err(),
        "memory get missing key should return not-found error"
    );

    cleanup(dir);
}

#[tokio::test]
async fn memory_list_with_limit_success_path() {
    let dir = temp_dir("mem-list-lim");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "memory",
        "list",
        "--limit",
        "5",
        "--offset",
        "0",
    ])
    .await
    .expect("memory list with limit should succeed");

    cleanup(dir);
}

// ── models (additional) ──

#[tokio::test]
async fn models_list_success_path() {
    let dir = temp_dir("models-list");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "models", "list"])
        .await
        .expect("models list should succeed");

    cleanup(dir);
}

// ── auth (partial — list/status only, no external deps) ──

#[tokio::test]
async fn auth_list_success_path() {
    let dir = temp_dir("auth-list");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "auth", "list"])
        .await
        .expect("auth list should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn auth_status_success_path() {
    let dir = temp_dir("auth-status");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "auth", "status"])
        .await
        .expect("auth status should succeed");

    cleanup(dir);
}

// ── tunnel (partial) ──

#[tokio::test]
async fn tunnel_status_missing_negative_path() {
    let dir = temp_dir("tunnel-status");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&["agentzero", "--data-dir", d, "tunnel", "status"]).await;
    assert!(result.is_err(), "tunnel status with no tunnel should fail");

    cleanup(dir);
}

// ── channel (partial — list/doctor only) ──

#[tokio::test]
async fn channel_list_success_path() {
    let dir = temp_dir("chan-list");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "channel", "list"])
        .await
        .expect("channel list should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn channel_doctor_success_path() {
    let dir = temp_dir("chan-doctor");
    let d = dir.to_str().unwrap();

    run_cmd(&["agentzero", "--data-dir", d, "channel", "doctor"])
        .await
        .expect("channel doctor should succeed");

    cleanup(dir);
}

// ── onboard (non-interactive quick mode) ──

#[tokio::test]
async fn onboard_quick_mode_success_path() {
    let dir = temp_dir("onboard");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "onboard",
        "--yes",
        "--provider",
        "openrouter",
        "--model",
        "anthropic/claude-sonnet-4-6",
        "--memory",
        "sqlite",
        "--no-totp",
    ])
    .await
    .expect("onboard quick mode should succeed");

    cleanup(dir);
}

// ── local commands ──

#[tokio::test]
async fn local_discover_success_path() {
    let dir = temp_dir("local-discover");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "local",
        "discover",
        "--timeout-ms",
        "200",
    ])
    .await
    .expect("local discover should succeed even with no services running");

    cleanup(dir);
}

#[tokio::test]
async fn local_status_success_path() {
    let dir = temp_dir("local-status");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "local", "status"])
        .await
        .expect("local status should succeed (reports non-local provider)");

    cleanup(dir);
}

#[tokio::test]
async fn local_health_unknown_provider_negative_path() {
    let dir = temp_dir("local-health-unk");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "local",
        "health",
        "not-a-provider",
    ])
    .await;
    assert!(
        result.is_err(),
        "local health with unknown provider should fail"
    );

    cleanup(dir);
}

#[test]
fn local_discover_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "local",
        "discover",
        "--timeout-ms",
        "500",
        "--json",
    ]);
    assert!(result.is_ok(), "local discover args should parse");
}

#[test]
fn local_health_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "local",
        "health",
        "ollama",
        "--url",
        "http://gpu:11434",
    ]);
    assert!(result.is_ok(), "local health args should parse");
}

// ── models pull ──

#[tokio::test]
async fn models_pull_non_pull_provider_negative_path() {
    let dir = temp_dir("models-pull-nopull");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "models",
        "pull",
        "some-model",
        "--provider",
        "llamacpp",
    ])
    .await;
    assert!(
        result.is_err(),
        "models pull should fail for non-pull provider"
    );

    cleanup(dir);
}

#[test]
fn models_pull_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "models",
        "pull",
        "llama3.1:8b",
        "--provider",
        "ollama",
    ]);
    assert!(result.is_ok(), "models pull args should parse");
}

// ──────────────────────────────────────────────────────────────
// T3 commands — parse-only smoke tests (server/interactive)
// ──────────────────────────────────────────────────────────────

#[test]
fn gateway_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "gateway",
        "--host",
        "0.0.0.0",
        "--port",
        "9090",
    ]);
    assert!(result.is_ok(), "gateway args should parse");
}

#[test]
fn daemon_args_parse_success_path() {
    let start = agentzero_cli::parse_cli_from([
        "agentzero",
        "daemon",
        "start",
        "--host",
        "127.0.0.1",
        "--port",
        "3000",
    ]);
    assert!(start.is_ok(), "daemon start args should parse");

    let stop = agentzero_cli::parse_cli_from(["agentzero", "daemon", "stop"]);
    assert!(stop.is_ok(), "daemon stop should parse");

    let status = agentzero_cli::parse_cli_from(["agentzero", "daemon", "status"]);
    assert!(status.is_ok(), "daemon status should parse");
}

#[test]
fn dashboard_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "dashboard"]);
    assert!(result.is_ok(), "dashboard args should parse");
}

#[test]
fn agent_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "agent", "--message", "hello world"]);
    assert!(result.is_ok(), "agent args should parse");
}

#[test]
fn tunnel_start_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "tunnel",
        "start",
        "--protocol",
        "http",
        "--remote",
        "host:80",
        "--local-port",
        "8080",
    ]);
    assert!(result.is_ok(), "tunnel start args should parse");
}

#[test]
fn channel_start_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "channel", "start"]);
    assert!(result.is_ok(), "channel start args should parse");
}

#[test]
fn auth_login_args_parse_success_path() {
    let result =
        agentzero_cli::parse_cli_from(["agentzero", "auth", "login", "--provider", "openai-codex"]);
    assert!(result.is_ok(), "auth login args should parse");
}

#[test]
fn plugin_dev_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "plugin",
        "dev",
        "--manifest",
        "m.json",
        "--wasm",
        "p.wasm",
    ]);
    assert!(result.is_ok(), "plugin dev args should parse");
}

#[test]
fn plugin_package_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "plugin",
        "package",
        "--manifest",
        "m.json",
        "--wasm",
        "p.wasm",
        "--out",
        "pkg.tar",
    ]);
    assert!(result.is_ok(), "plugin package args should parse");
}

#[test]
fn onboard_interactive_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "onboard", "--interactive"]);
    assert!(result.is_ok(), "onboard --interactive args should parse");
}

// --- Daemon lifecycle integration tests ---

#[test]
fn daemon_manager_start_stop_lifecycle_success_path() {
    let dir = temp_dir("daemon-lifecycle");
    let manager =
        agentzero_cli::daemon::DaemonManager::new(&dir).expect("manager should be created");
    let my_pid = std::process::id();

    // Start
    let started = manager
        .mark_started("127.0.0.1".to_string(), 8080, my_pid)
        .expect("start should succeed");
    assert!(started.running);
    assert_eq!(started.pid, Some(my_pid));

    // PID file
    agentzero_cli::daemon::write_pid_file(&dir, my_pid).expect("pid file write should succeed");
    assert_eq!(agentzero_cli::daemon::read_pid_file(&dir), Some(my_pid));

    // Status
    let status = manager.status().expect("status should succeed");
    assert!(status.running);
    assert!(status.uptime_secs() < 5);

    // Stop
    let stopped = manager.mark_stopped().expect("stop should succeed");
    assert!(!stopped.running);

    // Cleanup PID file
    agentzero_cli::daemon::remove_pid_file(&dir);
    assert!(agentzero_cli::daemon::read_pid_file(&dir).is_none());

    cleanup(dir);
}

#[test]
fn daemon_log_rotation_integration_success_path() {
    let dir = temp_dir("daemon-logrot");
    let log_path = agentzero_cli::daemon::log_file_path(&dir);

    // Write a large log file
    fs::write(&log_path, "x".repeat(500)).expect("log write should succeed");

    let config = agentzero_cli::daemon::LogRotationConfig {
        max_bytes: 100,
        max_files: 3,
    };
    let rotated =
        agentzero_cli::daemon::rotate_log_if_needed(&dir, &config).expect("rotate should succeed");
    assert!(rotated);
    assert!(!log_path.exists(), "original should be rotated away");
    assert!(
        dir.join("daemon.log.1").exists(),
        "rotated file should exist"
    );

    cleanup(dir);
}

#[tokio::test]
async fn auth_setup_token_and_list_round_trip_success_path() {
    let dir = temp_dir("auth-roundtrip");
    let d = dir.to_str().unwrap();

    // Setup a token
    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "auth",
        "setup-token",
        "--provider",
        "openrouter",
        "--token",
        "sk-or-test-token-roundtrip",
    ])
    .await
    .expect("setup-token should succeed");

    // List profiles — should not error
    run_cmd(&["agentzero", "--data-dir", d, "auth", "list"])
        .await
        .expect("auth list should succeed");

    // Status — should show the active profile
    run_cmd(&["agentzero", "--data-dir", d, "auth", "status"])
        .await
        .expect("auth status should succeed");

    cleanup(dir);
}

#[test]
fn local_discover_retries_arg_parses_success_path() {
    let result =
        agentzero_cli::parse_cli_from(["agentzero", "local", "discover", "--retries", "3"]);
    assert!(result.is_ok(), "local discover --retries should parse");
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — tools command (zero prior coverage)
// ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn tools_list_success_path() {
    let dir = temp_dir("tools-list");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "tools", "list"])
        .await
        .expect("tools list should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn tools_list_with_schema_success_path() {
    let dir = temp_dir("tools-list-schema");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "tools",
        "list",
        "--with-schema",
    ])
    .await
    .expect("tools list --with-schema should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn tools_info_known_tool_success_path() {
    let dir = temp_dir("tools-info");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "tools", "info", "read_file"])
        .await
        .expect("tools info read_file should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn tools_info_unknown_tool_negative_path() {
    let dir = temp_dir("tools-info-neg");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "tools",
        "info",
        "nonexistent_tool_xyz",
    ])
    .await;
    assert!(result.is_err(), "tools info with unknown tool should fail");

    cleanup(dir);
}

#[tokio::test]
async fn tools_schema_known_tool_success_path() {
    let dir = temp_dir("tools-schema");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "tools", "schema", "read_file"])
        .await
        .expect("tools schema read_file should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn tools_schema_pretty_success_path() {
    let dir = temp_dir("tools-schema-pretty");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "tools",
        "schema",
        "read_file",
        "--pretty",
    ])
    .await
    .expect("tools schema --pretty should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn tools_schema_unknown_negative_path() {
    let dir = temp_dir("tools-schema-neg");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "tools",
        "schema",
        "nonexistent_tool_xyz",
    ])
    .await;
    assert!(
        result.is_err(),
        "tools schema with unknown tool should fail"
    );

    cleanup(dir);
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — conversation command (zero prior coverage)
// ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn conversation_list_empty_success_path() {
    let dir = temp_dir("conv-list");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "conversation", "list"])
        .await
        .expect("conversation list should succeed with no conversations");

    cleanup(dir);
}

#[tokio::test]
async fn conversation_switch_success_path() {
    let dir = temp_dir("conv-switch");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "conversation",
        "switch",
        "test-conv",
    ])
    .await
    .expect("conversation switch should succeed");

    // Verify the active_conversation state file was written
    let state = fs::read_to_string(dir.join("active_conversation"))
        .expect("active_conversation file should exist");
    assert_eq!(state, "test-conv");

    cleanup(dir);
}

#[tokio::test]
async fn conversation_switch_global_success_path() {
    let dir = temp_dir("conv-switch-global");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    // First switch to a named conversation
    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "conversation",
        "switch",
        "temp",
    ])
    .await
    .expect("switch to named should succeed");

    // Then switch to global (empty string)
    run_cmd(&["agentzero", "--data-dir", d, "conversation", "switch", ""])
        .await
        .expect("conversation switch to global should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn conversation_fork_missing_negative_path() {
    let dir = temp_dir("conv-fork-neg");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    // fork succeeds even with nonexistent source (sqlite creates on-the-fly)
    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "conversation",
        "fork",
        "nonexistent-source",
        "new-target",
    ])
    .await
    .expect("conversation fork should succeed (creates empty fork)");

    cleanup(dir);
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — privacy command (zero prior coverage)
// ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn privacy_status_success_path() {
    let dir = temp_dir("privacy-status");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "privacy", "status"])
        .await
        .expect("privacy status should succeed");

    cleanup(dir);
}

#[test]
fn privacy_status_json_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "privacy", "status", "--json"]);
    assert!(result.is_ok(), "privacy status --json should parse");
}

#[test]
fn privacy_rotate_keys_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "privacy", "rotate-keys", "--force"]);
    assert!(result.is_ok(), "privacy rotate-keys --force should parse");
}

#[test]
fn privacy_generate_keypair_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "privacy", "generate-keypair"]);
    assert!(result.is_ok(), "privacy generate-keypair should parse");
}

#[test]
fn privacy_test_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "privacy", "test"]);
    assert!(result.is_ok(), "privacy test should parse");
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — doctor models (missing subcommand)
// ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn doctor_models_success_path() {
    let dir = temp_dir("doctor-models");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "doctor",
        "models",
        "--use-cache",
    ])
    .await
    .expect("doctor models --use-cache should succeed");

    cleanup(dir);
}

#[tokio::test]
async fn doctor_models_specific_provider_success_path() {
    let dir = temp_dir("doctor-models-prov");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "doctor",
        "models",
        "--provider",
        "openrouter",
        "--use-cache",
    ])
    .await
    .expect("doctor models --provider openrouter should succeed");

    cleanup(dir);
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — models refresh (missing subcommand)
// ──────────────────────────────────────────────────────────────

#[test]
fn models_refresh_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "models",
        "refresh",
        "--provider",
        "openrouter",
    ]);
    assert!(result.is_ok(), "models refresh args should parse");
}

#[test]
fn models_refresh_all_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "models", "refresh", "--all"]);
    assert!(result.is_ok(), "models refresh --all args should parse");
}

#[test]
fn models_refresh_force_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "models",
        "refresh",
        "--provider",
        "openrouter",
        "--force",
    ]);
    assert!(result.is_ok(), "models refresh --force args should parse");
}

#[tokio::test]
async fn models_refresh_unsupported_provider_negative_path() {
    let dir = temp_dir("models-refresh-unsup");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "models",
        "refresh",
        "--provider",
        "not-a-real-provider",
    ])
    .await;
    assert!(
        result.is_err(),
        "models refresh with unsupported provider should fail"
    );

    cleanup(dir);
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — auth gaps (paste-redirect, refresh)
// ──────────────────────────────────────────────────────────────

#[test]
fn auth_paste_redirect_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "auth",
        "paste-redirect",
        "--provider",
        "openai-codex",
        "--input",
        "code123",
    ]);
    assert!(result.is_ok(), "auth paste-redirect args should parse");
}

#[test]
fn auth_refresh_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "auth",
        "refresh",
        "--provider",
        "openai-codex",
    ]);
    assert!(result.is_ok(), "auth refresh args should parse");
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — skill gaps (go, python, install)
// ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn skill_new_go_success_path() {
    let dir = temp_dir("skill-new-go");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "skill",
        "new",
        "test-go-skill",
        "--template",
        "go",
        "--dir",
        d,
    ])
    .await
    .expect("skill new --template go should succeed");

    assert!(
        dir.join("test-go-skill").exists(),
        "go skill project directory should exist"
    );

    cleanup(dir);
}

#[tokio::test]
async fn skill_new_python_success_path() {
    let dir = temp_dir("skill-new-py");
    let d = dir.to_str().unwrap();

    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "skill",
        "new",
        "test-py-skill",
        "--template",
        "python",
        "--dir",
        d,
    ])
    .await
    .expect("skill new --template python should succeed");

    assert!(
        dir.join("test-py-skill").exists(),
        "python skill project directory should exist"
    );

    cleanup(dir);
}

#[test]
fn skill_install_args_parse_success_path() {
    let result =
        agentzero_cli::parse_cli_from(["agentzero", "skill", "install", "--name", "my-skill"]);
    assert!(result.is_ok(), "skill install args should parse");
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — plugin gaps
// ──────────────────────────────────────────────────────────────

#[cfg(feature = "plugins")]
#[tokio::test]
async fn plugin_enable_missing_negative_path() {
    let dir = temp_dir("plugin-enable-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "plugin",
        "enable",
        "--id",
        "nonexistent-plugin",
    ])
    .await;
    assert!(
        result.is_err(),
        "plugin enable on missing plugin should fail"
    );

    cleanup(dir);
}

#[cfg(feature = "plugins")]
#[tokio::test]
async fn plugin_disable_missing_negative_path() {
    let dir = temp_dir("plugin-disable-neg");
    let d = dir.to_str().unwrap();

    let result = run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "plugin",
        "disable",
        "--id",
        "nonexistent-plugin",
    ])
    .await;
    assert!(
        result.is_err(),
        "plugin disable on missing plugin should fail"
    );

    cleanup(dir);
}

#[cfg(feature = "plugins")]
#[tokio::test]
async fn plugin_info_missing_success_path() {
    let dir = temp_dir("plugin-info-miss");
    let d = dir.to_str().unwrap();

    // info on a missing plugin prints "No installed plugin found" and returns Ok
    run_cmd(&[
        "agentzero",
        "--data-dir",
        d,
        "plugin",
        "info",
        "--id",
        "nonexistent-plugin",
    ])
    .await
    .expect("plugin info on missing plugin should succeed (prints not found)");

    cleanup(dir);
}

#[cfg(feature = "plugins")]
#[test]
fn plugin_install_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "plugin",
        "install",
        "--url",
        "https://example.com/plugin.tar",
        "--sha256",
        "abc123def456",
    ]);
    assert!(result.is_ok(), "plugin install args should parse");
}

#[cfg(feature = "plugins")]
#[test]
fn plugin_search_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "plugin", "search", "browser"]);
    assert!(result.is_ok(), "plugin search args should parse");
}

#[cfg(feature = "plugins")]
#[test]
fn plugin_outdated_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "plugin", "outdated"]);
    assert!(result.is_ok(), "plugin outdated args should parse");
}

#[cfg(feature = "plugins")]
#[test]
fn plugin_update_args_parse_success_path() {
    let result =
        agentzero_cli::parse_cli_from(["agentzero", "plugin", "update", "--id", "some-plugin"]);
    assert!(result.is_ok(), "plugin update args should parse");
}

#[cfg(feature = "plugins")]
#[test]
fn plugin_refresh_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "plugin", "refresh"]);
    assert!(result.is_ok(), "plugin refresh args should parse");
}

#[cfg(feature = "plugins")]
#[test]
fn plugin_publish_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "plugin",
        "publish",
        "--manifest",
        "manifest.json",
        "--download-url",
        "https://example.com/plugin.tar",
        "--sha256",
        "abc123",
        "--description",
        "A test plugin",
        "--category",
        "tools",
        "--author",
        "tester",
        "--repository",
        "https://github.com/test/plugin",
    ]);
    assert!(result.is_ok(), "plugin publish args should parse");
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — update gaps (apply, rollback)
// ──────────────────────────────────────────────────────────────

#[test]
fn update_apply_args_parse_success_path() {
    let result =
        agentzero_cli::parse_cli_from(["agentzero", "update", "apply", "--version", "0.5.0"]);
    assert!(result.is_ok(), "update apply args should parse");
}

#[test]
fn update_rollback_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "update", "rollback"]);
    assert!(result.is_ok(), "update rollback args should parse");
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — hardware introspect
// ──────────────────────────────────────────────────────────────

#[cfg(feature = "hardware")]
#[tokio::test]
async fn hardware_introspect_success_path() {
    let dir = temp_dir("hw-introspect");
    let d = dir.to_str().unwrap();
    write_minimal_config(&dir);

    run_cmd(&["agentzero", "--data-dir", d, "hardware", "introspect"])
        .await
        .expect("hardware introspect should succeed");

    cleanup(dir);
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — peripheral gaps (flash, flash-nucleo, setup-uno-q)
// ──────────────────────────────────────────────────────────────

#[test]
fn peripheral_flash_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "peripheral",
        "flash",
        "--id",
        "nucleo1",
        "--firmware",
        "firmware.bin",
    ]);
    assert!(result.is_ok(), "peripheral flash args should parse");
}

#[test]
fn peripheral_flash_nucleo_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from(["agentzero", "peripheral", "flash-nucleo"]);
    assert!(result.is_ok(), "peripheral flash-nucleo args should parse");
}

#[test]
fn peripheral_setup_uno_q_args_parse_success_path() {
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "peripheral",
        "setup-uno-q",
        "--host",
        "192.168.0.48",
    ]);
    assert!(result.is_ok(), "peripheral setup-uno-q args should parse");
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — service restart
// ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn service_restart_after_install_success_path() {
    let dir = temp_dir("svc-restart");
    let d = dir.to_str().unwrap();

    // Install first
    run_cmd(&["agentzero", "--data-dir", d, "service", "install"])
        .await
        .expect("service install should succeed");

    // Restart
    run_cmd(&["agentzero", "--data-dir", d, "service", "restart"])
        .await
        .expect("service restart should succeed after install");

    // Cleanup
    let _ = run_cmd(&["agentzero", "--data-dir", d, "service", "uninstall"]).await;
    cleanup(dir);
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — tunnel stop (parse-only)
// ──────────────────────────────────────────────────────────────

#[test]
fn tunnel_stop_args_parse_success_path() {
    let result =
        agentzero_cli::parse_cli_from(["agentzero", "tunnel", "stop", "--name", "default"]);
    assert!(result.is_ok(), "tunnel stop args should parse");
}

// ──────────────────────────────────────────────────────────────
// Gap coverage — daemon status execution
// ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn daemon_status_no_daemon_success_path() {
    let dir = temp_dir("daemon-status-norun");
    let d = dir.to_str().unwrap();

    // daemon status when no daemon is running should not panic
    let result = run_cmd(&["agentzero", "--data-dir", d, "daemon", "status"]).await;
    // It may succeed (reporting not running) or error — either is acceptable
    // The key assertion is that it doesn't panic
    let _ = result;

    cleanup(dir);
}
