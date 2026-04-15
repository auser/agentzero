use agentzero_cli::{execute, parse_cli_from};
use base64::Engine as _;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "az-daily-driver-{prefix}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("temp dir should be created");
    dir
}

async fn run_cmd(args: &[&str]) -> anyhow::Result<()> {
    let cli = parse_cli_from(args).map_err(|e| anyhow::anyhow!("parse error: {e}"))?;
    execute(cli).await
}

#[cfg(unix)]
#[tokio::test]
async fn daily_driver_full_lifecycle() {
    let dir = temp_dir("full");
    let d = dir.to_str().unwrap();

    // 1. Setup environment
    let key_bytes = [0u8; 32];
    let key_base64 = base64::engine::general_purpose::STANDARD.encode(key_bytes);
    fs::write(dir.join(".agentzero-data.key"), key_base64.clone()).expect("write key");

    // Start mock server for LLM
    let server = MockServer::start().await;
    let server_uri = server.uri();

    // Mock OpenAI response
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({
                    "id": "chatcmpl-123",
                    "object": "chat.completion",
                    "created": 1677652288,
                    "model": "gpt-4o",
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "I am your daily driver agent. How can I help?"
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 1000,
                        "completion_tokens": 1000,
                        "total_tokens": 2000
                    }
                }))
                .set_delay(Duration::from_millis(10)),
        )
        .mount(&server)
        .await;

    let config_content = format!(
        r#"
[provider]
kind = "openai"
model = "gpt-4o"
base_url = "{}/v1"
api_key = "test-key"

[agent]
name = "DailyDriver"
system_prompt = "You are a daily driver."

[security]
enable_write_file = true

[cost]
enabled = true
"#,
        server_uri
    );

    let config_path = dir.join("agentzero.toml");
    fs::write(&config_path, config_content).expect("write config");

    // 2. Run agent (Execution)
    let config_path_str = config_path.to_str().unwrap();
    let base_url_env = format!("{}/v1", server_uri);

    // Set global env vars to force the CLI to use our temp data
    std::env::set_var("AGENTZERO_DATA_KEY", &key_base64);
    std::env::set_var("AGENTZERO_DATA_DIR", d);
    std::env::set_var("AGENTZERO_CONFIG", config_path_str);
    std::env::set_var("AGENTZERO_PROVIDER__KIND", "openai");
    std::env::set_var("AGENTZERO_PROVIDER__BASE_URL", &base_url_env);
    std::env::set_var("AGENTZERO_PROVIDER__API_KEY", "test-key");
    std::env::set_var("OPENAI_API_KEY", "test-key");

    // Force cleaning up other keys
    std::env::remove_var("ANTHROPIC_API_KEY");
    std::env::remove_var("OPENROUTER_API_KEY");

    run_cmd(&["agentzero", "agent", "-m", "Hello agent"])
        .await
        .expect("agent run should succeed");

    // 3. Validate "Growing Engine" (Trajectory recorded)
    let traj_file = dir.join("trajectories").join("successful.jsonl");
    // Wait for disk flush
    for _ in 0..20 {
        if traj_file.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(
        traj_file.exists(),
        "trajectory file should be created at {}",
        traj_file.display()
    );

    let content = fs::read_to_string(&traj_file).expect("read trajectory");
    let record: serde_json::Value = serde_json::from_str(&content).expect("parse trajectory");
    assert_eq!(record["model"], "gpt-4o");

    let cost = record["cost_microdollars"].as_u64().unwrap_or(0);
    // gpt-4o price is 2.5/Mtok in, 10/Mtok out. 1000 in + 1000 out = 2500 + 10000 = 12500 microdollars.
    assert!(cost > 0, "cost_microdollars should be > 0, got {}", cost);
    assert_eq!(cost, 12500, "expected exact cost calculation");

    // 4. Validate "Self-Updating" (Mocked check)
    std::env::set_var("AGENTZERO_UPDATE_LATEST", "1.0.0");
    run_cmd(&["agentzero", "update", "check", "--json"])
        .await
        .expect("update check should succeed");

    // 5. Cleanup
    std::env::remove_var("AGENTZERO_DATA_KEY");
    std::env::remove_var("AGENTZERO_DATA_DIR");
    std::env::remove_var("AGENTZERO_CONFIG");
    std::env::remove_var("AGENTZERO_PROVIDER__KIND");
    std::env::remove_var("AGENTZERO_PROVIDER__BASE_URL");
    std::env::remove_var("AGENTZERO_PROVIDER__API_KEY");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("AGENTZERO_UPDATE_LATEST");

    let _ = fs::remove_dir_all(dir);
}
