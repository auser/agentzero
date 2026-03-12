use crate::{
    load, load_audit_policy, load_env_var, load_tool_security_policy, update_auto_approve,
};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "agentzero-config-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("temp dir should be created");
    dir
}

fn with_clean_agentzero_env(test: impl FnOnce()) {
    let unset: Option<&str> = None;
    temp_env::with_vars(
        [
            ("AGENTZERO_PROVIDER", unset),
            ("AGENTZERO_PROVIDER__KIND", unset),
            ("AGENTZERO_BASE_URL", unset),
            ("AGENTZERO_PROVIDER__BASE_URL", unset),
            ("AGENTZERO_MODEL", unset),
            ("AGENTZERO_PROVIDER__MODEL", unset),
            ("AGENTZERO_MEMORY_BACKEND", unset),
            ("AGENTZERO_MEMORY__BACKEND", unset),
            ("AGENTZERO_MEMORY_PATH", unset),
            ("AGENTZERO_MEMORY__SQLITE_PATH", unset),
            ("AGENTZERO_AGENT__MEMORY_WINDOW_SIZE", unset),
            ("AGENTZERO_AGENT__MAX_PROMPT_CHARS", unset),
            ("AGENTZERO_ALLOWED_ROOT", unset),
            ("AGENTZERO_SECURITY__ALLOWED_ROOT", unset),
            ("AGENTZERO_ALLOWED_COMMANDS", unset),
            ("AGENTZERO_SECURITY__ALLOWED_COMMANDS", unset),
            ("AGENTZERO_ENV", unset),
            ("APP_ENV", unset),
            ("NODE_ENV", unset),
            ("OPENAI_API_KEY", unset),
        ],
        test,
    );
}

