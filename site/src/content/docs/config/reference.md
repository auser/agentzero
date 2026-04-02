---
title: Config Reference
description: Complete annotated agentzero.toml configuration reference with all sections and defaults.
---

AgentZero uses a single `agentzero.toml` file located in the data directory (default: `~/.agentzero/`). Generate a starter config with `agentzero onboard`.

## Full Configuration Template

```toml
# ─── Provider ────────────────────────────────────────────
[provider]
kind = "openrouter"                              # openai, openrouter, anthropic, ollama, candle, builtin, custom
base_url = "https://openrouter.ai/api/v1"        # provider API endpoint (not needed for builtin)
model = "anthropic/claude-sonnet-4-6"         # model identifier
default_temperature = 0.7                        # 0.0 – 2.0
# provider_api = "openai-chat-completions"       # or "openai-responses"
# model_support_vision = true                    # enable vision/multimodal
# Fallback providers — tried in order when primary fails
# [[provider.fallback_providers]]
# kind = "openai"
# base_url = "https://api.openai.com/v1"
# model = "gpt-4o"
# api_key_env = "OPENAI_API_KEY"                 # env var holding the API key
# For local inference with no external server (requires --features candle):
# kind = "candle"
# model = "qwen2.5-coder-3b"

# Credential pooling — distribute across multiple API keys
# [provider.credential_pool]
# strategy = "round-robin"                        # fill-first, round-robin, random
# keys = ["OPENAI_KEY_1", "OPENAI_KEY_2"]        # env var names

# Local model tuning (shared by candle and builtin providers)
# [local]
# n_ctx = 8192                                   # context window (tokens)
# temperature = 0.7                              # sampling temperature
# top_p = 0.9                                    # nucleus sampling
# max_output_tokens = 2048                       # max tokens per response
# device = "auto"                                # "auto" | "cpu" | "metal" | "cuda"

# ─── Memory ──────────────────────────────────────────────
[memory]
backend = "sqlite"                               # sqlite or turso
sqlite_path = "~/.agentzero/agentzero.db"        # database file path

# ─── Agent Settings ──────────────────────────────────────
[agent]
max_tool_iterations = 20                         # max tool calls per turn
request_timeout_ms = 30000                       # per-request timeout
memory_window_size = 50                          # context window (message count)
max_prompt_chars = 8000                          # max prompt character length
mode = "development"                             # development or production
parallel_tools = false                           # parallel tool execution
tool_dispatcher = "auto"                         # auto or sequential
compact_context = true                           # compress context when large

# Loop detection thresholds
loop_detection_no_progress_threshold = 3
loop_detection_ping_pong_cycles = 2
loop_detection_failure_streak = 3

# Enable the agent_manage tool (create/list/update/delete persistent agents)
enable_agent_manage = false

# Enable dynamic tool creation at runtime (tool_create tool).
# When enabled, agents can create new tools mid-session that persist across restarts.
# Created tools are stored encrypted in .agentzero/dynamic-tools.json.
enable_dynamic_tools = false

[agent.summarization]
enabled = false                                  # enable context summarization
keep_recent = 10                                 # messages to keep verbatim
min_entries_for_summarization = 20               # minimum entries before triggering
max_summary_chars = 2000                         # max summary length
compression_enabled = false                      # enable 4-phase context compression
max_tool_result_chars = 4000                     # truncate tool results beyond this
protect_head = 3                                 # messages to protect at start
protect_tail = 10                                # messages to protect at end

[agent.hooks]
enabled = false
timeout_ms = 250
fail_closed = false
on_error_default = "warn"                        # block, warn, or ignore
# on_error_low = "ignore"
# on_error_medium = "warn"
# on_error_high = "block"

# ─── Security ────────────────────────────────────────────
[security]
allowed_root = "."                               # filesystem scope root
allowed_commands = ["ls", "pwd", "cat", "echo"]  # shell command allowlist

[security.read_file]
max_read_bytes = 65536                           # 64 KiB
allow_binary = false

[security.write_file]
enabled = false                                  # explicit opt-in required
max_write_bytes = 65536

[security.shell]
max_args = 8
max_arg_length = 128
max_output_bytes = 8192
forbidden_chars = ";&|><$`\n\r"
context_aware_parsing = true

