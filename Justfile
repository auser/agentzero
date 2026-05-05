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

# Show tree of the project
show-tree:
  find . -maxdepth 4 -type f | sort
