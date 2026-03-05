#!/usr/bin/env bash
# Publish all workspace crates to crates.io in topological order.
# Used by the release workflow — assumes CARGO_REGISTRY_TOKEN is set.
#
# Post-consolidation workspace (16 members, 13 publishable).
# Skipped: agentzero-bench, agentzero-testkit, agentzero-ffi (publish = false)
set -euo pipefail

# Seconds to wait after each publish for crates.io index propagation.
PUBLISH_WAIT="${PUBLISH_WAIT:-20}"

publish() {
  local crate="$1"
  echo "==> Publishing ${crate}..."
  cargo publish -p "${crate}" --no-verify
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

# ── Tier 7: depends on nearly everything ──────────────────────────────────────
publish agentzero-cli          # -> core, auth, channels, config, infra, providers,
                               #    storage, tools, gateway, plugins

# ── Final: top-level binary ──────────────────────────────────────────────────
publish agentzero              # -> cli, core

echo "==> All crates published successfully."