[security.mcp]
enabled = false
allowed_servers = []                             # empty = allow all servers from mcp.json; non-empty = allowlist filter

[security.plugin]
enabled = false

[security.audit]
enabled = false
path = "./agentzero-audit.log"

[security.url_access]
block_private_ip = true
allow_loopback = false
enforce_domain_allowlist = false
domain_allowlist = []
domain_blocklist = []

[security.otp]
enabled = false
method = "totp"
token_ttl_secs = 30
cache_valid_secs = 300
gated_actions = ["shell", "file_write", "browser_open", "browser", "memory_forget"]

[security.estop]
enabled = false
state_file = "~/.agentzero/estop-state.json"
require_otp_to_resume = true

[security.outbound_leak_guard]
enabled = true
action = "redact"                                # redact or block
sensitivity = 0.7

[security.syscall_anomaly]
enabled = true
strict_mode = false
alert_on_unknown_syscall = true
max_denied_events_per_minute = 5
max_alerts_per_minute = 30

# ─── Autonomy ────────────────────────────────────────────
[autonomy]
level = "supervised"                             # supervised or autonomous
workspace_only = true
forbidden_paths = ["/etc", "/root", "/proc", "/sys", "~/.ssh", "~/.gnupg", "~/.aws"]
max_actions_per_hour = 20
max_cost_per_day_cents = 500
require_approval_for_medium_risk = true
block_high_risk_commands = true

# ─── Gateway ─────────────────────────────────────────────
[gateway]
host = "127.0.0.1"
port = 42617
require_pairing = true
allow_public_bind = false
# allow_insecure = false                          # set true to bypass TLS requirement in production mode
# relay_mode = false                              # when true, only relay routes active (agent endpoints → 503)

# TLS configuration (requires --features tls). When present, gateway serves HTTPS.
# [gateway.tls]
# cert_path = "/path/to/cert.pem"                # PEM certificate or chain
# key_path = "/path/to/key.pem"                  # PEM private key

[gateway.node_control]
enabled = false
# auth_token = "****"
allowed_node_ids = []

[gateway.relay]
# timing_jitter_ms = 500                          # random jitter range added to relay responses (ms)
# max_mailbox_size = 1000                         # max envelopes per routing_id mailbox
# gc_interval_secs = 60                           # expired envelope garbage collection interval

# ─── Observability ───────────────────────────────────────
[observability]
backend = "none"                                 # none or otel
otel_endpoint = "http://localhost:4318"
otel_service_name = "agentzero"
runtime_trace_mode = "none"                      # none or file
runtime_trace_path = "state/runtime-trace.jsonl"
runtime_trace_max_entries = 200

# ─── Cost Tracking ───────────────────────────────────────
[cost]
enabled = false
daily_limit_usd = 10.0
monthly_limit_usd = 100.0
warn_at_percent = 80

# ─── Identity ────────────────────────────────────────────
[identity]
format = "markdown"                              # markdown or aieos
# aieos_path = "identity.json"

# ─── Runtime ─────────────────────────────────────────────
[runtime]
kind = "native"                                  # native or docker
# reasoning_enabled = true                       # enable extended thinking
# adaptive_reasoning = true                      # auto-adjust effort by query complexity

[runtime.wasm]
tools_dir = "tools/wasm"
fuel_limit = 1000000
memory_limit_mb = 64
max_module_size_mb = 50
allow_workspace_read = false
allow_workspace_write = false
allowed_hosts = []

