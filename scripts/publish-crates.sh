#!/usr/bin/env bash
# Publish publishable workspace crates to crates.io in topological order.
# Used by the release workflow — assumes CARGO_REGISTRY_TOKEN is set.
#
# All library sub-crates are published as internal implementation details.
# The top-level `agentzero` facade crate is the public API.
set -euo pipefail

# Seconds to wait after each publish for crates.io index propagation.
PUBLISH_WAIT="${PUBLISH_WAIT:-20}"
MAX_RETRIES="${MAX_RETRIES:-3}"

publish() {
  local crate="$1"
  local attempt=1
  echo "==> Publishing ${crate}..."
  while true; do
    local output
    if output=$(cargo publish -p "${crate}" --no-verify 2>&1); then
      break
    fi
    # Already published — not an error.
    if echo "$output" | grep -qE "already uploaded|already exists"; then
      echo "    Already published, skipping."
      return 0
    fi
    # Cannot publish — skip gracefully (publish = false).
    if echo "$output" | grep -qE "cannot be published"; then
      echo "    Not publishable, skipping."
      return 0
    fi
    if [[ $attempt -ge $MAX_RETRIES ]]; then
      echo "$output" >&2
      echo "    FAILED after ${MAX_RETRIES} attempts" >&2
      return 1
    fi
    local wait=$((60 * attempt))
    echo "    Rate-limited; retrying in ${wait}s (attempt ${attempt}/${MAX_RETRIES})..."
    sleep "${wait}"
    attempt=$((attempt + 1))
  done
  echo "    Waiting ${PUBLISH_WAIT}s for crates.io to index..."
  sleep "${PUBLISH_WAIT}"
}

# ── Tier 1: leaf crates (no internal deps) ───────────────────────────────────
publish agentzero-core
publish agentzero-plugin-sdk

# ── Tier 2: depend only on core ──────────────────────────────────────────────
publish agentzero-storage       # -> core
publish agentzero-providers     # -> core
publish agentzero-autopilot     # -> core
publish agentzero-plugins       # no internal deps

# ── Tier 3: depend on tier 1-2 ──────────────────────────────────────────────
publish agentzero-tools         # -> core, providers, storage
publish agentzero-auth          # -> storage

# ── Tier 4: depend on tier 1-3 ──────────────────────────────────────────────
publish agentzero-config        # -> core, tools
publish agentzero-channels      # -> core, config, storage

# ── Tier 5: orchestration layer ──────────────────────────────────────────────
publish agentzero-infra         # -> auth, config, core, tools, storage, providers
publish agentzero-orchestrator  # -> channels, config, core, infra, storage, tools

# ── Tier 6: server + UI ─────────────────────────────────────────────────────
publish agentzero-gateway       # -> config, core, infra, storage, channels, orchestrator, providers, tools
publish agentzero-config-ui     # -> config, core, orchestrator

# ── Tier 7: CLI ──────────────────────────────────────────────────────────────
publish agentzero-cli           # -> nearly everything

# ── Tier 8: facade ───────────────────────────────────────────────────────────
publish agentzero               # -> all of the above

echo "==> All crates published successfully."
