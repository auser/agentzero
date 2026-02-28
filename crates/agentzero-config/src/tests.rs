use crate::{load, load_audit_policy, load_env_var, load_tool_security_policy};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("agentzero-config-{nanos}"));
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
        assert_eq!(cfg.agent.memory_window_size, 8);
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