# Host tools exposed to WASM plugins via CLI shim bridge (HTTP+shell shims)
# allowed_host_tools = ["read_file", "shell"]   # empty = none exposed
# Filesystem overlay mode for sandboxed writes
# overlay_mode = "disabled"                      # disabled, auto_commit, explicit_commit, dry_run

[runtime.wasm.security]
require_workspace_relative_tools_dir = true
reject_symlink_modules = true
reject_symlink_tools_dir = true
strict_host_validation = true
capability_escalation_mode = "deny"
module_hash_policy = "warn"                      # warn or enforce

# ─── Tools ───────────────────────────────────────────────
[browser]
enabled = false
backend = "agent_browser"

[http_request]
enabled = false
allowed_domains = []
max_response_size = 1000000
timeout_secs = 30

[web_fetch]
enabled = false
provider = "fast_html2md"
max_response_size = 500000

[web_search]
enabled = false
provider = "duckduckgo"
max_results = 5

[composio]
enabled = false

# ─── Skills ──────────────────────────────────────────────
[skills]
open_skills_enabled = false
prompt_injection_mode = "full"

# ─── Multimodal ──────────────────────────────────────────
[multimodal]
max_images = 4
max_image_size_mb = 5
allow_remote_fetch = false

# ─── Audio Input ─────────────────────────────────────────
[audio]
api_url = "https://api.groq.com/openai/v1/audio/transcriptions"  # transcription endpoint
api_key = ""                                                      # API key (e.g. Groq, OpenAI)
language = "en"                                                   # ISO-639-1 language hint
model = "whisper-large-v3"                                        # transcription model

# ─── Autopilot ──────────────────────────────────────────
[autopilot]
enabled = false                                  # enable autonomous company loop
supabase_url = ""                                # Supabase project URL
supabase_service_role_key = ""                   # Supabase service role key (env: SUPABASE_SERVICE_ROLE_KEY)
max_daily_spend_cents = 500                      # daily spend cap in cents
max_concurrent_missions = 5                      # max missions running at once
max_proposals_per_hour = 20                      # proposal rate limit
max_missions_per_agent_per_day = 10              # per-agent mission cap
stale_threshold_minutes = 30                     # heartbeat threshold for stale detection
reaction_matrix_path = ""                        # path to reactions.json

# Autopilot triggers (cron, event-driven, metric thresholds)
# [[autopilot.triggers]]
# name = "periodic_topic_proposal"
# condition = { type = "cron", schedule = "0 */6 * * *" }
# action = { type = "propose_task", agent = "editor", prompt = "Propose a new blog topic" }
# cooldown_secs = 21600

# ─── Research Mode ───────────────────────────────────────
[research]
enabled = false
trigger = "never"                                # never, always, or keyword
max_iterations = 5

# ─── Model Provider Profiles ─────────────────────────────
# [model_providers.local-ollama]
# base_url = "http://localhost:11434/v1"
# model = "llama3.2"

# ─── Model Routes ────────────────────────────────────────
# [[model_routes]]
# hint = "code"
# provider = "openrouter"
# model = "anthropic/claude-sonnet-4-6"

# ─── Delegate Sub-Agents ─────────────────────────────────
# [agents.researcher]
# provider = "openrouter"
# model = "anthropic/claude-sonnet-4-6"
# max_depth = 3
# agentic = true
# allowed_tools = ["web_search", "web_fetch"]
# privacy_boundary = "encrypted_only"            # inherit, local_only, encrypted_only, any
# allowed_providers = ["anthropic"]               # restrict to specific provider kinds
# blocked_providers = []                          # block specific provider kinds
# [agents.researcher.instruction_method]
# type = "system_prompt"                          # system_prompt (default), tool_definition, custom
# # For tool_definition: instructions injected as a tool description
# # type = "tool_definition"
# # tool_name = "instructions_reader"
# # For custom: template with {instructions} placeholder
# # type = "custom"
# # template = "SYSTEM: {instructions}"

