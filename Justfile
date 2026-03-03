# AgentZero task runner

# Default recipe - show help
default:
    @just --list

# ── Docs ──────────────────────────────────────────

# Install site dependencies
docs-install:
    cd site && npm install

# Run site dev server
docs-dev:
    cd site && npm run dev

# Build site for production
docs-build:
    cd site && npm run build

# Preview production build locally
docs-preview:
    cd site && npm run preview

# Lint markdown files
docs-lint:
    npx markdownlint-cli2 "site/src/content/**/*.md" "README.md" "AGENTS.md" --config .markdownlint-cli2.yaml

# -- Test ─────────────────────────────────────────

# Run tests
test:
    cargo nextest run --workspace

# Run tests with verbose output
test-verbose:
    cargo nextest run --workspace --no-capture

# Run benchmarks
bench:
    cargo bench --workspace

# Run clippy lints
lint:
    cargo clippy -- -D warnings

# Format code
fmt:
    cargo fmt

# Check formatting
fmt-check:
    cargo fmt --check

# Full CI check
ci: fmt-check lint test


# ── Release ───────────────────────────────────────

# Cut a release: just release 0.3.0
release VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "==> Releasing v{{VERSION}}"
    # 1. Bump workspace version in root Cargo.toml
    sed -i '' 's/^version = ".*"/version = "{{VERSION}}"/' Cargo.toml
    # Bump inline version on internal workspace dep entries (path = "crates/…", version = "…")
    perl -i -pe 's|(agentzero-[a-z-]+ = \{ path = "crates/[^"]+", version = )"[^"]+"|${1}"{{VERSION}}"|g' Cargo.toml
    cargo check --workspace --quiet
    echo "    Cargo.toml [workspace.package] version set to {{VERSION}}"
    # 2. Commit the version bump (if anything changed)
    if ! git diff --quiet Cargo.toml Cargo.lock; then
        git add Cargo.toml Cargo.lock
        git commit -m "chore: bump workspace version to {{VERSION}}"
    fi
    # 3. Quality gates
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo nextest run --workspace
    # 4. Verify changelog & crate versions match
    scripts/verify-release-version.sh --version "{{VERSION}}"
    # 5. Tag and push (triggers .github/workflows/release.yml)
    git tag "v{{VERSION}}"
    git push origin "v{{VERSION}}"
    echo "==> Tag v{{VERSION}} pushed. Release workflow will build and publish."
