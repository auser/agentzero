---
title: Container Sandbox Mode
description: Network and filesystem isolation for AgentZero using Docker containers and iptables-based egress control.
---

## What Sandboxing Provides

Container Sandbox Mode runs AgentZero inside a hardened Docker container that enforces two layers of isolation:

**Network isolation** -- The container's OUTPUT chain defaults to DROP. Only DNS resolution and explicitly allowed domains (from your `security-policy.yaml`) are permitted. This prevents the agent from exfiltrating data or reaching unauthorized services, even if a tool is compromised.

**Filesystem isolation** -- The host workspace is mounted read-only at `/workspace`. The container root filesystem is also read-only. Only `/tmp` (64 MB tmpfs) and `/sandbox` (256 MB tmpfs) are writable, and both are `noexec`. The agent cannot modify your source code or persist data outside the container.

Additional hardening:
- All Linux capabilities are dropped except `NET_ADMIN` (required for iptables during startup, before the process drops to the non-root `agentzero` user).
- Memory is capped at 512 MB, CPU at 1 core.
- The container runs as uid 1000 (`agentzero`), never as root after initialization.

## Quickstart

1. Create a security policy in your project:

```yaml
# .agentzero/security-policy.yaml
default: deny
rules:
  - tool: http_request
    egress:
      - api.openai.com
      - api.anthropic.com
    action: allow
  - tool: shell
    commands: [git, cargo, rustc]
    action: allow
  - tool: read_file
    filesystem: [/workspace]
    action: allow
```

1. Build the sandbox image (one-time):

```bash
docker build -f docker/sandbox/Dockerfile -t agentzero-sandbox:latest .
```

1. Start the sandbox:

```bash
agentzero sandbox start --detach
```

1. Check status:

```bash
agentzero sandbox status
```

1. Debug with a shell:

```bash
agentzero sandbox shell
```

1. Stop:

```bash
agentzero sandbox stop
```

## Architecture: YAML to iptables Flow

```
security-policy.yaml
        |
        v
  policy-to-iptables.py    (runs at container startup as root)
        |
        v
  iptables rules:
    - Flush OUTPUT chain
    - ACCEPT loopback
    - ACCEPT ESTABLISHED,RELATED
    - ACCEPT DNS (UDP/TCP 53)
    - ACCEPT resolved IPs for each allowed domain
    - Default policy: DROP OUTPUT
        |
        v
  Drop to non-root user (agentzero, uid 1000)
        |
        v
  exec agentzero gateway
```

The entrypoint script (`sandbox-entrypoint.sh`) orchestrates this flow. It reads the policy YAML from either `/workspace/.agentzero/security-policy.yaml` or `/data/security-policy.yaml`, invokes the Python converter to generate iptables rules, applies them, then drops privileges and execs the gateway binary.

Domain names are resolved to IP addresses at container startup. If a domain has multiple A records, all are allowed. Wildcard entries (`*.example.com`) in the YAML are skipped for iptables (they are enforced at the application layer by the AgentZero security policy engine).

## CLI Commands

| Command | Description |
| ------- | ----------- |
| `agentzero sandbox start` | Build/pull image, validate policy, start container |
| `agentzero sandbox stop` | Stop and remove the sandbox container |
| `agentzero sandbox status` | Show container state and applied policy |
| `agentzero sandbox shell` | Open a debug shell inside the running container |

Options for `start`:
- `--image <name>` -- Override the Docker image (default: `agentzero-sandbox:latest`)
- `--port <port>` -- Host port for the gateway (default: 8080)
- `--policy <path>` -- Explicit path to security-policy.yaml
- `--detach` / `-d` -- Run in background

## Comparison with NVIDIA NeMo Guardrails / OpenShell

| Feature | AgentZero Sandbox | NVIDIA OpenShell |
| ------- | ----------------- | ---------------- |
| Network isolation | iptables DROP + allowlist | gVisor / network namespace |
| Policy format | YAML (`security-policy.yaml`) | Python config |
| Filesystem | Read-only root + tmpfs | gVisor VFS layer |
| Runtime overhead | Minimal (native iptables) | Higher (syscall interposition) |
| Dependency | Docker only | gVisor + Docker/Kubernetes |
| Per-tool granularity | Yes (egress per tool) | No (global network policy) |
| Setup complexity | Single YAML file | Python scripts + config files |

AgentZero's approach trades the deeper syscall-level isolation of gVisor for simplicity and lower overhead. For most agent workloads where the primary threat is unauthorized network egress, iptables-based isolation provides equivalent protection with a much simpler operational model.
