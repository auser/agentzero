# CLI Commands Test Plan

Complete inventory of all `agentzero` CLI commands, their test coverage, and manual test procedures.

## Test Methodology

- **Unit tests**: Inline `#[cfg(test)]` modules in each command handler file. Test rendering logic, pure functions, and error paths without network or filesystem side effects.
- **Integration tests**: `crates/agentzero-cli/tests/cli_integration.rs`. Exercise commands through the public `parse_cli_from` + `execute` API with isolated temp directories.
- **Manual tests**: Commands that require running services, interactive TTY, or OS-level setup (daemon, gateway, dashboard, local service discovery).

---

## Command Inventory

### 1. `agentzero onboard`

| | |
|---|---|
| **File** | `commands/onboard.rs` |
| **Subcommands** | (none — flags only) |
| **Flags** | `--interactive`, `--force`, `--channels-only`, `--api-key`, `--yes`, `--provider`, `--base-url`, `--model`, `--memory`, `--memory-path`, `--no-totp`, `--allowed-root`, `--allowed-commands` |
| **Unit tests** | 6: `creates_config_from_interactive_answers`, `does_not_overwrite_when_user_declines`, `resolves_onboard_config_with_env_values`, `flag_values_override_env_values`, `base_url_options_include_current_and_custom_without_duplicates`, `allowed_commands_options_include_custom_and_filter_empty` |
| **Integration tests** | `onboard_quick_mode_success_path` |
| **Manual test** | `agentzero onboard --interactive` (requires TTY) |

### 2. `agentzero gateway`

| | |
|---|---|
| **File** | `commands/gateway.rs` |
| **Flags** | `--host`, `--port`, `--new-pairing` |
| **Unit tests** | 0 (thin wrapper over `agentzero_gateway::run`) |
| **Integration tests** | `gateway_args_parse_success_path` (parse-only) |
| **Manual test** | `agentzero gateway --port 9090` — verify HTTP listener starts, pairing flow works |

### 3. `agentzero daemon`

| | |
|---|---|
| **File** | `commands/daemon.rs` |
| **Flags** | `--host`, `--port` |
| **Unit tests** | 0 (thin wrapper over gateway + DaemonManager) |
| **Integration tests** | `daemon_args_parse_success_path` (parse-only) |
| **Manual test** | `agentzero daemon --port 9091` — verify state file written, clean shutdown writes stop marker |

### 4. `agentzero agent`

| | |
|---|---|
| **File** | `commands/agent.rs` |
| **Flags** | `--message` / `-m` |
| **Unit tests** | 1: `agent_command_fails_when_api_key_missing` |
| **Integration tests** | `agent_args_parse_success_path` (parse-only) |
| **Manual test** | `agentzero agent -m "hello"` — requires running provider |

### 5. `agentzero status`

| | |
|---|---|
| **File** | `commands/status.rs` |
| **Flags** | (none) |
| **Unit tests** | 0 (renders config summary) |
| **Integration tests** | `status_with_config_success_path` |
| **Manual test** | N/A — covered by integration test |

### 6. `agentzero auth`

| | |
|---|---|
| **File** | `commands/auth.rs` |
| **Subcommands** | `login`, `paste-redirect`, `paste-token`, `setup-token`, `refresh`, `logout`, `use`, `list`, `status` |
| **Unit tests** | 14 |
| **Integration tests** | `auth_list_success_path`, `auth_status_success_path` |
| **Manual test** | `agentzero auth login --provider openai-codex` (requires OAuth redirect) |

### 7. `agentzero cron`

| | |
|---|---|
| **File** | `commands/cron.rs` |
| **Subcommands** | `list`, `add`, `add-at`, `add-every`, `once`, `update`, `pause`, `resume`, `remove` |
| **Unit tests** | 3 |
| **Integration tests** | `cron_full_lifecycle_success_path`, `cron_remove_missing_negative_path` |
| **Manual test** | N/A — covered by tests |

### 8. `agentzero hooks`

| | |
|---|---|
| **File** | `commands/hooks.rs` |
| **Subcommands** | `list`, `enable`, `disable`, `test` |
| **Unit tests** | 2 |
| **Integration tests** | `hooks_list_empty_success_path`, `hooks_enable_missing_negative_path` |
| **Manual test** | N/A |

### 9. `agentzero skill`

