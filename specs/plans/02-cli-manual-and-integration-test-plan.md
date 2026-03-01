# CLI Commands: Manual Test Checklist & Integration Test Plan

## Context

Comprehensive test inventory for all ~35 top-level CLI commands and ~97 subcommands. The existing `cli_integration.rs` had ~55 tests but left significant gaps around template, skill lifecycle, auth token flows, cron variants, estop levels, channel add/remove, memory get, and plugin validate/remove. This plan added 58 new integration tests (113 total) and documented a manual test checklist for commands requiring live services/TTY/hardware.

**Status**: IMPLEMENTED (2026-03-01). All 113 integration tests passing.

---

## Section 1: Manual Test Checklist

Commands that **cannot** be integration-tested because they require live services, TTY, network probing, or OS-level setup.

### 1.1 Gateway & Daemon (requires HTTP listener)

```bash
# Start gateway, verify HTTP listener
cargo run -- gateway --port 9090
# Expected: "Listening on 127.0.0.1:9090", Ctrl+C clean shutdown

# Start gateway with new pairing
cargo run -- gateway --port 9091 --new-pairing
# Expected: Pairing code printed, old tokens cleared

# Start daemon, verify state file
cargo run -- daemon --port 9092
# Expected: State file written to data-dir, clean shutdown writes stop marker
```

### 1.2 Agent (requires live LLM provider)

```bash
# With valid API key and config
cargo run -- agent -m "say hello"
# Expected: LLM response printed

# Without API key
unset OPENAI_API_KEY && cargo run -- agent -m "hello"
# Expected: Error about missing API key or auth
```

### 1.3 Dashboard (requires TTY)

```bash
# In a real terminal (not piped)
cargo run -- dashboard
# Expected: TUI renders, q/Ctrl+C exits cleanly

# In non-TTY (pipe)
echo "" | cargo run -- dashboard
# Expected: Error "requires a terminal"
```

### 1.4 Local Model Discovery (requires running model server)

```bash
# With Ollama running
cargo run -- local discover
# Expected: Table with ollama=running, model list shown

cargo run -- local discover --json
# Expected: Valid JSON array with provider/url/status/models

cargo run -- local discover --timeout-ms 200
# Expected: Faster timeout, may show fewer results

# Health check with running Ollama
cargo run -- local health ollama
# Expected: Reachable: yes, Latency: Xms

cargo run -- local health ollama --url http://localhost:11434
# Expected: Same as above with explicit URL

# Status with ollama configured
cargo run -- --config /tmp/az-ollama.toml local status
# Expected: Provider: ollama, URL, Status: running/offline
```

### 1.5 Models Pull (requires running Ollama)

```bash
cargo run -- models pull llama3.1:8b
# Expected: Streaming progress, "Done" message

cargo run -- models pull llama3.1:8b --provider ollama
# Expected: Same as above with explicit provider
```

### 1.6 Models Refresh (requires network for live provider catalogs)

```bash
cargo run -- models refresh
# Expected: Fetches model list from configured provider

cargo run -- models refresh --all
# Expected: Refreshes all providers

cargo run -- models refresh --force
# Expected: Ignores cache, forces live refresh
```

### 1.7 Doctor Models (requires live providers)

```bash
cargo run -- doctor models
# Expected: Probes all known providers, reports availability

cargo run -- doctor models --provider openrouter
# Expected: Probes only openrouter

cargo run -- doctor models --use-cache
# Expected: Uses cached catalog instead of live probe
```

### 1.8 Service Install/Restart (requires OS init system)

```bash
# On systemd system
cargo run -- service install --service-init systemd
# Expected: Unit file created

cargo run -- service start --service-init systemd
# Expected: Service started via systemctl

cargo run -- service restart --service-init systemd
# Expected: Service restarted

cargo run -- service stop --service-init systemd
# Expected: Service stopped

cargo run -- service uninstall --service-init systemd
# Expected: Unit file removed
```

### 1.9 Channel Start (requires channel credentials)

```bash
# With Telegram bot token configured
cargo run -- channel start
# Expected: Bot connects, responds to messages

# With Discord token configured
AGENTZERO_DISCORD_TOKEN=xxx cargo run -- channel start
# Expected: Bot connects to Discord
```

