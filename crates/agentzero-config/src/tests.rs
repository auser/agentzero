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
fn rejects_enabled_mcp_without_allowed_servers() {
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
        assert!(result.is_err());
        assert!(result
            .expect_err("invalid config should fail")
            .to_string()
            .contains("allowed_servers"));
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
        "[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n\n[security.read_file]\nmax_read_bytes = 1024\nallow_binary = false\n\n[security.write_file]\nenabled = true\nmax_write_bytes = 2048\n\n[security.shell]\nmax_args = 2\nmax_arg_length = 12\nmax_output_bytes = 256\nforbidden_chars = \";&\"\n\n[security.mcp]\nenabled = true\nallowed_servers = [\"filesystem\"]\n\n[security.plugin]\nenabled = true\n",
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
        assert!(policy.enable_process_plugin);
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
