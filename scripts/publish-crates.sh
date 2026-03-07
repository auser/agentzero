#!/usr/bin/env bash
# Publish publishable workspace crates to crates.io in topological order.
# Used by the release workflow — assumes CARGO_REGISTRY_TOKEN is set.
#
# Most crates have publish = false (distributed via GitHub Releases only).
# Only library crates intended for external consumption are published here.
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

# ── Tier 1: leaf crate (no internal deps) ────────────────────────────────────
publish agentzero-core

# ── Tier 2: depend on core ───────────────────────────────────────────────────
publish agentzero-plugin-sdk   # -> core

echo "==> All crates published successfully."