### 1.10 Tunnel Start/Stop (requires remote target)

```bash
cargo run -- tunnel start --protocol http --remote example.com:80 --local-port 8080
# Expected: Tunnel established, traffic forwarded

cargo run -- tunnel status
# Expected: Shows active tunnel info

cargo run -- tunnel stop
# Expected: Tunnel closed cleanly
```

### 1.11 Auth Login (requires OAuth redirect)

```bash
cargo run -- auth login --provider openai-codex
# Expected: Opens browser for OAuth, prints auth URL

cargo run -- auth login --provider gemini
# Expected: Opens browser for Gemini OAuth

cargo run -- auth paste-redirect --provider openai-codex --input "https://redirect-url?code=xxx"
# Expected: Exchanges code for token, saves profile

cargo run -- auth refresh --provider openai-codex
# Expected: Refreshes access token using refresh token
```

### 1.12 Onboard Interactive (requires TTY)

```bash
cargo run -- onboard --interactive
# Expected: Full wizard with prompts for provider, model, memory, security

cargo run -- onboard --channels-only
# Expected: Only channel configuration prompts
```

### 1.13 Hardware & Peripheral Flash (requires connected hardware)

```bash
# With hardware feature enabled and board connected
cargo run -- hardware discover
# Expected: Lists connected boards

cargo run -- hardware introspect
# Expected: Board introspection data

cargo run -- peripheral flash --id sensor-1 --firmware path/to/fw.bin
# Expected: Firmware flashed

cargo run -- peripheral flash-nucleo
# Expected: Nucleo board flashed

cargo run -- peripheral setup-uno-q --host 192.168.0.48
# Expected: Uno Q setup flow runs
```

### 1.14 Update Apply/Rollback (modifies binary)

```bash
cargo run -- update apply --version 0.2.0
# Expected: Downloads and replaces binary

cargo run -- update rollback
# Expected: Restores previous version
```

---

## Section 2: New Integration Tests (Implemented)

58 tests added to `crates/agentzero-cli/tests/cli_integration.rs`. Each follows the existing `run_cmd()` + `temp_dir()` + `cleanup()` pattern.

### 2.1 Template Commands (T1 — 8 tests)

| # | Test Function | Assert |
|---|---|---|
| 1 | `template_list_success_path` | Ok |
| 2 | `template_list_json_success_path` | Ok |
| 3 | `template_show_known_success_path` | Ok (requires `template init` first) |
| 4 | `template_show_unknown_negative_path` | Err |
| 5 | `template_init_all_success_path` | Ok |
| 6 | `template_init_single_success_path` | Ok |
| 7 | `template_init_no_overwrite_skips_existing_success_path` | Ok (skips silently, does not error) |
| 8 | `template_validate_success_path` | Ok |

### 2.2 Skill Lifecycle (T1 — 6 tests)

| # | Test Function | Assert |
|---|---|---|
| 9 | `skill_new_typescript_success_path` | Ok |
| 10 | `skill_new_rust_success_path` | Ok |
| 11 | `skill_templates_success_path` | Ok |
| 12 | `skill_test_missing_negative_path` | Err |
| 13 | `skill_audit_missing_negative_path` | Err |
| 14 | `skill_remove_missing_negative_path` | Err |

### 2.3 Hooks (T1 — 2 tests)

| # | Test Function | Assert |
|---|---|---|
| 15 | `hooks_disable_missing_negative_path` | Err |
| 16 | `hooks_test_missing_negative_path` | Err |

### 2.4 Plugin (T1 — 4 tests)

| # | Test Function | Assert |
|---|---|---|
| 17 | `plugin_validate_after_new_success_path` | Ok (manifest is `manifest.json`) |
| 18 | `plugin_validate_missing_negative_path` | Err |
| 19 | `plugin_remove_missing_success_path` | Ok (idempotent) |
| 20 | `plugin_list_json_success_path` | Ok |

### 2.5 Cron Variants (T1 — 5 tests)

| # | Test Function | Assert |
|---|---|---|
| 21 | `cron_add_at_success_path` | Ok |
| 22 | `cron_add_every_success_path` | Ok |
| 23 | `cron_once_success_path` | Ok |
| 24 | `cron_update_missing_negative_path` | Err |
| 25 | `cron_pause_missing_negative_path` | Err |

