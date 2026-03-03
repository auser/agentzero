#!/usr/bin/env bash
# Publish all workspace crates to crates.io in topological order.
# Used by the release workflow — assumes CARGO_REGISTRY_TOKEN is set.
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
publish agentzero-approval
publish agentzero-autonomy
publish agentzero-common
publish agentzero-coordination
publish agentzero-cost
publish agentzero-delegation
publish agentzero-goals
publish agentzero-hardware
publish agentzero-health
publish agentzero-identity
publish agentzero-integrations
publish agentzero-leak-guard
publish agentzero-multimodal
publish agentzero-plugins
publish agentzero-routing
publish agentzero-security

# ── Tier 2 ───────────────────────────────────────────────────────────────────
publish agentzero-crypto       # -> common
publish agentzero-storage      # -> common, crypto

publish agentzero-auth         # -> storage
publish agentzero-cron         # -> storage
publish agentzero-daemon       # -> storage
publish agentzero-heartbeat    # -> storage
publish agentzero-hooks        # -> storage
publish agentzero-peripherals  # -> storage
publish agentzero-rag          # -> storage
publish agentzero-service      # -> storage
publish agentzero-skills       # -> storage
publish agentzero-tunnel       # -> storage
publish agentzero-update       # -> storage

# ── Tier 3 ───────────────────────────────────────────────────────────────────
publish agentzero-core         # -> security
publish agentzero-memory       # -> core
publish agentzero-providers    # -> core

# ── Tier 4 ───────────────────────────────────────────────────────────────────
publish agentzero-tools        # -> autonomy, core, common, hardware, delegation,
                               #    providers, routing, cron, skills, storage
publish agentzero-local        # -> common, health

# ── Tier 5 ───────────────────────────────────────────────────────────────────
publish agentzero-config       # -> common, tools
publish agentzero-doctor       # -> config, providers, heartbeat, health
publish agentzero-channels     # -> config, leak-guard, security, storage

# ── Tier 6 ───────────────────────────────────────────────────────────────────
publish agentzero-gateway      # -> security, storage, channels
publish agentzero-infra        # -> core, delegation, routing, tools, memory,
                               #    providers, security

# ── Tier 7 ───────────────────────────────────────────────────────────────────
publish agentzero-runtime      # -> auth, common, config, core, delegation,
                               #    infra, memory, routing, providers

# ── Tier 8: agentzero-cli (depends on all of the above) ──────────────────────
publish agentzero-cli

# ── Final: top-level binary ───────────────────────────────────────────────────
publish agentzero

echo "==> All crates published successfully."
