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


# ── FFI ──────────────────────────────────────────

# Generate all FFI bindings (Swift, Kotlin, Python)
ffi: ffi-swift ffi-kotlin ffi-python
    @echo "All FFI bindings generated in crates/agentzero-ffi/bindings/"

# Generate Swift bindings
ffi-swift:
    cargo build -p agentzero-ffi --release
    cargo run -p agentzero-ffi --features uniffi-cli --bin uniffi-bindgen generate \
        --library target/release/libagentzero_ffi.dylib \
        --language swift \
        --out-dir crates/agentzero-ffi/bindings/swift

# Generate Kotlin bindings
ffi-kotlin:
    cargo build -p agentzero-ffi --release
    cargo run -p agentzero-ffi --features uniffi-cli --bin uniffi-bindgen generate \
        --library target/release/libagentzero_ffi.dylib \
        --language kotlin \
        --out-dir crates/agentzero-ffi/bindings/kotlin

# Generate Python bindings
ffi-python:
    cargo build -p agentzero-ffi --release
    cargo run -p agentzero-ffi --features uniffi-cli --bin uniffi-bindgen generate \
        --library target/release/libagentzero_ffi.dylib \
        --language python \
        --out-dir crates/agentzero-ffi/bindings/python

# Build Node.js native addon (TypeScript)
ffi-node:
    cd crates/agentzero-ffi && cargo build --release --no-default-features --features node


# ── Build Variants ────────────────────────────

# Build full release (all features, ~19MB)
build:
    cargo build --release

# Build minimal binary (sqlite only, ~5MB)
build-minimal:
    cargo build -p agentzero --profile release-min --no-default-features --features minimal

# Build server binary (plugins + gateway, no TUI, ~7MB)
build-server:
    cargo build -p agentzero --profile release-min --no-default-features --features memory-sqlite,plugins,gateway,tls-rustls

# Build with wasmtime JIT (full + JIT WASM, ~20MB)
build-jit:
    cargo build --release --features wasm-jit

# Build with privacy features (Noise Protocol, sealed envelopes, key rotation)
build-private:
    cargo build --release --features privacy

# Build with native TLS instead of rustls
build-native-tls:
    cargo build -p agentzero --profile release-min --no-default-features --features memory-sqlite,plugins,tls-native

# Show binary sizes for all variants
build-sizes:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Building all variants..."
    echo ""
    cargo build --release -q
    echo "  full (release):     $(du -h target/release/agentzero | cut -f1)"
    cargo build -p agentzero --profile release-min --no-default-features --features minimal -q
    echo "  minimal:            $(du -h target/release-min/agentzero | cut -f1)"
    cargo build -p agentzero --profile release-min --no-default-features --features memory-sqlite,plugins,gateway,tls-rustls -q
    echo "  server:             $(du -h target/release-min/agentzero | cut -f1)"
    cargo build -p agentzero --profile release-min --no-default-features --features memory-sqlite,plugins,tls-native -q
    echo "  plugins+native-tls: $(du -h target/release-min/agentzero | cut -f1)"

# ── Release ───────────────────────────────────────

# Bump every Cargo.toml version across the workspace: just bump-versions 0.4.0
bump-versions VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    # Root workspace manifest: [workspace.package] version and inline dep versions
    sed -i '' 's/^version = ".*"/version = "{{VERSION}}"/' Cargo.toml
    perl -i -pe 's|(agentzero-[a-z-]+ = \{ path = "crates/[^"]+", version = )"[^"]+"|${1}"{{VERSION}}"|g' Cargo.toml
    echo "    Cargo.toml [workspace.package] → {{VERSION}}"
    # Standalone Cargo.toml files (plugins, fixtures) that don't use version.workspace
    rg --files -g 'Cargo.toml' | grep -v '^Cargo\.toml$' | while IFS= read -r f; do
        if grep -q '^version = ' "$f" && ! grep -q 'version\.workspace' "$f"; then
            sed -i '' 's/^version = ".*"/version = "{{VERSION}}"/' "$f"
            echo "    $f → {{VERSION}}"
        fi
    done

# Cut a release: just release 0.4.0
release VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "==> Releasing v{{VERSION}}"
    # 1. Bump all versions
    just bump-versions {{VERSION}}
    # 2. Quality gates — auto-fix fmt and clippy, then test
    cargo fmt --all
    cargo clippy --fix --allow-dirty --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-targets -- -D warnings
    cargo nextest run --workspace
    # 3. Commit the version bump + any fmt/clippy fixes + updated Cargo.lock
    if ! git diff --quiet; then
        git add -u
        git commit -m "chore: bump workspace version to {{VERSION}}"
    fi
    # 4. Add changelog entry if not already present (moves [Unreleased] → [VERSION] - DATE)
    today=$(date +%Y-%m-%d)
    if ! grep -q "^## \[{{VERSION}}\]" CHANGELOG.md; then
        sed -i '' "s/^## \[Unreleased\]/## [Unreleased]\n\n## [{{VERSION}}] - $today/" CHANGELOG.md
        git add CHANGELOG.md
        git commit -m "chore: add changelog entry for v{{VERSION}}"
    fi
    # 5. Verify changelog & crate versions match
    scripts/verify-release-version.sh --version "{{VERSION}}"
    # 6. Push branch commits, tag, and push tag (triggers .github/workflows/release.yml)
    git push
    git tag "v{{VERSION}}"
    git push origin "v{{VERSION}}"
    echo "==> Tag v{{VERSION}} pushed. Release workflow will build and publish."
