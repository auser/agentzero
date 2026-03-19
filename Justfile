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

# Run tests (compile with progress, then run)
test:
    cargo nextest run --workspace --exclude agentzero-ffi --exclude agentzero-plugin-sdk

# Run tests with verbose output
test-verbose:
    cargo nextest run --workspace --exclude agentzero-ffi --exclude agentzero-plugin-sdk --no-capture

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

# ── Platform Control UI ──────────────────────────

# Install platform UI dependencies
ui-install:
    cd ui && pnpm install

# Build platform UI (outputs to ui/dist/)
ui-build:
    cd ui && pnpm run build

# Dev mode: Vite dev server with proxy to gateway (http://localhost:5173)
ui-dev:
    cd ui && pnpm run dev

# Run Playwright e2e tests (requires daemon + ui-dev running)
ui-test:
    cd ui && pnpm test:e2e

# Run Playwright e2e tests with visible browser
ui-test-headed:
    cd ui && pnpm test:e2e:headed

# Full release build: UI first, then Rust with embedded-ui feature
build-full: ui-build
    cargo build --release --features embedded-ui

# ── Config UI ────────────────────────────────────

# Launch the visual node graph config editor (browser)
config-ui:
    cargo run -p agentzero --features config-ui -- config-ui

# Build the config UI frontend (for embedding)
config-ui-build:
    cd crates/agentzero-config-ui/ui && npm install && npm run build

# Dev mode: cargo watch for backend + vite dev for frontend (hot reload)
config-ui-dev:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Starting config UI dev servers..."
    echo "  Backend:  http://127.0.0.1:42618  (cargo watch)"
    echo "  Frontend: http://127.0.0.1:5173   (vite dev, proxies /api)"
    echo ""
    # Start backend with cargo watch, rebuilding on Rust changes
    cargo watch -w crates/agentzero-config-ui/src -x 'run -p agentzero --features config-ui -- config-ui' &
    CARGO_PID=$!
    # Wait for backend to be ready before starting frontend
    echo "Waiting for backend..."
    for i in $(seq 1 120); do
        if curl -s -o /dev/null -w '' http://127.0.0.1:42618/api/schema 2>/dev/null; then
            echo "Backend ready!"
            break
        fi
        if ! kill -0 $CARGO_PID 2>/dev/null; then
            echo "Backend process died"; exit 1
        fi
        sleep 1
    done
    # Start vite dev server for frontend hot reload
    cd crates/agentzero-config-ui/ui && npx vite --open &
    VITE_PID=$!
    trap "kill $CARGO_PID $VITE_PID 2>/dev/null" EXIT
    wait

# ── Docker ────────────────────────────────────────

# Build Docker image
docker-build:
    docker build -t agentzero:latest .

# Run Docker container (requires OPENAI_API_KEY in env)
docker-run:
    docker run -d \
      --name agentzero \
      -p 8080:8080 \
      -v agentzero-data:/data \
      -e OPENAI_API_KEY="${OPENAI_API_KEY}" \
      agentzero:latest

# Stop and remove Docker container
docker-stop:
    docker stop agentzero && docker rm agentzero

# Start via docker compose
docker-up:
    docker compose up -d

# Stop docker compose
docker-down:
    docker compose down

# ── Release ───────────────────────────────────────

# Preview changelog draft for next release (dry run, stdout only)
changelog VERSION:
    git-cliff --tag "v{{VERSION}}" --unreleased --strip header