# ─── Privacy ────────────────────────────────────────────
[privacy]
mode = "off"                                      # off | local_only | encrypted | full
# block_cloud_providers = false                   # legacy; prefer mode = "local_only"
# enforce_local_provider = false                  # force local provider regardless of mode

[privacy.noise]
enabled = false                                   # auto-enabled by mode = "encrypted" or "full"
handshake_pattern = "XX"                          # XX (mutual auth) or IK (known server key)
session_timeout_secs = 3600                       # session TTL in seconds
max_sessions = 1000                               # max concurrent Noise sessions

[privacy.sealed_envelopes]
enabled = false                                   # auto-enabled by mode = "full"
default_ttl_secs = 86400                          # default envelope TTL (seconds)
max_envelope_bytes = 65536                        # max sealed envelope payload size
timing_jitter_enabled = false                     # randomized delays on relay responses
submit_jitter_min_ms = 10                         # min delay on submit (ms)
submit_jitter_max_ms = 100                        # max delay on submit (ms)
poll_jitter_min_ms = 20                           # min delay on poll (ms)
poll_jitter_max_ms = 200                          # max delay on poll (ms)

[privacy.key_rotation]
enabled = false                                   # auto-enabled by mode = "encrypted" or "full"
rotation_interval_secs = 604800                   # seconds between rotations (default: 7 days)
overlap_secs = 86400                              # overlap where both old and new keys are valid
key_store_path = ""                               # key persistence directory (empty = data dir)

# [security.tool_boundaries]                      # per-tool privacy boundaries
# shell = "local_only"
# web_search = "any"

# [channels_config]
# default_privacy_boundary = "encrypted_only"     # global default for all channels

# [channels_config.voice_wake]
# wake_words = ["hey agent", "ok computer"]       # wake words to listen for
# energy_threshold = 0.05                         # RMS energy threshold for VAD
# capture_timeout_secs = 10                       # max capture duration
# transcription_url = "https://api.groq.com/openai/v1/audio/transcriptions"
# transcription_api_key = ""                      # or GROQ_API_KEY env var
# sample_rate = 16000                             # audio sample rate (Hz)
# auto_tts_response = false                       # auto-speak agent responses
```

## [audio]

Configures speech-to-text transcription for audio input markers in user messages.

When a user message contains an `[AUDIO:/path/to/file.wav]` marker, AgentZero transcribes the file before passing the message to the LLM. If `api_key` is not set, audio markers are stripped silently with a warning rather than causing an error.

| Field | Default | Description |
|---|---|---|
| `api_url` | `https://api.groq.com/openai/v1/audio/transcriptions` | OpenAI-compatible transcription endpoint |
| `api_key` | `""` | API key for the transcription service |
| `language` | `"en"` | ISO-639-1 language hint passed to the model |
| `model` | `"whisper-large-v3"` | Transcription model identifier |

Supported audio formats: `flac`, `mp3`, `mp4`, `m4a`, `ogg`, `opus`, `wav`, `webm` (max 25 MB per file).

The default endpoint is compatible with the Groq audio API. To use OpenAI directly, set `api_url = "https://api.openai.com/v1/audio/transcriptions"` and `model = "whisper-1"`.

## Config Inspection Commands

```bash
# Show effective config (secrets masked)
agentzero config show

# Show raw config (secrets visible)
agentzero config show --raw

# Query a single value
agentzero config get provider.model

# Set a value
agentzero config set provider.model "anthropic/claude-sonnet-4-6"

# Print TOML template
agentzero config schema

# Print JSON schema
agentzero config schema --json
```

## Config Precedence

1. CLI flags (highest)
2. Environment variables
3. `agentzero.toml` file
4. Compiled defaults (lowest)

## Data Directory

Default: `~/.agentzero/`

Override with:
- `--data-dir <path>` flag (highest)
- `AGENTZERO_DATA_DIR` env var
- `data_dir` in config file
