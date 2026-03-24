---
title: Running on Raspberry Pi
description: Install or compile AgentZero on Raspberry Pi 3, 4, 5 and other ARM single-board computers.
---

AgentZero provides pre-built ARM binaries and compiles natively on Raspberry Pi. It works well as a headless agent runtime on any Pi model with enough RAM.

## Compatibility

| Model | Architecture | Recommended Binary | Notes |
|-------|-------------|-------------------|-------|
| Pi 5 (4/8 GB) | aarch64 | `linux-aarch64` | Best performance |
| Pi 4 (64-bit OS) | aarch64 | `linux-aarch64` | Ensure 64-bit Raspberry Pi OS |
| Pi 4 (32-bit OS) | armv7 | `linux-armv7` | Legacy 32-bit |
| Pi 3 | armv7 | `linux-armv7` | CLI mode recommended |
| Pi Zero 2 W | aarch64 | `linux-aarch64` | Limited RAM (512 MB) |

## Option 1: Install Script (Recommended)

The install script auto-detects your architecture and prompts you to choose a variant:

```bash
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash
```

This detects `aarch64` (Pi 4/5 with 64-bit OS) or `armv7l` (Pi 3 or 32-bit OS) and downloads the correct binary.

For Raspberry Pi, the **lite** variant is recommended — it's a gateway-only binary (~3 MB) with privacy-first defaults:

```bash
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash -s -- --variant lite
```

| Variant | Use Case | Size |
|---------|----------|------|
| `lite` | Gateway-only, privacy-first (recommended for Pi) | ~3 MB |
| `minimal` | Full CLI with core tools | ~5 MB |
| `default` | Everything (TUI, plugins, gateway) | ~19 MB |

## Option 2: Download Pre-built Binary

