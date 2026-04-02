---
title: Security Overview
description: AgentZero's 12-layer defense-in-depth security model — the most comprehensive security posture in any AI agent framework
---

AgentZero implements **defense-in-depth** security with 12 independent layers. Every capability is **denied by default** and must be explicitly enabled. All boundaries are **fail-closed** — if a security check can't complete, the operation is rejected.

## At a Glance

| Layer | What It Protects | Key Mechanism |
|---|---|---|
| File I/O | Path traversal, symlink attacks | Canonicalization + hard-link guard + 40+ sensitive patterns |
| Shell Execution | Command injection | Quote-aware parser + explicit allowlist |
| Encryption at Rest | Data confidentiality | XChaCha20-Poly1305 AEAD, random nonce per write |
| Network Security | SSRF, DNS rebinding | Private IP blocking + DNS resolution check |
| Credential Leak Guard | Secret exfiltration | Pattern matching + Shannon entropy + channel boundary isolation |
| LLM Guardrails | PII leakage, prompt injection | Regex-based detection, audit mode by default |
| WASM Plugin Sandbox | Malicious plugins | Fuel metering + memory cap + Ed25519 signatures |
| Gateway Security | Unauthorized access, replay attacks | mTLS + HMAC-SHA256 signing + TLS enforcement |
| MCP Attestation | Supply chain attacks | SHA-256 binary hash verification |
| Autonomy & Delegation | Privilege escalation | 3-tier autonomy + policy intersection + per-tool rate limits |
| Declarative Policies | Misconfiguration | YAML per-tool rules for egress, filesystem, commands |
| Audit & Redaction | Information leakage | Secret redaction in errors/logs/panics, path sanitization |

---

## File I/O Security

Every file operation passes through a multi-stage validation pipeline:

```
Input Path
  -> Component::ParentDir rejection (blocks ../)
  -> Canonicalization (resolves symlinks to real paths)
  -> Allowed Root check (must be within workspace)
  -> Hard-link guard (rejects files with multiple hard links)
  -> Sensitive file check (40+ patterns)
  -> Size limit check
  -> Execute
```

### Sensitive File Patterns

The following files are blocked by default (configurable via `allow_sensitive_file_reads`/`allow_sensitive_file_writes`):

**Environment files:** `.env`, `.env.*` (any suffix)

**Cloud credentials:** `.aws/credentials`, `.aws/config`, `.azure/accessTokens.json`, `.config/gcloud/credentials.db`, `.config/gcloud/application_default_credentials.json`

**SSH keys:** `.ssh/id_rsa`, `.ssh/id_ed25519`, `.ssh/id_ecdsa`, `.ssh/id_dsa`

**Kubernetes/Docker:** `.kube/config`, `.docker/config.json`

**Package registries:** `.npmrc`, `.pypirc`, `.gem/credentials`

**Database credentials:** `.pgpass`, `.my.cnf`, `.netrc`

**Certificates:** `*.pem`, `*.key`, `*.p12`, `*.pfx`

**Service accounts:** `credentials.json`, `service-account.json`, `client_secret.json`

---

## Shell Execution Security

AgentZero uses a **context-aware quote parser** that understands shell quoting rules:

- **Backtick (`` ` ``) is always forbidden** — even inside quotes
- **`$` is blocked outside quotes** — prevents variable expansion and `$(command)` substitution
- **`;`, `&`, `|`, `>`, `<` are blocked outside quotes** — prevents command chaining
- **Quoted metacharacters are safe:** `echo 'hello;world'` is allowed

Commands must be on an **explicit allowlist** — there is no path to execute arbitrary commands.

| Input | Result | Reason |
|---|---|---|
| `echo 'hello;world'` | Allowed | Semicolon inside single quotes |
| `echo hello;world` | **Blocked** | Unquoted semicolon |
| `` echo `whoami` `` | **Blocked** | Backtick always forbidden |
| `echo $HOME` | **Blocked** | Unquoted dollar sign |
| `grep "pattern" file.txt` | Allowed | Quoted argument |

---

## Encryption at Rest

All sensitive data is encrypted before writing to disk:

- **Algorithm:** XChaCha20-Poly1305 (authenticated encryption with associated data)
- **Key size:** 256-bit
- **Nonce:** 24-byte, randomly generated per encryption operation
- **Key storage:** Auto-generated, stored with Unix permissions `0o600`
- **Key sources:** File-based (`.agentzero-data.key`) or environment variable (`AGENTZERO_DATA_KEY`)
- **Atomic writes:** Data is written to a temporary file, then atomically renamed
- **API key hashing:** Raw API keys are SHA-256 hashed before storage — the raw key is never persisted

---

## Network Security

### SSRF Prevention

All network tools share a unified **URL Access Policy**:

```
URL Parse -> Scheme Check (http/https only) -> Domain Blocklist ->
IP Resolution -> Private IP Check -> DNS Rebinding Check ->
Domain Allowlist -> Execute
```

**DNS rebinding protection:** Domain names are resolved to IP addresses and verified against the blocklist. An attacker cannot register a domain that initially resolves to a public IP, then change DNS to point to `127.0.0.1`.

### Gateway Security

- **Loopback-only binding** by default — public binding requires `gateway.allow_public_bind = true`
- **Constant-time token comparison** via `subtle::ConstantTimeEq` to prevent timing side-channel attacks
- **Rate limiting:** Sliding window with per-identity isolation (600 req/min default)
- **HSTS headers** automatically applied when TLS is active
- **Request size limit:** 1 MB default maximum body size

### mTLS (Mutual TLS)

Configure `gateway.tls.client_ca_path` to require client certificates:

```toml
[gateway.tls]
cert_path = "/etc/ssl/server.pem"
key_path = "/etc/ssl/server-key.pem"
client_ca_path = "/etc/ssl/client-ca.pem"  # Enables mTLS
```

When set, clients must present a certificate signed by the specified CA. Unsigned or mis-signed connections are rejected at the TLS layer.

### HMAC Request Signing

API keys can be created with HMAC signing enabled. When active, requests must include:

- `X-AZ-Timestamp`: Unix epoch timestamp (must be within 5 minutes of server time)
- `X-AZ-Signature`: `hmac-sha256=<hex>` covering `{timestamp}.{METHOD}.{path}.{body}`

This prevents replay attacks and ensures request integrity.

### TLS Enforcement

When `AGENTZERO_ENV=production`, the gateway **refuses to start** without TLS unless `gateway.allow_insecure = true` is explicitly set.

---

## Credential Leak Guard

Outbound messages to external channels (Telegram, Discord, Slack) are scanned for credentials before sending:

### Pattern Detection

| Pattern | Example |
|---|---|
| API key prefixes | `sk-*`, `api_key=*` |
| Bearer tokens | `Bearer <token>` |
| JWT tokens | `eyJ*.eyJ*.*` |
| AWS access keys | `AKIA*` |
| Private key headers | `-----BEGIN PRIVATE KEY-----` |
| GitHub tokens | `ghp_*`, `ghs_*` |
| Anthropic keys | `sk-ant-*` |
| Slack tokens | `xox[baprs]-*` |
| X25519/Noise keys | 64-char hex strings with key-like prefixes |

### Entropy Detection

High-entropy strings (32+ characters) are flagged using Shannon entropy analysis. The sensitivity threshold is configurable (default 0.7).

### Boundary Isolation

Content marked `local_only` is blocked from being sent to remote channels. This prevents accidental leakage of internal data to external platforms.

### Custom Patterns

Add your own detection patterns in `agentzero.toml`:

```toml
[security.outbound_leak_guard]
enabled = true
action = "redact"
sensitivity = 0.7

[[security.outbound_leak_guard.extra_patterns]]
name = "stripe_key"
regex = "sk_live_[a-zA-Z0-9]{24,}"

[[security.outbound_leak_guard.extra_patterns]]
name = "internal_token"
regex = "int_[a-f0-9]{32}"
```

---

## LLM Guardrails

The LLM pipeline includes composable guardrails that are **enabled in audit mode by default**:

### PII Redaction Guard

Detects and optionally redacts:
- **Email addresses** -> `[EMAIL_REDACTED]`
- **US phone numbers** -> `[PHONE_REDACTED]`
- **Social Security Numbers** -> `[SSN_REDACTED]`
- **API key patterns** (`sk-*`, `AKIA*`, `ghp_*`) -> `[API_KEY_REDACTED]`

### Prompt Injection Guard

Detects 9+ common injection patterns:
- "ignore all previous instructions"
- "you are now [DAN/jailbreak/unrestricted]"
- "new system prompt:", "override system prompt"
- "forget all rules/instructions"
- "pretend you have no restrictions"

### Unicode Injection Guard

Detects invisible Unicode characters commonly used for steganographic prompt injection:
- **Zero-width characters**: ZWSP, ZWNJ, ZWJ, BOM, word joiner
- **Bidirectional overrides**: LTR/RTL marks, embedding, isolation (can reorder visible text)
- **Tag characters**: U+E0001..U+E007F (Unicode steganography)
- **Invisible separators**: soft hyphen, combining grapheme joiner, Mongolian vowel separator, function application, invisible times/separator/plus
- **Annotation anchors**: interlinear annotation markers

In sanitize mode, all suspicious characters are stripped. This guard is applied to both user input and context file content before inclusion in system prompts.

### Context File Scanning

Before any file is included in the system prompt (`.agentzero.md`, project context files, loaded references), its content is scanned through both the prompt injection guard and the Unicode injection guard. This catches:
- Prompt injection hidden inside project documentation
- Invisible Unicode steganography in source files
- Bidirectional text overrides that reorder visible content

Use `scan_for_injection(content)` programmatically, or rely on the automatic scanning in the guardrails pipeline.

### Enforcement Modes

Configure via `[guardrails]` in `agentzero.toml`:

```toml
[guardrails]
pii_mode = "audit"       # "off", "audit", "sanitize", "block"
injection_mode = "audit"  # "off", "audit", "sanitize", "block"
unicode_mode = "sanitize" # "off", "audit", "sanitize", "block"
```

| Mode | Behavior |
|---|---|
| `off` | Disabled |
| `audit` | Log violations, pass through (default) |
| `sanitize` | Redact content, continue |
| `block` | Reject the request entirely |

---

## WASM Plugin Sandbox

Plugins execute in a strict sandbox with configurable isolation:

| Control | Default |
|---|---|
| Max execution time | 30 seconds |
| Max module size | 5 MB |
| Memory cap | 256 MB |
| Network access | **Denied** |
| Filesystem write | **Denied** |
| Filesystem read | **Denied** |
| Signature required | **Yes** (release builds) |

### Plugin Signing

In release builds, `require_signed` defaults to `true`. Unsigned WASM modules are rejected. Plugin manifests are signed with Ed25519 and verified before loading.

Use `--allow-unsigned-plugins` for local development.

---

## MCP Server Attestation

MCP servers are spawned as subprocesses. To prevent supply-chain attacks, configure a SHA-256 hash for each server binary:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem"],
      "sha256": "a1b2c3d4e5f6..."
    }
  }
}
```

Before spawning, AgentZero:
1. Resolves the binary path (absolute or via `PATH`)
2. Reads the binary and computes SHA-256
3. Compares against the configured hash
4. **Refuses to spawn** on mismatch

---

## Autonomy & Delegation

### Three Autonomy Levels

| Level | Read Tools | Write Tools | Network Tools |
|---|---|---|---|
| `ReadOnly` | Auto-approve | **Blocked** | **Blocked** |
| `Supervised` | Auto-approve | Requires approval | Requires approval |
| `Full` | Auto-approve | Auto-approve | Auto-approve |

### Sub-Agent Privilege Boundaries

When delegating to sub-agents, policies are **intersected** (most restrictive wins):
- Autonomy level: most restrictive of parent and child
- Forbidden paths: union (more paths forbidden)
- Allowed roots: intersection (only roots allowed by both)
- Sensitive file access: only if both parent and child allow
- Per-tool rate limits: the lower limit wins

A sub-agent can **never** escalate beyond its parent's privileges.

### Per-Tool Rate Limits

Configure rate limits per tool in the autonomy policy to prevent resource-intensive tools from being called excessively:

```toml
[autonomy.tool_rate_limits.shell]
max_calls = 10
window_secs = 60

[autonomy.tool_rate_limits.http_request]
max_calls = 30
window_secs = 60
```

---

## Declarative Security Policies

Create `.agentzero/security-policy.yaml` for fine-grained per-tool rules:

```yaml
default: deny
rules:
  - tool: http_request
    egress:
      - api.openai.com
      - "*.github.com"
    action: allow

  - tool: shell
    commands: [git, cargo, rustc]
    action: allow

  - tool: read_file
    filesystem: [/workspace, /tmp]
    action: allow

  - tool: "mcp:*"
    egress: [prompt]
    action: prompt
```

Paths in filesystem rules are **canonicalized** before comparison, preventing traversal attacks at the policy layer.

---

## Audit & Redaction

### Automatic Secret Redaction

Secrets are automatically stripped from errors, logs, and panic output:

| Pattern | Replacement |
|---|---|
| `OPENAI_API_KEY=sk-*` | `OPENAI_API_KEY=[REDACTED]` |
| `"api_key": "..."` | `"api_key":"[REDACTED]"` |
| `Authorization: Bearer ...` | `Authorization: Bearer [REDACTED]` |
| `sk-[A-Za-z0-9_-]{10,}` | `sk-[REDACTED]` |

The **panic hook** ensures secrets are redacted even during crashes. The **error chain walker** redacts every level of an `anyhow` error chain.

### Path Sanitization

Error messages returned to the LLM replace the user's home directory with `~`, preventing leakage of filesystem layout (e.g., `/home/alice/secret-project/` becomes `~/secret-project/`).

### Structured Audit Events

Security events are logged as structured tracing with `target: "audit"`:

- `auth_failure` — failed authentication attempt
- `scope_denied` — authenticated but insufficient scope
- `pair_success` / `pair_failure` — pairing flow events
- `api_key_created` / `api_key_revoked` — key lifecycle
- `rate_limited` — request rejected by rate limiter
- `estop` — emergency stop triggered

These integrate with any log aggregation system (ELK, Datadog, Grafana Loki) via JSON/JSONL subscriber configuration.

---

## Privacy Transport

For sensitive deployments, AgentZero supports end-to-end encrypted communication:

- **Noise protocol** key exchange with X25519
- **Session management** with configurable TTL and max concurrent sessions
- **Key rotation** with overlap periods for graceful rollover
- **Sealed envelope relay** with timing jitter to obscure message patterns

Configure via `[privacy]` in `agentzero.toml`:

```toml
[privacy]
mode = "encrypted"  # "off", "private", "encrypted", "full"
```

---

## Configuration Quick Reference

```toml
# Autonomy
[autonomy]
level = "supervised"  # read_only, supervised, full

# Guardrails (default: audit)
[guardrails]
pii_mode = "sanitize"
injection_mode = "block"

# Gateway TLS + mTLS
[gateway.tls]
cert_path = "/etc/ssl/cert.pem"
key_path = "/etc/ssl/key.pem"
client_ca_path = "/etc/ssl/client-ca.pem"

# WebSocket tuning
[gateway.websocket]
heartbeat_interval_secs = 30
pong_timeout_secs = 60
idle_timeout_secs = 300
max_message_bytes = 2097152

# Leak guard with custom patterns
[security.outbound_leak_guard]
enabled = true
action = "redact"
sensitivity = 0.7
extra_patterns = [
  { name = "stripe_key", regex = "sk_live_[a-zA-Z0-9]{24,}" },
]
```

## See Also

- [Security Boundaries](/security/boundaries/) — Defense-in-depth boundary layers
- [Threat Model](/security/threat-model/) — Security threat analysis and risk tiers
- [Dependency Policy](/security/dependency-policy/) — Third-party dependency standards
- [Config Reference](/config/reference/) — Full annotated `agentzero.toml`
