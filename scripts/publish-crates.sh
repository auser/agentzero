#!/usr/bin/env bash
# Publish all workspace crates to crates.io in topological order.
# Used by the release workflow — assumes CARGO_REGISTRY_TOKEN is set.
#
# Post-consolidation workspace (16 members).
# Skipped: agentzero-bench, agentzero-ffi, agentzero-cli, agentzero (publish = false)
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
publish agentzero-plugins

# ── Tier 2: depend on core ────────────────────────────────────────────────────
publish agentzero-providers    # -> core
publish agentzero-storage      # -> core
publish agentzero-testkit      # -> core (test utility crate)

# ── Tier 3: depend on tier 2 ─────────────────────────────────────────────────
publish agentzero-tools        # -> core, providers, storage
publish agentzero-auth         # -> storage

# ── Tier 4: depend on tier 3 ─────────────────────────────────────────────────
publish agentzero-config       # -> core, tools

# ── Tier 5: depend on tier 4 ─────────────────────────────────────────────────
publish agentzero-channels     # -> core, config, storage
publish agentzero-infra        # -> core, auth, config, tools, storage, providers, plugins

# ── Tier 6: depend on tier 5 ─────────────────────────────────────────────────
publish agentzero-gateway      # -> core, config, infra, storage, channels

# agentzero-cli and agentzero (binary) have publish = false;
# they are distributed via GitHub Releases, not crates.io.

echo "==> All crates published successfully."