### 2.6 Estop Levels (T1 — 4 tests)

| # | Test Function | Assert |
|---|---|---|
| 26 | `estop_network_kill_and_resume_success_path` | Ok |
| 27 | `estop_domain_block_and_resume_success_path` | Ok |
| 28 | `estop_tool_freeze_and_resume_success_path` | Ok |
| 29 | `estop_engage_with_require_otp_success_path` | Ok |

### 2.7 Auth Token Flows (T1 — 5 tests)

| # | Test Function | Assert |
|---|---|---|
| 30 | `auth_paste_token_and_list_success_path` | Ok |
| 31 | `auth_paste_token_and_logout_success_path` | Ok |
| 32 | `auth_use_missing_profile_negative_path` | Err |
| 33 | `auth_logout_missing_negative_path` | Ok or Err (idempotent) |
| 34 | `auth_setup_token_success_path` | Ok |

### 2.8 Channel Add/Remove (T1 — 3 tests)

| # | Test Function | Assert |
|---|---|---|
| 35 | `channel_add_and_list_success_path` | Ok |
| 36 | `channel_add_and_remove_success_path` | Ok |
| 37 | `channel_remove_missing_negative_path` | Err |

### 2.9 Doctor Traces Filters (T1 — 2 tests)

| # | Test Function | Assert |
|---|---|---|
| 38 | `doctor_traces_with_event_filter_success_path` | Ok |
| 39 | `doctor_traces_with_contains_filter_success_path` | Ok |

### 2.10 Completions Remaining Shells (T1 — 3 tests)

| # | Test Function | Assert |
|---|---|---|
| 40 | `completions_fish_success_path` | Ok |
| 41 | `completions_powershell_success_path` | Ok |
| 42 | `completions_elvish_success_path` | Ok |

### 2.11 Config Variants (T1/T2 — 2 tests)

| # | Test Function | Assert |
|---|---|---|
| 43 | `config_schema_json_success_path` | Ok |
| 44 | `config_show_raw_success_path` | Ok |

### 2.12 Memory (T2 — 3 tests)

| # | Test Function | Assert |
|---|---|---|
| 45 | `memory_get_empty_negative_path` | Err (not-found) |
| 46 | `memory_get_missing_key_negative_path` | Err (not-found) |
| 47 | `memory_list_with_limit_success_path` | Ok |

### 2.13 Models (T2 — 1 test)

| # | Test Function | Assert |
|---|---|---|
| 48 | `models_list_success_path` | Ok |

### 2.14 Parse-Only Smoke Tests (T3 — 6 tests)

| # | Test Function | Assert |
|---|---|---|
| 49 | `tunnel_start_args_parse_success_path` | parse Ok |
| 50 | `channel_start_args_parse_success_path` | parse Ok |
| 51 | `auth_login_args_parse_success_path` | parse Ok |
| 52 | `plugin_dev_args_parse_success_path` | parse Ok |
| 53 | `plugin_package_args_parse_success_path` | parse Ok |
| 54 | `onboard_interactive_args_parse_success_path` | parse Ok |

---

## Implementation Notes

Behavioral discoveries made during implementation:

1. **`template show`** requires `template init` first — the template must exist on disk
2. **`template init`** without `--force` skips existing files silently (does not error)
3. **`memory get`** returns error for missing entries (not empty success)
4. **`plugin remove`** for missing ID is idempotent (prints message, does not error)
5. **`plugin new`** generates `manifest.json` (not `plugin.toml`)

## Verification

```bash
# Run all CLI integration tests
cargo test -p agentzero-cli --test cli_integration
# Expected: 113 passed; 0 failed

# Run with feature-gated tests
cargo test -p agentzero-cli --test cli_integration --features rag,hardware

# Verify total test count
cargo test -p agentzero-cli --test cli_integration 2>&1 | grep "test result"
```

## Files Modified

- `crates/agentzero-cli/tests/cli_integration.rs` — 58 new integration tests added
- `specs/plans/02-cli-manual-and-integration-test-plan.md` — this plan document
- `specs/SPRINT.md` — changelog entry added