| | |
|---|---|
| **File** | `commands/skill.rs` |
| **Subcommands** | `list`, `install`, `test`, `remove` |
| **Unit tests** | 2 |
| **Integration tests** | `skill_list_empty_success_path` |
| **Manual test** | N/A |

### 10. `agentzero tunnel`

| | |
|---|---|
| **File** | `commands/tunnel.rs` |
| **Subcommands** | `start`, `stop`, `status` |
| **Unit tests** | 2 |
| **Integration tests** | `tunnel_status_missing_negative_path` |
| **Manual test** | `agentzero tunnel start --protocol http --remote host:port --local-port 8080` (requires target) |

### 11. `agentzero plugin`

| | |
|---|---|
| **File** | `commands/plugin.rs` |
| **Subcommands** | `new`, `validate`, `test`, `package`, `dev`, `install`, `list`, `remove` |
| **Unit tests** | 6 |
| **Integration tests** | `plugin_new_and_list_success_path` |
| **Manual test** | N/A |

### 12. `agentzero providers`

| | |
|---|---|
| **File** | `commands/providers.rs` |
| **Flags** | `--json`, `--no-color` |
| **Unit tests** | 6: `render_providers_marks_configured_provider_as_active`, `render_providers_marks_alias_match_as_active`, `render_providers_warns_when_active_provider_is_unknown`, `render_providers_colorizes_provider_id_column`, `render_providers_no_color_emits_plain_text`, `render_providers_json_is_uncolored_and_includes_active_state` |
| **Integration tests** | `providers_list_success_path`, `providers_no_color_success_path` |
| **Manual test** | N/A |

### 13. `agentzero providers-quota`

| | |
|---|---|
| **File** | `commands/providers.rs` |
| **Flags** | `--provider`, `--json` |
| **Unit tests** | 0 |
| **Integration tests** | `providers_quota_success_path` |
| **Manual test** | N/A — covered by integration test |

### 14. `agentzero estop`

| | |
|---|---|
| **File** | `commands/estop.rs` |
| **Subcommands** | `status`, `resume` |
| **Flags** | `--level`, `--domain`, `--tool`, `--require-otp` |
| **Unit tests** | 5 |
| **Integration tests** | `estop_status_success_path`, `estop_engage_and_resume_success_path` |
| **Manual test** | N/A |

### 15. `agentzero channel`

| | |
|---|---|
| **File** | `commands/channel.rs` |
| **Subcommands** | `add`, `bind-telegram`, `doctor`, `list`, `remove`, `start` |
| **Unit tests** | 8 |
| **Integration tests** | `channel_list_success_path`, `channel_doctor_success_path` |
| **Manual test** | `agentzero channel start` (requires Telegram bot token) |

### 16. `agentzero integrations`

| | |
|---|---|
| **File** | `commands/integrations.rs` |
| **Subcommands** | `info`, `list`, `search` |
| **Unit tests** | 3 |
| **Integration tests** | `integrations_list_success_path`, `integrations_search_success_path`, `integrations_info_success_path` |
| **Manual test** | N/A |

### 17. `agentzero local`

| | |
|---|---|
| **File** | `commands/local.rs` |
| **Subcommands** | `discover`, `status`, `health` |
| **Unit tests** | 3: `health_check_unknown_provider_returns_error`, `status_non_local_provider_reports_not_local`, `discover_json_output_is_valid_json_array` |
| **Integration tests** | `local_discover_success_path`, `local_status_success_path`, `local_health_unknown_provider_negative_path` |
| **Manual test** | `agentzero local discover` with Ollama running — verify running services shown with model list |

### 18. `agentzero models`

| | |
|---|---|
| **File** | `commands/models.rs` |
| **Subcommands** | `refresh`, `list`, `set`, `status`, `pull` |
| **Unit tests** | 7: `cache_round_trip_success_path`, `load_cache_returns_error_for_invalid_json_negative_path`, `load_cached_respects_ttl_success_path`, `upsert_provider_model_updates_existing_config_success_path`, `upsert_provider_model_rejects_invalid_toml_negative_path`, `humanize_age_formats_units_success_path`, `cached_model_catalog_serde_round_trip_success_path` |
| **Integration tests** | `models_status_success_path`, `models_set_success_path`, `models_pull_non_pull_provider_negative_path` |
| **Manual test** | `agentzero models pull llama3.1:8b` — requires running Ollama |

