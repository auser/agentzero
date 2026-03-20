---
title: Provider Setup Guides
description: Step-by-step instructions for connecting AgentZero to OpenAI, Anthropic, OpenRouter, Ollama, and other providers.
---

This guide covers setup for the most common providers. AgentZero supports 37 providers — run `agentzero providers` for the full list.

## OpenAI

1. Get an API key from [platform.openai.com/api-keys](https://platform.openai.com/api-keys).
2. Configure:

```bash
agentzero onboard --provider openai --model gpt-4o --yes
agentzero auth setup-token --provider openai --token sk-...
```

Or set the environment variable:

```bash
export OPENAI_API_KEY="sk-..."
```

**TOML config:**

```toml
[provider]
kind = "openai"
base_url = "https://api.openai.com/v1"
model = "gpt-4o"
```

**Available models:** `gpt-4o`, `gpt-4o-mini`, `gpt-4-turbo`, `o1`, `o1-mini`, `o3-mini`

---

## Anthropic

**Option A: Browser login (recommended)** — uses your claude.ai subscription:

```bash
agentzero onboard --provider anthropic --model claude-sonnet-4-6 --yes
agentzero auth login --provider anthropic       # opens browser for OAuth
```

**Option B: API key** — from [console.anthropic.com/settings/keys](https://console.anthropic.com/settings/keys):

```bash
agentzero auth setup-token --provider anthropic --token sk-ant-...
```

Or set the environment variable:

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

**TOML config:**

```toml
[provider]
kind = "anthropic"
base_url = "https://api.anthropic.com"
model = "claude-sonnet-4-6"
```

**Available models:** `claude-opus-4-6`, `claude-sonnet-4-6`, `claude-haiku-4-5-20251001`

:::note
Anthropic uses a different API format (Messages API) from OpenAI. AgentZero handles this automatically when `kind = "anthropic"`.
:::

---

## OpenRouter

OpenRouter gives you access to hundreds of models through a single API key.

1. Get an API key from [openrouter.ai/keys](https://openrouter.ai/keys).
2. Configure:

```bash
agentzero onboard --provider openrouter --model anthropic/claude-sonnet-4-6 --yes
agentzero auth setup-token --provider openrouter --token sk-or-v1-...
```

Or set the environment variable:

```bash
export OPENROUTER_API_KEY="sk-or-v1-..."
```

**TOML config:**

```toml
[provider]
kind = "openrouter"
base_url = "https://openrouter.ai/api/v1"
model = "anthropic/claude-sonnet-4-6"
```

**Model names** use the format `provider/model` — e.g., `openai/gpt-4o`, `google/gemini-pro`, `meta-llama/llama-3.1-70b-instruct`.

---

## Built-in Local Model (recommended for local)

AgentZero includes a built-in local LLM provider powered by llama.cpp. No external server or API key needed — the model runs entirely in-process.

**Default model:** Qwen2.5-Coder-3B-Instruct (Q4_K_M quantization, ~2 GB download on first run)

### Setup

1. Build with the `local-model` feature:

```bash
cargo build --release --features local-model
```

1. Configure:

```toml
[provider]
kind = "builtin"
model = "qwen2.5-coder-3b"
```

That's it. On first run, AgentZero automatically downloads the model from HuggingFace Hub to `~/.agentzero/models/` and shows a progress bar.

### Custom GGUF models

You can use any GGUF model file:

```toml
# Local file path
model = "/path/to/my-model.gguf"

# HuggingFace repo (org/repo/filename.gguf)
model = "TheBloke/Mistral-7B-Instruct-v0.2-GGUF/mistral-7b-instruct-v0.2.Q4_K_M.gguf"
```

### Tool use

The builtin provider supports tool calling via Qwen's `<tool_call>` prompt format. Tool definitions are automatically injected into the system prompt and model outputs are parsed for tool invocations. All built-in tools and plugin tools work with the builtin provider.

### GPU acceleration

On macOS (Apple Silicon), the model automatically offloads to the GPU via Metal. On Linux with CUDA, GPU offloading is used when available.

### Limitations

- The default 3B model is best for simple tasks — coding assistance, file operations, basic research
- For complex multi-step pipelines, consider using a larger model or a cloud provider
- Vision/image inputs are not supported

:::note
The `local-model` feature requires a C++ compiler for llama.cpp bindings. On macOS this is included with Xcode Command Line Tools. On Linux, install `build-essential` or equivalent.
:::

---

## Ollama (local)

Ollama runs models locally. No API key needed.

1. Install Ollama from [ollama.com](https://ollama.com).
2. Pull a model:

```bash
ollama pull llama3.1:8b
```

1. Start Ollama (it runs on `http://localhost:11434` by default):

```bash
ollama serve
```

1. Configure AgentZero:

```bash
agentzero onboard --provider ollama --model llama3.1:8b --yes
```

**TOML config:**

```toml
[provider]
kind = "ollama"
base_url = "http://localhost:11434/v1"
model = "llama3.1:8b"
```

AgentZero can auto-discover local Ollama instances:

```bash
agentzero local discover
```

---

## Other Local Providers

### LM Studio

```toml
[provider]
kind = "lmstudio"
base_url = "http://localhost:1234/v1"
model = "your-model-name"
```

### llama.cpp server

```toml
[provider]
kind = "llamacpp"
base_url = "http://localhost:8080/v1"
model = "default"
```

### vLLM

```toml
[provider]
kind = "vllm"
base_url = "http://localhost:8000/v1"
model = "your-model-name"
```

---

## Cloud Providers with Default URLs

These providers have built-in base URLs — you only need to set the API key:

| Provider | Kind | Env Var |
|---|---|---|
| Groq | `groq` | `GROQ_API_KEY` |
| Mistral | `mistral` | `MISTRAL_API_KEY` |
| xAI (Grok) | `xai` | `XAI_API_KEY` |
| DeepSeek | `deepseek` | `DEEPSEEK_API_KEY` |
| Together AI | `together` | `TOGETHER_API_KEY` |
| Fireworks AI | `fireworks` | — |
| Perplexity | `perplexity` | — |
| Cohere | `cohere` | — |
| NVIDIA NIM | `nvidia` | — |

Example for Groq:

```bash
agentzero onboard --provider groq --model llama-3.1-70b-versatile --yes
export GROQ_API_KEY="gsk_..."
```

---

## Custom Endpoints

For any OpenAI-compatible API not in the catalog:

```toml
[provider]
kind = "custom:https://my-api.example.com/v1"
model = "my-model"
```

For Anthropic-compatible APIs:

```toml
[provider]
kind = "anthropic-custom:https://my-proxy.example.com"
model = "claude-sonnet-4-6"
```

---

## Transport Configuration

Per-provider transport settings can be configured for timeout, retries, and circuit breaking:

```toml
[provider.transport]
timeout_ms = 30000              # request timeout (default: 30s)
max_retries = 3                 # retry count on failure (default: 3)
circuit_breaker_threshold = 5   # failures before circuit opens (default: 5)
circuit_breaker_reset_ms = 30000 # time before half-open retry (default: 30s)
```

**Retry policy:** Retries on `429 Too Many Requests` and `5xx` server errors with exponential backoff and jitter. Honors `Retry-After` headers when present. Non-retryable errors (401, 403, 404) fail immediately.

**Circuit breaker:** Tracks consecutive failures per provider. After reaching the threshold, the circuit opens and rejects requests for the reset duration. It then transitions to half-open, allowing a single probe request. A successful probe closes the circuit; a failed probe reopens it.

**Observability:** Provider requests are instrumented with `tracing` spans (`anthropic_complete`, `openai_stream`, etc.). Request/response events log at `info!` level with provider, model, status, body size, and latency. Retries log at `warn!` level. Circuit breaker state transitions log at `info!`/`warn!` level.

---

## Provider Fallback Chains

Configure backup providers that activate automatically when the primary provider fails (circuit breaker open, 5xx errors, timeouts):

```toml
[provider]
kind = "anthropic"
base_url = "https://api.anthropic.com"
model = "claude-sonnet-4-6"

[[provider.fallback_providers]]
kind = "openai"
base_url = "https://api.openai.com/v1"
model = "gpt-4o"
api_key_env = "OPENAI_API_KEY"

[[provider.fallback_providers]]
kind = "openrouter"
base_url = "https://openrouter.ai/api/v1"
model = "anthropic/claude-sonnet-4-6"
api_key_env = "OPENROUTER_API_KEY"
```

Providers are tried in order. The first successful response is used. Each fallback entry requires:

| Field | Description |
|---|---|
| `kind` | Provider type (`openai`, `anthropic`, `openrouter`, etc.) |
| `base_url` | API endpoint URL |
| `model` | Model identifier for this provider |
| `api_key_env` | Environment variable name holding the API key |

Fallback events emit the `provider_fallback_total{from, to}` Prometheus metric so you can monitor how often failover occurs.

:::note
Streaming requests fall back to non-streaming on secondary providers to avoid duplicate partial chunks. The response is still returned correctly — just not streamed incrementally from the fallback provider.
:::

---

## Checking Provider Status

```bash
# List all supported providers (marks active one)
agentzero providers

# Check provider quota and API key status
agentzero providers quota

# Diagnose model availability
agentzero doctor models
```