# Bump every Cargo.toml version across the workspace: just bump-versions 0.4.0
bump-versions VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    # Root workspace manifest: [workspace.package] version and inline dep versions
    sed -i '' 's/^version = ".*"/version = "{{VERSION}}"/' Cargo.toml
    # Update existing workspace dep versions, then add version to any that lack it
    perl -i -pe 's|(version = )"[^"]+"|${1}"{{VERSION}}"| if /^agentzero-/' Cargo.toml
    perl -i -pe 's| \}$|, version = "{{VERSION}}" }| if /^agentzero-/ && !/version/' Cargo.toml
    echo "    Cargo.toml [workspace.package] → {{VERSION}}"
    # Standalone Cargo.toml files (plugins, fixtures) that don't use version.workspace
    rg --files -g 'Cargo.toml' | grep -v '^Cargo\.toml$' | while IFS= read -r f; do
        if grep -q '^version = ' "$f" && ! grep -q 'version\.workspace' "$f"; then
            sed -i '' 's/^version = ".*"/version = "{{VERSION}}"/' "$f"
            echo "    $f → {{VERSION}}"
        fi
    done

# Cut a release with automatic version bump (based on conventional commits)
release-auto:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "==> Preparing automatic release"
    # 1. Quality gates — auto-fix fmt and clippy, then test
    cargo fmt --all
    cargo clippy --fix --allow-dirty --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-targets -- -D warnings
    cargo nextest run --workspace --exclude agentzero-ffi --exclude agentzero-plugin-sdk
    # 2. Determine next version from conventional commits
    NEXT_VERSION=$(git-cliff --bumped-version | sed 's/^v//')
    echo "==> Auto-detected next version: $NEXT_VERSION"
    # 3. Bump all workspace versions
    just bump-versions "$NEXT_VERSION"
    # 4. Commit the version bump + any fmt/clippy fixes + updated Cargo.lock
    if ! git diff --quiet; then
        git add -u
        git commit -m "chore: bump workspace version to $NEXT_VERSION"
    fi
    # 5. Generate changelog entry from conventional commits (via git-cliff)
    if ! grep -q "^## \[$NEXT_VERSION\]" CHANGELOG.md; then
        git-cliff --tag "v$NEXT_VERSION" --unreleased --prepend CHANGELOG.md
        git add CHANGELOG.md
        git commit -m "chore: add changelog entry for v$NEXT_VERSION"
    fi
    # 6. Verify changelog & crate versions match
    scripts/verify-release-version.sh --version "$NEXT_VERSION"
    # 7. Push branch commits, tag, and push tag (triggers .github/workflows/release.yml)
    git push
    if ! git tag -l "v$NEXT_VERSION" | grep -q .; then
        git tag "v$NEXT_VERSION"
    fi
    git push origin "v$NEXT_VERSION"
    echo "==> Tag v$NEXT_VERSION pushed. Release workflow will build and publish."

# Cut a release with specific version: just release 0.4.0
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
    cargo nextest run --workspace --exclude agentzero-ffi --exclude agentzero-plugin-sdk
    # 3. Commit the version bump + any fmt/clippy fixes + updated Cargo.lock
    if ! git diff --quiet; then
        git add -u
        git commit -m "chore: bump workspace version to {{VERSION}}"
    fi
    # 4. Generate changelog entry from conventional commits (via git-cliff)
    if ! grep -q "^## \[{{VERSION}}\]" CHANGELOG.md; then
        git-cliff --tag "v{{VERSION}}" --unreleased --prepend CHANGELOG.md
        git add CHANGELOG.md
        git commit -m "chore: add changelog entry for v{{VERSION}}"
    fi
    # 5. Verify changelog & crate versions match
    scripts/verify-release-version.sh --version "{{VERSION}}"
    # 6. Push branch commits, tag, and push tag (triggers .github/workflows/release.yml)
    git push
    if ! git tag -l "v{{VERSION}}" | grep -q .; then
        git tag "v{{VERSION}}"
    fi
    git push origin "v{{VERSION}}"
    echo "==> Tag v{{VERSION}} pushed. Release workflow will build and publish."

# ── E2E Ollama Testing ──────────────────────────────────────────────

# Build with OpenTelemetry tracing support
build-otel:
    cargo build --release -p agentzero --features telemetry

# Run E2E tests against a local Ollama instance
test-ollama:
    cargo nextest run --run-ignored only -E 'test(ollama)' --test-threads 1