### 19. `agentzero approval`

| | |
|---|---|
| **File** | `commands/approval.rs` |
| **Subcommands** | `evaluate` |
| **Unit tests** | 2 |
| **Integration tests** | `approval_evaluate_success_path`, `approval_evaluate_deny_returns_error_negative_path`, `approval_evaluate_allow_success_path` |
| **Manual test** | N/A |

### 20. `agentzero identity`

| | |
|---|---|
| **File** | `commands/identity.rs` |
| **Subcommands** | `upsert`, `get`, `add-role` |
| **Unit tests** | 2 |
| **Integration tests** | `identity_upsert_get_add_role_success_path`, `identity_get_missing_negative_path` |
| **Manual test** | N/A |

### 21. `agentzero coordination`

| | |
|---|---|
| **File** | `commands/coordination.rs` |
| **Subcommands** | `status`, `set` |
| **Unit tests** | 1 |
| **Integration tests** | `coordination_set_and_status_success_path` |
| **Manual test** | N/A |

### 22. `agentzero cost`

| | |
|---|---|
| **File** | `commands/cost.rs` |
| **Subcommands** | `status`, `record`, `reset` |
| **Unit tests** | 1 |
| **Integration tests** | `cost_record_status_reset_success_path` |
| **Manual test** | N/A |

### 23. `agentzero goals`

| | |
|---|---|
| **File** | `commands/goals.rs` |
| **Subcommands** | `list`, `add`, `complete` |
| **Unit tests** | 2 |
| **Integration tests** | `goals_add_complete_list_success_path`, `goals_complete_missing_negative_path` |
| **Manual test** | N/A |

### 24. `agentzero doctor`

| | |
|---|---|
| **File** | `commands/doctor.rs` |
| **Subcommands** | `models`, `traces` |
| **Unit tests** | 4 |
| **Integration tests** | `doctor_traces_success_path` |
| **Manual test** | `agentzero doctor models` — probes live providers |

### 25. `agentzero service`

| | |
|---|---|
| **File** | `commands/service.rs` |
| **Subcommands** | `install`, `start`, `stop`, `restart`, `uninstall`, `status` |
| **Unit tests** | 2 |
| **Integration tests** | `service_lifecycle_success_path` |
| **Manual test** | `agentzero service install` — requires systemd/openrc |

### 26. `agentzero dashboard`

| | |
|---|---|
| **File** | `commands/dashboard.rs` |
| **Flags** | (none) |
| **Unit tests** | 2: `snapshot_fields_reflect_render_data_success_path`, `dashboard_command_requires_tty_negative_path` |
| **Integration tests** | `dashboard_args_parse_success_path` (parse-only) |
| **Manual test** | `agentzero dashboard` — requires TTY |

### 27. `agentzero migrate`

| | |
|---|---|
| **File** | `commands/update.rs` |
| **Subcommands** | `openclaw` |
| **Unit tests** | 0 |
| **Integration tests** | `migrate_openclaw_dry_run_success_path` |
| **Manual test** | N/A |

### 28. `agentzero update`

| | |
|---|---|
| **File** | `commands/update.rs` |
| **Subcommands** | `check`, `apply`, `rollback`, `status` |
| **Unit tests** | 4 |
| **Integration tests** | `update_status_success_path`, `update_check_success_path` |
| **Manual test** | `agentzero update apply --version X.Y.Z` (modifies binary) |

### 29. `agentzero completions`

| | |
|---|---|
| **File** | `commands/completions.rs` |
| **Flags** | `--shell` |
| **Unit tests** | 2 |
| **Integration tests** | `completions_bash_success_path`, `completions_zsh_success_path`, `completions_invalid_shell_negative_path` |
| **Manual test** | N/A |

### 30. `agentzero config`

| | |
|---|---|
| **File** | `commands/config.rs` |
| **Subcommands** | `schema`, `show`, `get`, `set` |
| **Unit tests** | 6 |
| **Integration tests** | `config_schema_toml_success_path`, `config_set_and_get_round_trip_success_path`, `config_get_missing_key_negative_path`, `config_show_success_path`, `config_get_provider_kind_success_path` |
| **Manual test** | N/A |

### 31. `agentzero memory`