#[test]
fn loads_typed_config_from_toml_file() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"openrouter\"\nbase_url=\"https://openrouter.ai/api\"\nmodel=\"openai/gpt-4o-mini\"\n\n[memory]\nbackend=\"sqlite\"\nsqlite_path=\"./local.db\"\n\n[agent]\nmax_tool_iterations=7\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("typed config should load");
        assert_eq!(cfg.provider.kind, "openrouter");
        assert_eq!(cfg.provider.model, "openai/gpt-4o-mini");
        assert_eq!(cfg.memory.sqlite_path, "./local.db");
        assert_eq!(cfg.agent.max_tool_iterations, 7);
        assert_eq!(cfg.agent.request_timeout_ms, 30_000);
        assert_eq!(cfg.agent.memory_window_size, 50); // new default
        assert_eq!(cfg.agent.max_prompt_chars, 8_000);
        assert_eq!(cfg.security.allowed_commands, vec!["echo".to_string()]);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn loads_user_configured_hook_settings() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[agent]\nmode=\"production\"\nmax_tool_iterations=4\nrequest_timeout_ms=15000\nmemory_window_size=6\nmax_prompt_chars=4096\n\n[agent.hooks]\nenabled=true\ntimeout_ms=500\nfail_closed=true\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("hook settings should load");
        assert!(cfg.agent.hooks.enabled);
        assert_eq!(cfg.agent.hooks.timeout_ms, 500);
        assert!(cfg.agent.hooks.fail_closed);
        assert_eq!(cfg.agent.memory_window_size, 6);
        assert_eq!(cfg.agent.max_prompt_chars, 4096);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn loads_legacy_onboard_field_names() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nname=\"openai\"\nbase_url=\"https://api.openai.com\"\nmodel=\"gpt-4o-mini\"\n\n[memory]\npath=\"./legacy.db\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("legacy field names should parse");
        assert_eq!(cfg.provider.kind, "openai");
        assert_eq!(cfg.memory.sqlite_path, "./legacy.db");
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn env_overrides_file_values() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"openai\"\nbase_url=\"https://api.openai.com\"\nmodel=\"old-model\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        temp_env::with_var("AGENTZERO_PROVIDER__MODEL", Some("new-model"), || {
            let cfg = load(&config_path).expect("typed config should load with env override");
            assert_eq!(cfg.provider.model, "new-model");
        });
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn dotenv_chain_overrides_in_order() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"openai\"\nbase_url=\"https://api.openai.com\"\nmodel=\"from-file\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");
    fs::write(dir.join(".env"), "AGENTZERO_PROVIDER__MODEL=from-dotenv\n")
        .expect(".env should be written");
    fs::write(
        dir.join(".env.local"),
        "AGENTZERO_PROVIDER__MODEL=from-dotenv-local\n",
    )
    .expect(".env.local should be written");
    fs::write(
        dir.join(".env.development"),
        "AGENTZERO_PROVIDER__MODEL=from-dotenv-development\n",
    )
    .expect(".env.development should be written");

    with_clean_agentzero_env(|| {
        temp_env::with_var("AGENTZERO_ENV", Some("development"), || {
            let cfg = load(&config_path).expect("typed config should load with dotenv chain");
            assert_eq!(cfg.provider.model, "from-dotenv-development");
        });
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn process_env_overrides_dotenv_files() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"openai\"\nbase_url=\"https://api.openai.com\"\nmodel=\"from-file\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");
    fs::write(
        dir.join(".env"),
        "AGENTZERO_PROVIDER__MODEL=from-dotenv\nAGENTZERO_ENV=development\n",
    )
    .expect(".env should be written");
    fs::write(
        dir.join(".env.development"),
        "AGENTZERO_PROVIDER__MODEL=from-dotenv-development\n",
    )
    .expect(".env.development should be written");

    with_clean_agentzero_env(|| {
        temp_env::with_var(
            "AGENTZERO_PROVIDER__MODEL",
            Some("from-process-env"),
            || {
                let cfg = load(&config_path).expect("typed config should load with env precedence");
                assert_eq!(cfg.provider.model, "from-process-env");
            },
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn load_env_var_reads_from_dotenv_when_process_env_missing() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(&config_path, "").expect("empty config should be written");
    fs::write(dir.join(".env"), "OPENAI_API_KEY=dotenv-key\n").expect(".env should be written");

    with_clean_agentzero_env(|| {
        temp_env::with_var("OPENAI_API_KEY", None::<&str>, || {
            let key =
                load_env_var(&config_path, "OPENAI_API_KEY").expect("env var load should succeed");
            assert_eq!(key.as_deref(), Some("dotenv-key"));
        });
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn load_env_var_prefers_process_env_over_dotenv() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(&config_path, "").expect("empty config should be written");
    fs::write(dir.join(".env"), "OPENAI_API_KEY=dotenv-key\n").expect(".env should be written");

    with_clean_agentzero_env(|| {
        temp_env::with_var("OPENAI_API_KEY", Some("process-key"), || {
            let key =
                load_env_var(&config_path, "OPENAI_API_KEY").expect("env var load should succeed");
            assert_eq!(key.as_deref(), Some("process-key"));
        });
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn cwd_dotenv_overrides_config_dir_dotenv() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let config_dir = temp_dir();
    let cwd_dir = temp_dir();
    let config_path = config_dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"openai\"\nbase_url=\"https://api.openai.com\"\nmodel=\"from-file\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    // Config dir has a .env with one value
    fs::write(
        config_dir.join(".env"),
        "AGENTZERO_PROVIDER__MODEL=from-config-dir\n",
    )
    .expect("config dir .env should be written");

    // CWD has a .env with a different value — should win
    fs::write(cwd_dir.join(".env"), "AGENTZERO_PROVIDER__MODEL=from-cwd\n")
        .expect("cwd .env should be written");

    let original_dir = std::env::current_dir().expect("should get cwd");
    std::env::set_current_dir(&cwd_dir).expect("should set cwd");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("typed config should load with cwd dotenv");
        assert_eq!(cfg.provider.model, "from-cwd");
    });

    std::env::set_current_dir(&original_dir).expect("should restore cwd");
    fs::remove_dir_all(config_dir).expect("config temp dir should be removed");
    fs::remove_dir_all(cwd_dir).expect("cwd temp dir should be removed");
}

#[test]
fn allows_enabled_mcp_without_allowed_servers() {
    // allowed_servers is now optional — servers come from mcp.json files.
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n\n[security.mcp]\nenabled=true\nallowed_servers=[]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(
            result.is_ok(),
            "mcp enabled with empty allowed_servers should be valid"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_invalid_hook_error_mode_negative_path() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[agent.hooks]\nenabled=true\ntimeout_ms=250\nfail_closed=false\non_error_default=\"panic\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("invalid hook mode should fail")
            .to_string()
            .contains("on_error_default"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_empty_allowlist_in_non_dev_mode() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[agent]\nmode=\"production\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("empty allowlist in non-dev mode should fail")
            .to_string()
            .contains("security.allowed_commands"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn allows_empty_allowlist_in_dev_mode() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[agent]\nmode=\"development\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("empty allowlist should be allowed in dev mode");
        assert!(cfg.security.allowed_commands.is_empty());
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_relative_allowed_root_traversal_escape() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root=\"../outside\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load_tool_security_policy(&dir, &config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("traversal allowed_root should fail")
            .to_string()
            .contains("must not contain parent directory traversal"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_unsupported_provider_url_scheme() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"openai\"\nbase_url=\"ftp://example.com\"\nmodel=\"gpt-4o-mini\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("unsupported provider scheme should fail")
            .to_string()
            .contains("scheme must be http or https"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_zero_request_timeout() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[agent]\nrequest_timeout_ms=0\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("zero request timeout should fail")
            .to_string()
            .contains("agent.request_timeout_ms"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_zero_memory_window_size() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[agent]\nmemory_window_size=0\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("zero memory window size should fail")
            .to_string()
            .contains("agent.memory_window_size"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_zero_max_prompt_chars() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[agent]\nmax_prompt_chars=0\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("zero max prompt chars should fail")
            .to_string()
            .contains("agent.max_prompt_chars"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_zero_shell_max_output_bytes() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n\n[security.shell]\nmax_output_bytes=0\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("zero shell max_output_bytes should fail")
            .to_string()
            .contains("security.shell.max_output_bytes"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_zero_write_file_max_write_bytes() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n\n[security.write_file]\nmax_write_bytes=0\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("zero write_file max_write_bytes should fail")
            .to_string()
            .contains("security.write_file.max_write_bytes"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn loads_config_backed_tool_policy() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[security.read_file]\nmax_read_bytes = 1024\nallow_binary = false\n\n[security.write_file]\nenabled = true\nmax_write_bytes = 2048\n\n[security.shell]\nmax_args = 2\nmax_arg_length = 12\nmax_output_bytes = 256\nforbidden_chars = \";&\"\n\n[security.mcp]\nenabled = true\nallowed_servers = [\"filesystem\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let policy = load_tool_security_policy(&dir, &config_path).expect("policy should load");
        assert_eq!(policy.shell.allowed_commands, vec!["echo".to_string()]);
        assert_eq!(policy.shell.max_args, 2);
        assert_eq!(policy.shell.max_output_bytes, 256);
        assert_eq!(policy.read_file.max_read_bytes, 1024);
        assert_eq!(policy.write_file.max_write_bytes, 2048);
        assert!(policy.enable_write_file);
        assert!(policy.enable_mcp);
        assert_eq!(policy.allowed_mcp_servers, vec!["filesystem".to_string()]);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn loads_enabled_audit_policy() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[security.audit]\nenabled = true\npath = \"./audit/events.log\"\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let audit = load_audit_policy(&dir, &config_path).expect("audit policy should load");
        assert!(audit.enabled);
        assert!(audit.path.ends_with("audit/events.log"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_enabled_audit_policy_with_empty_path() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[security.audit]\nenabled = true\npath = \"\"\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load_audit_policy(&dir, &config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("empty audit path should fail")
            .to_string()
            .contains("security.audit.path"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

// --- Phase A3-A6 deserialization tests ---

#[test]
fn parses_observability_config() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[observability]\nbackend = \"otel\"\notel_endpoint = \"http://collector:4318\"\notel_service_name = \"myservice\"\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("observability config should load");
        assert_eq!(cfg.observability.backend, "otel");
        assert_eq!(cfg.observability.otel_endpoint, "http://collector:4318");
        assert_eq!(cfg.observability.otel_service_name, "myservice");
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn parses_research_config() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[research]\nenabled = true\ntrigger = \"keywords\"\nmax_iterations = 10\nkeywords = [\"find\", \"search\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("research config should load");
        assert!(cfg.research.enabled);
        assert_eq!(cfg.research.trigger, "keywords");
        assert_eq!(cfg.research.max_iterations, 10);
        assert_eq!(cfg.research.keywords, vec!["find", "search"]);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn parses_runtime_config_with_wasm() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[runtime]\nkind = \"wasm\"\n\n[runtime.wasm]\nfuel_limit = 500000\nmemory_limit_mb = 32\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("runtime config should load");
        assert_eq!(cfg.runtime.kind, "wasm");
        assert_eq!(cfg.runtime.wasm.fuel_limit, 500_000);
        assert_eq!(cfg.runtime.wasm.memory_limit_mb, 32);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn parses_browser_config() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[browser]\nenabled = true\nbackend = \"native\"\nallowed_domains = [\"example.com\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("browser config should load");
        assert!(cfg.browser.enabled);
        assert_eq!(cfg.browser.backend, "native");
        assert_eq!(cfg.browser.allowed_domains, vec!["example.com"]);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn parses_web_search_config() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[web_search]\nenabled = true\nprovider = \"brave\"\nbrave_api_key = \"bk-xxx\"\nmax_results = 10\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("web_search config should load");
        assert!(cfg.web_search.enabled);
        assert_eq!(cfg.web_search.provider, "brave");
        assert_eq!(cfg.web_search.brave_api_key, Some("bk-xxx".to_string()));
        assert_eq!(cfg.web_search.max_results, 10);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn parses_cost_config() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[cost]\nenabled = true\ndaily_limit_usd = 5.0\nmonthly_limit_usd = 50.0\nwarn_at_percent = 90\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("cost config should load");
        assert!(cfg.cost.enabled);
        assert!((cfg.cost.daily_limit_usd - 5.0).abs() < f64::EPSILON);
        assert!((cfg.cost.monthly_limit_usd - 50.0).abs() < f64::EPSILON);
        assert_eq!(cfg.cost.warn_at_percent, 90);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn parses_identity_config() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[identity]\nformat = \"aieos\"\naieos_path = \"/etc/aieos.json\"\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("identity config should load");
        assert_eq!(cfg.identity.format, "aieos");
        assert_eq!(cfg.identity.aieos_path, Some("/etc/aieos.json".to_string()));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn parses_model_provider_profiles() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[model_providers.local]\nname = \"ollama\"\nbase_url = \"http://localhost:11434\"\nmodel = \"llama3\"\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("model_providers config should load");
        let profile = cfg
            .model_providers
            .get("local")
            .expect("local profile should exist");
        assert_eq!(profile.name, Some("ollama".to_string()));
        assert_eq!(profile.base_url, Some("http://localhost:11434".to_string()));
        assert_eq!(profile.model, Some("llama3".to_string()));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn parses_model_routes() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[[model_routes]]\nhint = \"fast\"\nprovider = \"openai\"\nmodel = \"gpt-4o-mini\"\nmax_tokens = 4096\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("model_routes config should load");
        assert_eq!(cfg.model_routes.len(), 1);
        assert_eq!(cfg.model_routes[0].hint, "fast");
        assert_eq!(cfg.model_routes[0].provider, "openai");
        assert_eq!(cfg.model_routes[0].model, "gpt-4o-mini");
        assert_eq!(cfg.model_routes[0].max_tokens, Some(4096));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn parses_embedding_routes() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[[embedding_routes]]\nhint = \"default\"\nprovider = \"openai\"\nmodel = \"text-embedding-3-small\"\ndimensions = 1536\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("embedding_routes config should load");
        assert_eq!(cfg.embedding_routes.len(), 1);
        assert_eq!(cfg.embedding_routes[0].hint, "default");
        assert_eq!(cfg.embedding_routes[0].model, "text-embedding-3-small");
        assert_eq!(cfg.embedding_routes[0].dimensions, Some(1536));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn parses_query_classification() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[query_classification]\nenabled = true\n\n[[query_classification.rules]]\nhint = \"code\"\nkeywords = [\"implement\", \"fix\"]\npriority = 10\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("query_classification config should load");
        assert!(cfg.query_classification.enabled);
        assert_eq!(cfg.query_classification.rules.len(), 1);
        assert_eq!(cfg.query_classification.rules[0].hint, "code");
        assert_eq!(
            cfg.query_classification.rules[0].keywords,
            vec!["implement", "fix"]
        );
        assert_eq!(cfg.query_classification.rules[0].priority, 10);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn parses_delegate_agent_config() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[agents.coder]\nprovider = \"openai\"\nmodel = \"gpt-4o\"\nmax_depth = 2\nagentic = true\nmax_iterations = 15\nallowed_tools = [\"shell\", \"read_file\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("delegate agent config should load");
        let coder = cfg.agents.get("coder").expect("coder agent should exist");
        assert_eq!(coder.provider, "openai");
        assert_eq!(coder.model, "gpt-4o");
        assert_eq!(coder.max_depth, 2);
        assert!(coder.agentic);
        assert_eq!(coder.max_iterations, 15);
        assert_eq!(coder.allowed_tools, vec!["shell", "read_file"]);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

// --- Negative-path tests ---

#[test]
fn rejects_invalid_provider_temperature() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind = \"openai\"\nbase_url = \"https://api.openai.com\"\nmodel = \"gpt-4o\"\ndefault_temperature = 3.0\n\n[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("invalid temperature should fail")
            .to_string()
            .contains("default_temperature"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_invalid_provider_api() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind = \"openai\"\nbase_url = \"https://api.openai.com\"\nmodel = \"gpt-4o\"\nprovider_api = \"graphql\"\n\n[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("invalid provider_api should fail")
            .to_string()
            .contains("provider_api"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

// --- Masking test ---

#[test]
fn masked_config_redacts_api_keys() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[web_search]\nenabled = true\nbrave_api_key = \"sk-secret-key\"\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("config should load");
        let masked = cfg.masked();
        assert_eq!(masked.web_search.brave_api_key, Some("****".to_string()));
        // Original should be untouched
        assert_eq!(
            cfg.web_search.brave_api_key,
            Some("sk-secret-key".to_string())
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

// --- Phase D: Channel config tests ---

#[test]
fn channels_config_group_reply_parses() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        r#"
[security]
allowed_root = "."
allowed_commands = ["echo"]

[channels_config]
message_timeout_secs = 600
stream_mode = "partial"
draft_update_interval_ms = 250
interrupt_on_new_message = true

[channels_config.group_reply.telegram]
mode = "mention_only"
allowed_sender_ids = ["admin-123"]
bot_name = "MyBot"

[channels_config.group_reply.discord]
mode = "all_messages"

[channels_config.ack_reaction.telegram]
enabled = true
emoji_pool = ["👍", "🔥"]
strategy = "first"
sample_rate = 0.8
"#,
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("config should load");
        assert_eq!(cfg.channels_config.message_timeout_secs, 600);
        assert_eq!(cfg.channels_config.stream_mode, "partial");
        assert_eq!(cfg.channels_config.draft_update_interval_ms, 250);
        assert!(cfg.channels_config.interrupt_on_new_message);

        let tg_reply = cfg.channels_config.group_reply.get("telegram").unwrap();
        assert_eq!(tg_reply.mode, "mention_only");
        assert_eq!(tg_reply.allowed_sender_ids, vec!["admin-123"]);
        assert_eq!(tg_reply.bot_name, Some("MyBot".to_string()));

        let dc_reply = cfg.channels_config.group_reply.get("discord").unwrap();
        assert_eq!(dc_reply.mode, "all_messages");

        let tg_ack = cfg.channels_config.ack_reaction.get("telegram").unwrap();
        assert!(tg_ack.enabled);
        assert_eq!(tg_ack.emoji_pool, vec!["👍", "🔥"]);
        assert_eq!(tg_ack.strategy, "first");
        assert!((tg_ack.sample_rate - 0.8).abs() < f64::EPSILON);
    });

    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn channels_config_defaults_are_reasonable() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("config should load");
        assert_eq!(cfg.channels_config.message_timeout_secs, 300);
        assert_eq!(cfg.channels_config.stream_mode, "off");
        assert_eq!(cfg.channels_config.draft_update_interval_ms, 500);
        assert!(!cfg.channels_config.interrupt_on_new_message);
        assert!(cfg.channels_config.group_reply.is_empty());
        assert!(cfg.channels_config.ack_reaction.is_empty());
    });

    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn channels_config_ack_reaction_rules_parse() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        r#"
[security]
allowed_root = "."
allowed_commands = ["echo"]

[channels_config.ack_reaction.slack]
enabled = true
emoji_pool = ["👀"]
strategy = "random"
sample_rate = 1.0

[[channels_config.ack_reaction.slack.rules]]
contains_any = ["urgent", "asap"]
emoji_override = ["🚨"]

[[channels_config.ack_reaction.slack.rules]]
sender_ids = ["boss-id"]
contains_none = ["test"]
"#,
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("config should load");
        let slack_ack = cfg.channels_config.ack_reaction.get("slack").unwrap();
        assert!(slack_ack.enabled);
        assert_eq!(slack_ack.rules.len(), 2);

        let rule0 = &slack_ack.rules[0];
        assert_eq!(rule0.contains_any, vec!["urgent", "asap"]);
        assert_eq!(rule0.emoji_override, vec!["🚨"]);

        let rule1 = &slack_ack.rules[1];
        assert_eq!(rule1.sender_ids, vec!["boss-id"]);
        assert_eq!(rule1.contains_none, vec!["test"]);
    });

    fs::remove_dir_all(dir).expect("cleanup");
}

// --- Phase B7: Shell config tests ---

#[test]
fn shell_context_aware_parsing_defaults_to_true() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("config should load");
        assert!(cfg.security.shell.context_aware_parsing);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn shell_context_aware_parsing_can_be_disabled() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[security.shell]\ncontext_aware_parsing = false\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("config should load");
        assert!(!cfg.security.shell.context_aware_parsing);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

// --- D2: Approval persistence tests ---

#[test]
fn update_auto_approve_creates_section_from_empty_file() {
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(&config_path, "").expect("empty file should be written");

    update_auto_approve(&config_path, &["shell".to_string(), "browser".to_string()])
        .expect("update should succeed");

    let content = fs::read_to_string(&config_path).expect("file should be readable");
    assert!(content.contains("[autonomy]"));
    assert!(content.contains("auto_approve"));
    assert!(content.contains("shell"));
    assert!(content.contains("browser"));

    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn update_auto_approve_preserves_other_sections() {
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind = \"openai\"\nmodel = \"gpt-4o\"\n",
    )
    .expect("config should be written");

    update_auto_approve(&config_path, &["shell".to_string()]).expect("update should succeed");

    let content = fs::read_to_string(&config_path).expect("file should be readable");
    assert!(content.contains("[provider]"));
    assert!(content.contains("kind = \"openai\""));
    assert!(content.contains("[autonomy]"));
    assert!(content.contains("shell"));

    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn update_auto_approve_overwrites_existing_list() {
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(&config_path, "[autonomy]\nauto_approve = [\"old_tool\"]\n")
        .expect("config should be written");

    update_auto_approve(&config_path, &["new_tool".to_string()]).expect("update should succeed");

    let content = fs::read_to_string(&config_path).expect("file should be readable");
    assert!(content.contains("new_tool"));
    assert!(!content.contains("old_tool"));

    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn update_auto_approve_empty_list_clears() {
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(&config_path, "[autonomy]\nauto_approve = [\"shell\"]\n")
        .expect("config should be written");

    update_auto_approve(&config_path, &[]).expect("update should succeed");

    let content = fs::read_to_string(&config_path).expect("file should be readable");
    assert!(content.contains("auto_approve = []"));

    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn resolve_local_provider_defaults_overrides_cloud_base_url_for_ollama() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"ollama\"\nmodel=\"llama3.1:8b\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("ollama config should load");
        assert_eq!(cfg.provider.kind, "ollama");
        assert_eq!(
            cfg.provider.base_url, "http://localhost:11434",
            "ollama should auto-resolve to localhost:11434"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn resolve_local_provider_defaults_overrides_cloud_base_url_for_lmstudio() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"lmstudio\"\nmodel=\"local-model\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("lmstudio config should load");
        assert_eq!(
            cfg.provider.base_url, "http://localhost:1234",
            "lmstudio should auto-resolve to localhost:1234"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn resolve_local_provider_defaults_preserves_explicit_base_url() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"ollama\"\nbase_url=\"http://gpu-server:11434\"\nmodel=\"llama3.1:8b\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("ollama with custom url should load");
        assert_eq!(
            cfg.provider.base_url, "http://gpu-server:11434",
            "explicit base_url should not be overridden"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn resolve_local_provider_defaults_does_not_affect_cloud_providers() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"openrouter\"\nbase_url=\"https://openrouter.ai/api/v1\"\nmodel=\"openai/gpt-4o-mini\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("openrouter config should load");
        assert_eq!(
            cfg.provider.base_url, "https://openrouter.ai/api",
            "cloud provider base_url should have trailing /v1 stripped by normalization"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn resolve_local_provider_defaults_all_local_providers_resolve() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();

    let providers = [
        ("ollama", "http://localhost:11434"),
        ("llamacpp", "http://localhost:8080"),
        ("lmstudio", "http://localhost:1234"),
        ("vllm", "http://localhost:8000"),
        ("sglang", "http://localhost:30000"),
    ];

    for (kind, expected_url) in &providers {
        let config_path = dir.join(format!("agentzero-{kind}.toml"));
        fs::write(
            &config_path,
            format!(
                "[provider]\nkind=\"{kind}\"\nmodel=\"test-model\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n"
            ),
        )
        .expect("config should be written");

        with_clean_agentzero_env(|| {
            let cfg =
                load(&config_path).unwrap_or_else(|e| panic!("{kind} config should load: {e}"));
            assert_eq!(
                cfg.provider.base_url, *expected_url,
                "provider '{kind}' should resolve to {expected_url}"
            );
        });
    }

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn normalize_base_url_strips_trailing_v1() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"openrouter\"\nbase_url=\"https://openrouter.ai/api/v1\"\nmodel=\"openai/gpt-4o-mini\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("config with /v1 suffix should load");
        assert_eq!(
            cfg.provider.base_url, "https://openrouter.ai/api",
            "trailing /v1 should be stripped from base_url"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn normalize_base_url_strips_trailing_v1_with_slash() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"openrouter\"\nbase_url=\"https://openrouter.ai/api/v1/\"\nmodel=\"openai/gpt-4o-mini\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("config with /v1/ suffix should load");
        assert_eq!(
            cfg.provider.base_url, "https://openrouter.ai/api",
            "trailing /v1/ should be stripped from base_url"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn normalize_base_url_preserves_url_without_v1() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"openrouter\"\nbase_url=\"https://openrouter.ai/api\"\nmodel=\"openai/gpt-4o-mini\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("config without /v1 should load");
        assert_eq!(
            cfg.provider.base_url, "https://openrouter.ai/api",
            "base_url without /v1 should remain unchanged"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

// --- Policy flag coverage ---

#[test]
fn enable_git_derived_from_allowed_commands() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");

    // Config with git in allowed_commands.
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\", \"git\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let policy = load_tool_security_policy(&dir, &config_path).expect("policy should load");
        assert!(
            policy.enable_git,
            "enable_git should be true when 'git' is in allowed_commands"
        );
    });

    // Config without git.
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\", \"ls\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let policy = load_tool_security_policy(&dir, &config_path).expect("policy should load");
        assert!(
            !policy.enable_git,
            "enable_git should be false without 'git' in allowed_commands"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn enable_web_search_from_config() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[web_search]\nenabled = true\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let policy = load_tool_security_policy(&dir, &config_path).expect("policy should load");
        assert!(policy.enable_web_search);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn enable_browser_from_config() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[browser]\nenabled = true\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let policy = load_tool_security_policy(&dir, &config_path).expect("policy should load");
        assert!(policy.enable_browser);
        assert!(policy.enable_browser_open);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn cidr_parse_error_returns_err() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[security.url_access]\nallow_cidrs = [\"not-a-cidr\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load_tool_security_policy(&dir, &config_path);
        assert!(result.is_err(), "invalid CIDR should fail");
        assert!(
            result.unwrap_err().to_string().contains("CIDR"),
            "error should mention CIDR"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn absolute_allowed_root_is_accepted() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    // Use the temp dir as the absolute allowed_root (it exists and is canonical).
    let abs_root = dir.to_string_lossy().to_string();
    fs::write(
        &config_path,
        format!("[security]\nallowed_root = \"{abs_root}\"\nallowed_commands = [\"echo\"]\n"),
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let policy = load_tool_security_policy(&dir, &config_path).expect("policy should load");
        assert!(
            policy.read_file.allowed_root.is_absolute(),
            "allowed_root should be absolute"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_gateway_port_zero() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[gateway]\nport=0\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("port 0 should fail")
            .to_string()
            .contains("gateway.port"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_empty_gateway_host() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[gateway]\nhost=\"\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("empty host should fail")
            .to_string()
            .contains("gateway.host"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_public_host_without_allow_public_bind() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[gateway]\nhost=\"0.0.0.0\"\nallow_public_bind=false\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("public host without allow_public_bind should fail")
            .to_string()
            .contains("allow_public_bind"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_invalid_autonomy_level() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[autonomy]\nlevel=\"yolo\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("invalid autonomy level should fail")
            .to_string()
            .contains("autonomy.level"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn rejects_zero_max_cost_per_day() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[autonomy]\nmax_cost_per_day_cents=0\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        assert!(result
            .expect_err("zero cost should fail")
            .to_string()
            .contains("max_cost_per_day_cents"));
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

// --- Privacy config tests ---

#[test]
fn privacy_defaults_to_off_when_section_absent() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("config should load without privacy section");
        assert_eq!(cfg.privacy.mode, "off");
        assert!(!cfg.privacy.enforce_local_provider);
        assert!(!cfg.privacy.block_cloud_providers);
        assert!(!cfg.privacy.noise.enabled);
        assert_eq!(cfg.privacy.noise.handshake_pattern, "XX");
        assert_eq!(cfg.privacy.noise.session_timeout_secs, 3600);
        assert_eq!(cfg.privacy.noise.max_sessions, 256);
        assert!(!cfg.privacy.sealed_envelopes.enabled);
        assert_eq!(cfg.privacy.sealed_envelopes.default_ttl_secs, 86400);
        assert!(!cfg.privacy.key_rotation.enabled);
        assert_eq!(cfg.privacy.key_rotation.rotation_interval_secs, 604_800);
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn privacy_parses_full_toml_section() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        r#"
[security]
allowed_root = "."
allowed_commands = ["echo"]

[privacy]
mode = "encrypted"
enforce_local_provider = false
block_cloud_providers = false

[privacy.noise]
enabled = true
handshake_pattern = "XX"
session_timeout_secs = 1800
max_sessions = 128

[privacy.sealed_envelopes]
enabled = true
default_ttl_secs = 43200
max_envelope_bytes = 2097152

[privacy.key_rotation]
enabled = true
rotation_interval_secs = 86400
overlap_secs = 3600
key_store_path = "/tmp/keys"
"#,
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("config with privacy should load");
        assert_eq!(cfg.privacy.mode, "encrypted");
        assert!(cfg.privacy.noise.enabled);
        assert_eq!(cfg.privacy.noise.session_timeout_secs, 1800);
        assert_eq!(cfg.privacy.noise.max_sessions, 128);
        assert!(cfg.privacy.sealed_envelopes.enabled);
        assert_eq!(cfg.privacy.sealed_envelopes.default_ttl_secs, 43200);
        assert_eq!(cfg.privacy.sealed_envelopes.max_envelope_bytes, 2_097_152);
        assert!(cfg.privacy.key_rotation.enabled);
        assert_eq!(cfg.privacy.key_rotation.rotation_interval_secs, 86400);
        assert_eq!(cfg.privacy.key_rotation.overlap_secs, 3600);
        assert_eq!(cfg.privacy.key_rotation.key_store_path, "/tmp/keys");
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn privacy_rejects_invalid_mode() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[privacy]\nmode=\"invalid\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        let err = result.expect_err("invalid mode should fail").to_string();
        assert!(
            err.contains("privacy.mode"),
            "error should mention privacy.mode: {err}"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn privacy_rejects_cloud_provider_in_local_only_mode() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"openrouter\"\nbase_url=\"https://openrouter.ai/api\"\nmodel=\"gpt-4o-mini\"\n\n[privacy]\nmode=\"local_only\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        let err = result
            .expect_err("cloud provider in local_only should fail")
            .to_string();
        assert!(
            err.contains("local provider"),
            "error should mention local provider: {err}"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn privacy_allows_local_provider_in_local_only_mode() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"ollama\"\nbase_url=\"http://localhost:11434\"\nmodel=\"llama3\"\n\n[privacy]\nmode=\"local_only\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("ollama in local_only should succeed");
        assert_eq!(cfg.privacy.mode, "local_only");
        assert_eq!(cfg.provider.kind, "ollama");
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn privacy_enforce_local_provider_blocks_cloud() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"anthropic\"\nbase_url=\"https://api.anthropic.com\"\nmodel=\"claude-sonnet-4-6\"\n\n[privacy]\nmode=\"off\"\nenforce_local_provider=true\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        let err = result
            .expect_err("enforce_local_provider should reject cloud")
            .to_string();
        assert!(
            err.contains("local provider"),
            "error should mention local provider: {err}"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn privacy_rejects_invalid_handshake_pattern() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[privacy.noise]\nhandshake_pattern=\"NK\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        let err = result.expect_err("invalid pattern should fail").to_string();
        assert!(
            err.contains("handshake_pattern"),
            "error should mention handshake_pattern: {err}"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn privacy_all_four_modes_accepted() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    for mode in &["off", "local_only", "encrypted", "full"] {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        let provider = if *mode == "local_only" || *mode == "full" {
            "kind=\"ollama\"\nbase_url=\"http://localhost:11434\"\nmodel=\"llama3\""
        } else {
            "kind=\"openrouter\"\nbase_url=\"https://openrouter.ai/api\"\nmodel=\"gpt-4o-mini\""
        };
        // Encrypted mode requires noise.enabled = true.
        let noise_section = if *mode == "encrypted" {
            "\n[privacy.noise]\nenabled=true\n"
        } else {
            ""
        };
        fs::write(
            &config_path,
            format!(
                "[provider]\n{provider}\n\n[privacy]\nmode=\"{mode}\"\n{noise_section}\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n"
            ),
        )
        .expect("config should be written");

        with_clean_agentzero_env(|| {
            let cfg = load(&config_path)
                .unwrap_or_else(|e| panic!("mode '{mode}' should be accepted: {e}"));
            assert_eq!(cfg.privacy.mode, *mode);
        });

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}

#[test]
fn privacy_local_only_rejects_non_localhost_base_url() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"ollama\"\nbase_url=\"http://remote-server.example.com:11434\"\nmodel=\"llama3\"\n\n[privacy]\nmode=\"local_only\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let result = load(&config_path);
        assert!(result.is_err());
        let err = result
            .expect_err("non-localhost base_url in local_only should fail")
            .to_string();
        assert!(
            err.contains("localhost"),
            "error should mention localhost: {err}"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn privacy_local_only_network_tools_disabled() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[provider]\nkind=\"ollama\"\nbase_url=\"http://localhost:11434\"\nmodel=\"llama3\"\n\n[privacy]\nmode=\"local_only\"\n\n[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n\n[web_search]\nenabled=true\n\n[http_request]\nenabled=true\n\n[web_fetch]\nenabled=true\n",
    )
    .expect("config should be written");

    with_clean_agentzero_env(|| {
        let policy =
            crate::load_tool_security_policy(&dir, &config_path).expect("policy should load");
        // local_only must override user's config and disable network tools.
        assert!(
            !policy.enable_http_request,
            "http_request should be disabled"
        );
        assert!(!policy.enable_web_fetch, "web_fetch should be disabled");
        assert!(!policy.enable_web_search, "web_search should be disabled");
        assert!(
            policy.url_access.enforce_domain_allowlist,
            "domain allowlist should be enforced"
        );
        assert!(
            policy
                .url_access
                .domain_allowlist
                .contains(&"localhost".to_string()),
            "localhost should be in domain allowlist"
        );
    });

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

// --- Config validation: routing and classification (Sprint 23 Phase 5) ---

#[test]
fn query_classification_deserializes_with_rules() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[query_classification]\nenabled = true\n\n[[query_classification.rules]]\nhint = \"code\"\nkeywords = [\"implement\", \"fix\"]\npatterns = [\"(?i)refactor\"]\nmin_length = 5\nmax_length = 5000\npriority = 10\n",
    )
    .expect("write");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("should load");
        assert!(cfg.query_classification.enabled);
        assert_eq!(cfg.query_classification.rules.len(), 1);
        let rule = &cfg.query_classification.rules[0];
        assert_eq!(rule.hint, "code");
        assert_eq!(rule.keywords, vec!["implement", "fix"]);
        assert_eq!(rule.patterns, vec!["(?i)refactor"]);
        assert_eq!(rule.min_length, Some(5));
        assert_eq!(rule.max_length, Some(5000));
        assert_eq!(rule.priority, 10);
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn query_classification_default_has_enabled_false() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n",
    )
    .expect("write");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("should load");
        assert!(!cfg.query_classification.enabled);
        assert!(cfg.query_classification.rules.is_empty());
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn embedding_route_deserializes_with_optional_fields() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[[embedding_routes]]\nhint = \"code\"\nprovider = \"openai\"\nmodel = \"text-embedding-3-large\"\n\n[[embedding_routes]]\nhint = \"search\"\nprovider = \"openai\"\nmodel = \"text-embedding-3-small\"\ndimensions = 512\n",
    )
    .expect("write");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("should load");
        assert_eq!(cfg.embedding_routes.len(), 2);
        // First route: no dimensions
        assert_eq!(cfg.embedding_routes[0].hint, "code");
        assert!(cfg.embedding_routes[0].dimensions.is_none());
        // Second route: explicit dimensions
        assert_eq!(cfg.embedding_routes[1].hint, "search");
        assert_eq!(cfg.embedding_routes[1].dimensions, Some(512));
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn validation_warns_on_empty_classification_rules() {
    // This test validates that the config is accepted (no error) even when
    // classification is enabled with no rules. The warning is emitted at
    // runtime via tracing, which is a no-op in tests.
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[query_classification]\nenabled = true\n",
    )
    .expect("write");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("should load even with empty rules");
        assert!(cfg.query_classification.enabled);
        assert!(cfg.query_classification.rules.is_empty());
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_rejects_invalid_agent_privacy_boundary() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        r#"
[provider]
kind = "ollama"
base_url = "http://localhost:11434"
model = "llama3"

[agents.researcher]
provider = "openai"
model = "gpt-4o"
privacy_boundary = "bogus_value"
"#,
    )
    .expect("write");

    with_clean_agentzero_env(|| {
        let err = load(&config_path).unwrap_err();
        assert!(
            err.to_string().contains("privacy_boundary"),
            "error should mention privacy_boundary: {err}"
        );
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_rejects_agent_boundary_more_permissive_than_global() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        r#"
[provider]
kind = "ollama"
base_url = "http://localhost:11434"
model = "llama3"

[privacy]
mode = "local_only"

[agents.researcher]
provider = "ollama"
model = "llama3"
privacy_boundary = "any"
"#,
    )
    .expect("write");

    with_clean_agentzero_env(|| {
        let err = load(&config_path).unwrap_err();
        assert!(
            err.to_string().contains("more permissive"),
            "error should mention more permissive: {err}"
        );
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_rejects_invalid_tool_boundary() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        r#"
[provider]
kind = "ollama"
base_url = "http://localhost:11434"
model = "llama3"

[security.tool_boundaries]
shell = "invalid_boundary"
"#,
    )
    .expect("write");

    with_clean_agentzero_env(|| {
        let err = load(&config_path).unwrap_err();
        assert!(
            err.to_string().contains("tool_boundaries"),
            "error should mention tool_boundaries: {err}"
        );
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_accepts_valid_privacy_boundaries() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        r#"
[provider]
kind = "ollama"
base_url = "http://localhost:11434"
model = "llama3"

[privacy]
mode = "encrypted"

[privacy.noise]
enabled = true

[agents.research]
provider = "anthropic"
model = "claude-sonnet-4-6"
privacy_boundary = "encrypted_only"

[agents.local]
provider = "ollama"
model = "llama3"
privacy_boundary = "local_only"

[security.tool_boundaries]
shell = "local_only"
web_search = "any"
"#,
    )
    .expect("write");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("valid boundaries should load successfully");
        // Double-check agents loaded correctly.
        assert_eq!(
            cfg.agents.get("research").unwrap().privacy_boundary,
            "encrypted_only"
        );
        assert_eq!(
            cfg.agents.get("local").unwrap().privacy_boundary,
            "local_only"
        );
        assert_eq!(
            cfg.security.tool_boundaries.get("shell").unwrap(),
            "local_only"
        );
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn channels_global_config_default_privacy_boundary_empty() {
    let cfg = crate::ChannelsGlobalConfig::default();
    assert_eq!(cfg.default_privacy_boundary, "");
}

#[test]
fn channels_global_config_toml_parses_default_privacy_boundary() {
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        r#"
[channels_config]
default_privacy_boundary = "encrypted_only"
"#,
    )
    .unwrap();

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("config should load");
        assert_eq!(
            cfg.channels_config.default_privacy_boundary,
            "encrypted_only"
        );
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_rejects_encrypted_mode_without_noise() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        r#"
[provider]
kind = "ollama"
base_url = "http://localhost:11434"
model = "llama3"

[privacy]
mode = "encrypted"

[privacy.noise]
enabled = false
"#,
    )
    .expect("write");

    with_clean_agentzero_env(|| {
        let err = load(&config_path).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("encrypted") && msg.contains("noise"),
            "error should mention encrypted mode requiring noise: {msg}"
        );
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn validate_accepts_encrypted_mode_with_noise_enabled() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(
        &config_path,
        r#"
[provider]
kind = "ollama"
base_url = "http://localhost:11434"
model = "llama3"

[privacy]
mode = "encrypted"

[privacy.noise]
enabled = true
"#,
    )
    .expect("write");

    with_clean_agentzero_env(|| {
        let cfg = load(&config_path).expect("config should load with encrypted+noise");
        assert_eq!(cfg.privacy.mode, "encrypted");
        assert!(cfg.privacy.noise.enabled);
    });

    fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------------------
// MCP mcp.json config loading tests
// ---------------------------------------------------------------------------

fn base_mcp_config_toml() -> &'static str {
    "[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n\n[security.mcp]\nenabled=true\n"
}

#[test]
fn mcp_loads_global_mcp_json() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(&config_path, base_mcp_config_toml()).expect("config should be written");

    // Global mcp.json in the same dir as agentzero.toml.
    fs::write(
        dir.join("mcp.json"),
        r#"{"mcpServers":{"fs":{"command":"echo","args":["hello"]}}}"#,
    )
    .expect("mcp.json should be written");

    with_clean_agentzero_env(|| {
        let policy = load_tool_security_policy(&dir, &config_path).expect("policy should load");
        assert!(policy.enable_mcp);
        assert_eq!(policy.mcp_servers.len(), 1);
        assert!(policy.mcp_servers.contains_key("fs"));
        assert_eq!(policy.mcp_servers["fs"].command, "echo");
        assert_eq!(policy.mcp_servers["fs"].args, vec!["hello"]);
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn mcp_loads_project_mcp_json() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(&config_path, base_mcp_config_toml()).expect("config should be written");

    // Project-level mcp.json in .agentzero/ subdirectory of workspace.
    let project_dir = dir.join(".agentzero");
    fs::create_dir_all(&project_dir).expect("project dir should be created");
    fs::write(
        project_dir.join("mcp.json"),
        r#"{"mcpServers":{"git":{"command":"git-mcp","args":[]}}}"#,
    )
    .expect("project mcp.json should be written");

    with_clean_agentzero_env(|| {
        let policy = load_tool_security_policy(&dir, &config_path).expect("policy should load");
        assert_eq!(policy.mcp_servers.len(), 1);
        assert!(policy.mcp_servers.contains_key("git"));
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn mcp_project_overrides_global_by_name() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(&config_path, base_mcp_config_toml()).expect("config should be written");

    // Global defines "fs" with command "global-cmd".
    fs::write(
        dir.join("mcp.json"),
        r#"{"mcpServers":{"fs":{"command":"global-cmd","args":[]},"shared":{"command":"global-shared","args":[]}}}"#,
    )
    .expect("global mcp.json should be written");

    // Project overrides "fs" with command "project-cmd".
    let project_dir = dir.join(".agentzero");
    fs::create_dir_all(&project_dir).expect("project dir should be created");
    fs::write(
        project_dir.join("mcp.json"),
        r#"{"mcpServers":{"fs":{"command":"project-cmd","args":["--project"]}}}"#,
    )
    .expect("project mcp.json should be written");

    with_clean_agentzero_env(|| {
        let policy = load_tool_security_policy(&dir, &config_path).expect("policy should load");
        assert_eq!(policy.mcp_servers.len(), 2);
        // "fs" should be overridden by project.
        assert_eq!(policy.mcp_servers["fs"].command, "project-cmd");
        assert_eq!(policy.mcp_servers["fs"].args, vec!["--project"]);
        // "shared" should remain from global.
        assert_eq!(policy.mcp_servers["shared"].command, "global-shared");
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn mcp_allowed_servers_filters_results() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    // Config with allowed_servers = ["fs"] — only "fs" should pass.
    fs::write(
        &config_path,
        "[security]\nallowed_root=\".\"\nallowed_commands=[\"echo\"]\n\n[security.mcp]\nenabled=true\nallowed_servers=[\"fs\"]\n",
    )
    .expect("config should be written");

    fs::write(
        dir.join("mcp.json"),
        r#"{"mcpServers":{"fs":{"command":"echo","args":[]},"git":{"command":"git-mcp","args":[]}}}"#,
    )
    .expect("mcp.json should be written");

    with_clean_agentzero_env(|| {
        let policy = load_tool_security_policy(&dir, &config_path).expect("policy should load");
        assert_eq!(policy.mcp_servers.len(), 1);
        assert!(policy.mcp_servers.contains_key("fs"));
        assert!(!policy.mcp_servers.contains_key("git"));
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn mcp_no_files_returns_empty_servers() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(&config_path, base_mcp_config_toml()).expect("config should be written");

    // No mcp.json files, no env var.
    with_clean_agentzero_env(|| {
        let policy = load_tool_security_policy(&dir, &config_path).expect("policy should load");
        assert!(policy.mcp_servers.is_empty());
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn mcp_env_field_in_server_entry() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let config_path = dir.join("agentzero.toml");
    fs::write(&config_path, base_mcp_config_toml()).expect("config should be written");

    fs::write(
        dir.join("mcp.json"),
        r#"{"mcpServers":{"gh":{"command":"gh-mcp","args":[],"env":{"GITHUB_TOKEN":"test123"}}}}"#,
    )
    .expect("mcp.json should be written");

    with_clean_agentzero_env(|| {
        let policy = load_tool_security_policy(&dir, &config_path).expect("policy should load");
        assert_eq!(
            policy.mcp_servers["gh"].env.get("GITHUB_TOKEN").unwrap(),
            "test123"
        );
    });

    fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------------------
// Example config smoke tests
// ---------------------------------------------------------------------------
// Each test copies an example config to an isolated temp directory and
// verifies it loads and validates without error.

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .canonicalize()
        .expect("examples directory should exist")
}

fn smoke_test_example(config_path_in_examples: &str) {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let source = examples_dir().join(config_path_in_examples);
    let content = fs::read_to_string(&source)
        .unwrap_or_else(|e| panic!("should read {}: {e}", source.display()));
    let dest = dir.join("agentzero.toml");
    fs::write(&dest, &content).expect("should write temp config");

    with_clean_agentzero_env(|| {
        let cfg =
            load(&dest).unwrap_or_else(|e| panic!("{config_path_in_examples} should load: {e}"));

        // Basic structural checks that apply to every example
        assert!(!cfg.provider.kind.is_empty(), "provider.kind must be set");
        assert!(!cfg.provider.model.is_empty(), "provider.model must be set");
        assert!(
            !cfg.security.allowed_root.is_empty(),
            "security.allowed_root must be set"
        );
        assert!(
            !cfg.security.allowed_commands.is_empty(),
            "security.allowed_commands must be set"
        );
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn example_config_basic_loads_and_validates() {
    smoke_test_example("config-basic.toml");
}

#[test]
fn example_config_full_loads_and_validates() {
    smoke_test_example("config-full.toml");
}

#[test]
fn example_business_office_loads_and_validates() {
    smoke_test_example("business-office/agentzero.toml");
}

#[test]
fn example_research_pipeline_loads_and_validates() {
    smoke_test_example("research-pipeline/agentzero.toml");
}

#[test]
fn example_config_basic_has_expected_provider() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let source = examples_dir().join("config-basic.toml");
    let dest = dir.join("agentzero.toml");
    fs::copy(&source, &dest).expect("should copy config");

    with_clean_agentzero_env(|| {
        let cfg = load(&dest).expect("basic config should load");
        assert_eq!(cfg.provider.kind, "openrouter");
        assert_eq!(cfg.provider.model, "anthropic/claude-sonnet-4-6");
        assert_eq!(cfg.gateway.port, 42617);
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn example_business_office_has_swarm_agents() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let source = examples_dir().join("business-office/agentzero.toml");
    let dest = dir.join("agentzero.toml");
    fs::copy(&source, &dest).expect("should copy config");

    with_clean_agentzero_env(|| {
        let cfg = load(&dest).expect("business-office config should load");
        let swarm = &cfg.swarm;
        assert!(swarm.enabled, "swarm should be enabled");
        assert!(
            !swarm.agents.is_empty(),
            "swarm should have at least one agent"
        );
        assert!(
            swarm.agents.len() >= 7,
            "expected at least 7 swarm agents, got {}",
            swarm.agents.len()
        );
        assert!(
            !cfg.swarm.pipelines.is_empty(),
            "should have at least one pipeline"
        );
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn example_research_pipeline_has_pipeline_steps() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let source = examples_dir().join("research-pipeline/agentzero.toml");
    let dest = dir.join("agentzero.toml");
    fs::copy(&source, &dest).expect("should copy config");

    with_clean_agentzero_env(|| {
        let cfg = load(&dest).expect("research-pipeline config should load");
        let swarm = &cfg.swarm;
        assert!(swarm.enabled, "swarm should be enabled");
        assert!(
            swarm.agents.len() >= 4,
            "expected at least 4 swarm agents, got {}",
            swarm.agents.len()
        );
        assert!(
            !cfg.swarm.pipelines.is_empty(),
            "should have at least one pipeline"
        );

        let research_pipeline = cfg
            .swarm
            .pipelines
            .iter()
            .find(|p| p.name == "research-to-brief")
            .expect("should have a 'research-to-brief' pipeline");
        assert_eq!(
            research_pipeline.steps.len(),
            4,
            "research pipeline should have 4 steps"
        );
    });

    fs::remove_dir_all(dir).ok();
}

#[test]
fn example_config_full_exercises_all_sections() {
    let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
    let dir = temp_dir();
    let source = examples_dir().join("config-full.toml");
    let dest = dir.join("agentzero.toml");
    fs::copy(&source, &dest).expect("should copy config");

    with_clean_agentzero_env(|| {
        let cfg = load(&dest).expect("full config should load");

        assert_eq!(cfg.provider.kind, "openrouter");
        assert!(!cfg.provider.model.is_empty());
        assert_eq!(cfg.memory.backend, "sqlite");
        assert!(!cfg.security.allowed_root.is_empty());
        assert!(!cfg.security.allowed_commands.is_empty());
        assert!(cfg.gateway.port > 0);
        assert!(cfg.agent.max_tool_iterations > 0);
        assert!(cfg.agent.request_timeout_ms > 0);
    });

    fs::remove_dir_all(dir).ok();
}
