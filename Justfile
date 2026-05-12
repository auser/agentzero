set dotenv-load := true
set shell := ["sh", "-cu"]

# Default target
default:
  @just --list

# List all targets
list:
  @just --list

# Check documentation
docs-check:
  python3 scripts/docs_check.py

# Check ADRs
adr-check:
  python3 scripts/adr_check.py

# Check the project
check:
  cargo check --workspace

# Run tests
test:
  cargo test --workspace

# Run clippy
clippy:
  cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run formatter
fmt:
  cargo fmt --all -- --check

# Run CI checks
ci: docs-check adr-check check test clippy fmt

# Build the docs site
docs-build:
  cd site && pnpm install && pnpm run build

# Start docs dev server
docs-dev:
  cd site && pnpm install && pnpm run dev

# Preview built docs
docs-preview:
  cd site && pnpm run preview

# Release a specific version: just release 0.2.0
release version:
  #!/usr/bin/env sh
  set -eu
  VERSION="{{version}}"
  TAG="v${VERSION}"
  echo "Releasing ${TAG}..."
  # Ensure clean tree
  if [ -n "$(git status --porcelain)" ]; then
    echo "error: working tree is dirty — commit or stash first" >&2
    exit 1
  fi
  # Run CI checks
  just ci
  # Generate changelog
  git-cliff --tag "${TAG}" -o CHANGELOG.md
  # Update version in root Cargo.toml
  sed -i '' "s/^version = \".*\"/version = \"${VERSION}\"/" Cargo.toml
  # Commit changelog + version bump
  git add CHANGELOG.md Cargo.toml Cargo.lock
  git commit -m "chore: release ${TAG}"
  # Tag
  git tag -a "${TAG}" -m "$(git-cliff --tag "${TAG}" --unreleased --strip header)"
  echo ""
  echo "Tagged ${TAG}. Push with:"
  echo "  git push origin main ${TAG}"

# Release with auto-detected version from conventional commits
release-auto:
  #!/usr/bin/env sh
  set -eu
  VERSION="$(git-cliff --bumped-version | sed 's/^v//')"
  echo "Auto-detected next version: ${VERSION}"
  just release "${VERSION}"

# Generate changelog without releasing
changelog:
  git-cliff -o CHANGELOG.md

# Preview unreleased changelog
changelog-preview:
  git-cliff --unreleased --strip header

# Build release binary and symlink to ~/.bin/agentzero
install:
  #!/usr/bin/env sh
  set -eu
  cargo build --release --features wasm,rag
  mkdir -p "$HOME/.bin"
  ln -sf "$(pwd)/target/release/az" "$HOME/.bin/az"
  echo "Installed: ~/.bin/az → $(pwd)/target/release/az"

# Build a WASM plugin: just build-plugin brain
build-plugin name:
  #!/usr/bin/env sh
  set -eu
  MANIFEST="plugins/{{name}}/Cargo.toml"
  if [ ! -f "$MANIFEST" ]; then
    echo "error: no plugin at plugins/{{name}}/" >&2
    exit 1
  fi
  echo "Building plugin {{name}} for wasm32-unknown-unknown..."
  cargo build --manifest-path "$MANIFEST" --target wasm32-unknown-unknown --release
  WASM="plugins/{{name}}/target/wasm32-unknown-unknown/release/agentzero_{{name}}_wasm.wasm"
  if [ ! -f "$WASM" ]; then
    echo "error: expected output at $WASM" >&2
    exit 1
  fi
  SIZE=$(wc -c < "$WASM" | tr -d ' ')
  echo "Built: $WASM (${SIZE} bytes)"
  # Compute checksum
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$WASM" | tee "plugins/{{name}}/target/{{name}}.wasm.sha256"
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$WASM" | tee "plugins/{{name}}/target/{{name}}.wasm.sha256"
  fi

# Install a built plugin into .agentzero/plugins/
install-plugin name:
  #!/usr/bin/env sh
  set -eu
  WASM="plugins/{{name}}/target/wasm32-unknown-unknown/release/agentzero_{{name}}_wasm.wasm"
  MANIFEST="plugins/{{name}}/PLUGIN.toml"
  if [ ! -f "$WASM" ]; then
    echo "error: plugin not built. Run: just build-plugin {{name}}" >&2
    exit 1
  fi
  if [ ! -f "$MANIFEST" ]; then
    echo "error: no PLUGIN.toml at plugins/{{name}}/" >&2
    exit 1
  fi
  DEST=".agentzero/plugins/{{name}}"
  mkdir -p "$DEST"
  cp "$MANIFEST" "$DEST/PLUGIN.toml"
  cp "$WASM" "$DEST/{{name}}.wasm"
  echo "Installed plugin {{name}} to $DEST/"

# Show tree of the project
show-tree:
  find . -maxdepth 4 -type f | sort
