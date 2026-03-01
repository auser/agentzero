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
    let dir = std::env::temp_dir().join(format!("az-integ-{prefix}-{nanos}-{seq}"));
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
async fn migrate_openclaw_dry_run_success_path() {
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
        "openclaw",
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
    let result = agentzero_cli::parse_cli_from([
        "agentzero",
        "daemon",
        "--host",
        "127.0.0.1",
        "--port",
        "3000",
    ]);
    assert!(result.is_ok(), "daemon args should parse");
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
