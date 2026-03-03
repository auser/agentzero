---
title: Installation
description: How to install AgentZero — one-liner script, cargo install, or build from source.
---

## Quick Install

The recommended way to install AgentZero is with the install script. It automatically detects your platform and architecture, downloads the correct pre-built binary, and verifies its checksum.

```bash
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash
```

The script requires no external dependencies beyond `curl` (or `wget`) and standard Unix tools.

### Common Examples

```bash
# Install specific version
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash -s -- --version 0.2.0

# Install to /usr/local/bin (may prompt for sudo)
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash -s -- --dir /usr/local/bin

# Install with zsh completions
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash -s -- --completions zsh

# Force reinstall
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash -s -- --force

# Dry run — see what would happen without making changes
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash -s -- --dry-run --verbose

# Build from source (requires Rust 1.80+)
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash -s -- --from-source
```

## Install Options

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--version VERSION` | `-v` | Install specific version | `latest` |
| `--dir DIR` | `-d` | Install directory | `~/.local/bin` |
| `--channel CHANNEL` | `-c` | Release channel (stable, nightly) | `stable` |
| `--force` | `-f` | Force reinstall even if already installed | |
| `--quiet` | `-q` | Suppress non-essential output | |
| `--verbose` | `-V` | Enable debug output | |
| `--dry-run` | `-n` | Show what would happen without doing it | |
| `--no-color` | | Disable colored output | |
| `--no-verify` | | Skip SHA-256 checksum verification | |
| `--completions SHELL` | | Install shell completions (bash, zsh, fish) | |
| `--from-source` | | Build from source instead of downloading binary | |
| `--uninstall` | | Remove agentzero and its data | |
| `--help` | `-h` | Show help message | |

Short flags can be combined: `-fqV` is equivalent to `--force --quiet --verbose`.

### Environment Variables

| Variable | Description |
|----------|-------------|
| `AGENTZERO_INSTALL_DIR` | Override install directory |
| `AGENTZERO_VERSION` | Override version to install |
| `NO_COLOR` | Disable colored output ([standard](https://no-color.org)) |

## Supported Platforms

Pre-built binaries are provided for these platform/architecture combinations:

| OS | Architecture | Artifact Name |
|----|-------------|---------------|
| Linux | x86_64 | `agentzero-v*-linux-x86_64` |
| Linux | aarch64 (ARM64) | `agentzero-v*-linux-aarch64` |
| macOS | aarch64 (Apple Silicon) | `agentzero-v*-macos-aarch64` |
| macOS | x86_64 (Intel) | `agentzero-v*-macos-x86_64` |
| Windows | x86_64 | `agentzero-v*-windows-x86_64.exe` |

For other architectures (ARMv7, 32-bit x86), use `--from-source` to build locally. See the [Raspberry Pi guide](/agentzero/guides/raspberry-pi/) for detailed ARM instructions.

### Platform-Specific Guides

For detailed instructions on specific platforms:

- **[Android](/agentzero/guides/android/)** — Running in Termux, cross-compilation with Android NDK
- **[Raspberry Pi](/agentzero/guides/raspberry-pi/)** — Pre-built ARM binaries, building on-device, systemd service setup

## Install via Cargo

If you have Rust installed, you can install directly from crates.io:

```bash
cargo install agentzero
```

This builds and installs the `agentzero` binary to `~/.cargo/bin/`.

## Build from Source

```bash
git clone https://github.com/auser/agentzero.git
cd agentzero
cargo build --release
```

The binary is at `target/release/agentzero`.

### Prerequisites

- **Rust 1.80+** and Cargo (install via [rustup.rs](https://rustup.rs))
- An OpenAI-compatible API key (OpenRouter, OpenAI, Anthropic, or local provider)

## Shell Completions

Shell completions can be installed via the install script:

```bash
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash -s -- --completions zsh
```

Or generated manually after installation:

```bash
# Bash
agentzero completions --shell bash > ~/.local/share/bash-completion/completions/agentzero

# Zsh
agentzero completions --shell zsh > ~/.zfunc/_agentzero

# Fish
agentzero completions --shell fish > ~/.config/fish/completions/agentzero.fish

# PowerShell
agentzero completions --shell power-shell > $HOME/.config/powershell/agentzero.ps1
```

## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash -s -- --uninstall
```

This removes the binary and shell completions. You will be asked whether to also remove the `~/.agentzero` data directory (config, memory database, plugins).

## Verify Installation

```bash
agentzero --version
agentzero --help
```

## Feature Flags

Some functionality requires compile-time feature flags (only applies to source builds):

| Feature | Description |
|---|---|
| `hardware` | Hardware discovery and peripheral commands |
| `whatsapp-web` | WhatsApp Web channel support |

```bash
cargo build -p agentzero --release --features hardware
```

## Development Build

For development with debug assertions:

```bash
cargo build -p agentzero
```

Run commands without installing:

```bash
cargo run -p agentzero -- --help
cargo run -p agentzero -- status
```

## Next Steps

- [Quick Start](/agentzero/quickstart/) — Set up config and run your first agent message
- [Config Reference](/agentzero/config/reference/) — Full annotated configuration file