Download directly from [GitHub Releases](https://github.com/auser/agentzero/releases):

```bash
# Pi 4/5 (64-bit):
curl -LO https://github.com/auser/agentzero/releases/latest/download/agentzero-linux-aarch64
chmod +x agentzero-linux-aarch64
sudo mv agentzero-linux-aarch64 /usr/local/bin/agentzero

# Pi 3 (32-bit):
curl -LO https://github.com/auser/agentzero/releases/latest/download/agentzero-linux-armv7
chmod +x agentzero-linux-armv7
sudo mv agentzero-linux-armv7 /usr/local/bin/agentzero
```

A statically-linked musl build is also available for minimal environments:

```bash
curl -LO https://github.com/auser/agentzero/releases/latest/download/agentzero-linux-aarch64-musl
```

Verify the installation:

```bash
agentzero --version
agentzero onboard
```

## Option 3: Build from Source on the Pi

### Prerequisites

```bash
# Raspberry Pi OS 64-bit (Bookworm) recommended
sudo apt update
sudo apt install -y build-essential git curl cmake pkg-config libssl-dev

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### Build

```bash
git clone https://github.com/auser/agentzero.git
cd agentzero
cargo build -p agentzero --release
```

The binary is at `target/release/agentzero`.

**Approximate build times:**

| Model | Build Time |
|-------|-----------|
| Pi 5 (8 GB) | ~5 minutes |
| Pi 4 (4 GB) | ~10-15 minutes |
| Pi 3 (1 GB) | ~30+ minutes |

### Minimal Build

For Pi 3 or other constrained devices, build with minimal features to reduce binary size and compile time:

```bash
cargo build -p agentzero --release --no-default-features --features memory-sqlite
```

This produces a smaller binary (~15 MB) with CLI + SQLite storage, skipping optional subsystems.

## Option 4: Cross-Compilation from Desktop

Build on your development machine and copy the binary to the Pi.

### For aarch64 (Pi 4/5)

```bash
# Install cross-compiler (Ubuntu/Debian)
sudo apt install gcc-aarch64-linux-gnu

# Add Rust target
rustup target add aarch64-unknown-linux-gnu

# Build
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
  cargo build -p agentzero --release --target aarch64-unknown-linux-gnu

# Copy to Pi
scp target/aarch64-unknown-linux-gnu/release/agentzero pi@raspberrypi.local:~/
```

### For armv7 (Pi 3)

```bash
# Install cross-compiler
sudo apt install gcc-arm-linux-gnueabihf

# Add Rust target
rustup target add armv7-unknown-linux-gnueabihf

# Build
CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc \
  cargo build -p agentzero --release --target armv7-unknown-linux-gnueabihf

# Copy to Pi
scp target/armv7-unknown-linux-gnueabihf/release/agentzero pi@raspberrypi.local:~/
```

### Static musl Build

For a fully static binary with no runtime dependencies:

```bash
pip3 install ziglang
cargo install cargo-zigbuild

cargo zigbuild -p agentzero --release --target aarch64-unknown-linux-musl
```

## Feature Flags

When building from source, optional features can be enabled:

| Feature | Description | Use Case |
|---------|-------------|----------|
| `memory-sqlite` | Local SQLite storage (default) | Always recommended |
| `hardware` | GPIO and peripheral support | Robot/IoT builds with GPIO, motors, sensors |
| `rag` | Retrieval-augmented generation | Document indexing and search |
| `channels-standard` | Extra communication channels | Telegram, Discord, Slack, etc. |
| `memory-turso` | Remote Turso database | Cloud-synced memory |

Example with hardware peripherals enabled:

```bash
cargo build -p agentzero --release --features hardware
```

## agentzero-lite: Privacy-First for Edge Devices

For Raspberry Pi deployments, `agentzero-lite` is the recommended binary. It's a lightweight gateway that defaults to `"private"` privacy mode — no CLI, no TUI, no plugins, just the gateway server.

```bash
# Install via the installer
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash -s -- --variant lite

# Or build from source (smaller, privacy-first defaults)
cargo build -p agentzero-lite --release

# Run with defaults (private mode, Noise encryption auto-enabled)
agentzero-lite --host 0.0.0.0 --port 8080

# Fully offline (blocks cloud providers too)
agentzero-lite --host 0.0.0.0 --port 8080 --privacy-mode local_only
```

See the [Privacy Guide](/guides/privacy/) for details on what each privacy mode does.

## Daemon and Service Setup

### Foreground Mode

```bash
agentzero gateway --host 0.0.0.0 --port 8080
```

### Systemd Service (Auto-start on Boot)

AgentZero can install itself as a systemd service:

```bash
agentzero service install
agentzero service start
agentzero service status
```

Or create the unit file manually:

```ini
# /etc/systemd/system/agentzero.service
[Unit]
Description=AgentZero Agent Runtime
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=pi
ExecStart=/usr/local/bin/agentzero daemon start --host 0.0.0.0 --port 8080
Restart=on-failure
RestartSec=5
Environment=AGENTZERO_DATA_DIR=/home/pi/.agentzero

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable agentzero
sudo systemctl start agentzero

# Check logs
journalctl -u agentzero -f
```

## Performance Tips

- **Use NVMe on Pi 5** — SD cards are significantly slower for both builds and runtime I/O
- **Active cooling** — required for sustained compilation; the Pi will thermal-throttle without it
- **Add swap for Pi 3** — 1 GB RAM is tight for compilation:
  ```bash
  sudo dphys-swapfile swapoff
  sudo sed -i 's/CONF_SWAPSIZE=.*/CONF_SWAPSIZE=2048/' /etc/dphys-swapfile
  sudo dphys-swapfile setup
  sudo dphys-swapfile swapon
  ```
- **Reduce parallel jobs on low-RAM devices:**
  ```bash
  cargo build -p agentzero --release -j2
  ```
- **8 GB Pi 5 is recommended** for building the full workspace from source; 4 GB works but is tight

## Troubleshooting

### Missing `libssl-dev`

```
error: failed to run custom build command for `openssl-sys`
```

Fix: `sudo apt install libssl-dev`

### Out of memory during build

Symptoms: compiler killed by OOM, `signal: 9 (SIGKILL)`.

Fix: Add swap (see Performance Tips above) and reduce parallel jobs with `-j2`.

### GPIO permissions

```
Permission denied (os error 13)
```

Fix: Add your user to the `gpio` and `dialout` groups:

```bash
sudo usermod -aG gpio,dialout $USER
# Log out and back in for group changes to take effect
```

### Serial port access (for peripherals)

```bash
sudo usermod -aG dialout $USER
```

## Next Steps

- [Quick Start](/quickstart/) — Set up config and run your first agent message
- [Config Reference](/config/reference/) — Full annotated configuration file
- [Gateway Deployment](/guides/deployment/) — Reverse proxy and Docker setup
- [Provider Setup](/guides/providers/) — Configure LLM providers
