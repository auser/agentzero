# AgentZero task runner

default:
    @just --list

# ── Docs ──────────────────────────────────────────

# Install site dependencies
docs-install:
    cd public && npm install

# Run site dev server
docs-dev:
    cd public && npm run dev

# Build site for production
docs-build:
    cd public && npm run build

# Preview production build locally
docs-preview:
    cd public && npm run preview

# Lint markdown files
docs-lint:
    npx markdownlint-cli2 "public/src/content/**/*.md" "README.md" "AGENTS.md" --config .markdownlint-cli2.yaml

# ── Release ───────────────────────────────────────

# Cut a release: just release 0.2.0
release VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "==> Releasing v{{VERSION}}"
    # 1. Quality gates
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    # 2. Verify changelog & crate versions match
    scripts/verify-release-version.sh --version "{{VERSION}}"
    # 3. Tag and push (triggers .github/workflows/release.yml)
    git tag "v{{VERSION}}"
    git push origin "v{{VERSION}}"
    echo "==> Tag v{{VERSION}} pushed. Release workflow will build and publish."