| | |
|---|---|
| **File** | `commands/memory.rs` |
| **Subcommands** | `list`, `get`, `stats`, `clear` |
| **Unit tests** | 4 |
| **Integration tests** | `memory_stats_success_path`, `memory_list_success_path`, `memory_clear_success_path` |
| **Manual test** | N/A |

### 32. `agentzero rag`

| | |
|---|---|
| **File** | `commands/rag.rs` |
| **Subcommands** | `ingest`, `query` |
| **Unit tests** | 3 |
| **Integration tests** | `rag_ingest_and_query_success_path` |
| **Manual test** | N/A |

### 33. `agentzero hardware`

| | |
|---|---|
| **File** | `commands/hardware.rs` |
| **Subcommands** | `discover`, `info`, `introspect` |
| **Unit tests** | 1: `hardware_command_without_feature_fails_negative_path` |
| **Integration tests** | `hardware_discover_success_path`, `hardware_info_success_path` |
| **Manual test** | Requires `hardware` feature flag |

### 34. `agentzero peripheral`

| | |
|---|---|
| **File** | `commands/peripheral.rs` |
| **Subcommands** | `list`, `add`, `flash`, `flash-nucleo`, `setup-uno-q` |
| **Unit tests** | 1 |
| **Integration tests** | `peripheral_list_empty_success_path`, `peripheral_add_and_list_success_path` |
| **Manual test** | `flash` / `flash-nucleo` require connected hardware |

---

## Coverage Summary

| Category | Count |
|---|---|
| Total top-level commands | 34 |
| Commands with unit tests | 29 |
| Commands with integration tests | 34 |
| Commands requiring manual test | 9 |
| Total unit tests (CLI) | ~100 |
| Total integration tests (CLI) | ~55 |

## Commands Requiring Manual Testing

These commands have automated test coverage for argument parsing and error paths, but require live services or TTY for full end-to-end validation:

| Command | Reason | Setup Required |
|---|---|---|
| `agentzero gateway` | Starts HTTP listener | None (just run) |
| `agentzero daemon` | Long-running process | None (just run) |
| `agentzero agent -m "hello"` | Calls live LLM provider | Running provider + API key |
| `agentzero dashboard` | Requires TTY for TUI | Terminal |
| `agentzero local discover` | Network probing | Local model server running |
| `agentzero models pull <model>` | Downloads from Ollama | Running Ollama |
| `agentzero doctor models` | Probes live providers | Running provider |
| `agentzero service install` | OS init system | systemd or openrc |
| `agentzero channel start` | Starts channel listener | Telegram bot token |

## Manual Test Checklist

### Local Model Support (Sprint 1+2)

```bash
# 1. Config auto-resolution
echo '[provider]\nkind = "ollama"\nmodel = "llama3.1:8b"' > /tmp/az-test.toml
agentzero --config /tmp/az-test.toml config get provider.base_url
# Expected: http://localhost:11434

# 2. Local discovery (with Ollama running)
agentzero local discover
# Expected: table with ollama=running, others=offline

# 3. Local discovery JSON
agentzero local discover --json
# Expected: valid JSON array with provider/url/status/models

# 4. Local status (with ollama configured)
agentzero --config /tmp/az-test.toml local status
# Expected: Provider: ollama, URL: http://localhost:11434, Status: running/offline

# 5. Health check
agentzero local health ollama
# Expected: Reachable: yes/no, Latency: Xms

# 6. Health check with custom URL
agentzero local health ollama --url http://localhost:11434
# Expected: same as above

# 7. Health check unknown provider
agentzero local health fakeprovider
# Expected: error "Unknown provider 'fakeprovider'"

# 8. Models pull (with Ollama running)
agentzero models pull llama3.1:8b
# Expected: streaming progress, "Done" message

# 9. Models pull non-pull provider
agentzero models pull some-model --provider llamacpp
# Expected: error "does not support model pulling"

# 10. Models list from local provider
agentzero --config /tmp/az-test.toml models list
# Expected: live model list from Ollama (or fallback to static)

# 11. Agent with local provider (no API key)
unset OPENAI_API_KEY
agentzero --config /tmp/az-test.toml agent -m "say hello"
# Expected: response from local model (or connection error if not running)

# 12. Providers list shows local tag
agentzero providers
# Expected: ollama, llamacpp, lmstudio, vllm, sglang marked [local]
```
