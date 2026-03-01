---
title: Installation
description: How to install AgentZero from source or pre-built binaries.
---

## Prerequisites

- **Rust 1.80+** and Cargo (install via [rustup.rs](https://rustup.rs))
- An OpenAI-compatible API key (OpenRouter, OpenAI, Anthropic, or local provider)

## Build from Source

```bash
git clone https://github.com/auser/agentzero.git
cd agentzero
cargo build --release
```

The binary is at `target/release/agentzero`.

## Install via Cargo

```bash
cargo install agentzero
```

This builds and installs the `agentzero` binary to `~/.cargo/bin/`.

## Verify Installation

```bash
agentzero --version
agentzero --help
```

## Shell Completions

Generate shell completions for your preferred shell:

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

## Feature Flags

Some functionality requires compile-time feature flags:

| Feature | Description |
|---|---|
| `hardware` | Hardware discovery and peripheral commands |
| `whatsapp-web` | WhatsApp Web channel support |

```bash
cargo build -p agentzero --release --features hardware
```

## Next Steps

- [Quick Start](/agentzero/quickstart/) — Set up config and run your first agent message
- [Config Reference](/agentzero/config/reference/) — Full annotated configuration file
