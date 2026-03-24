---
title: Running on Android
description: Install or compile AgentZero for Android devices via Termux, ADB, or cross-compilation.
---

AgentZero runs as a CLI binary on Android — it is not a native Android app. The recommended environment is [Termux](https://termux.dev/), a terminal emulator that provides a full Linux environment on your phone or tablet.

## Supported Architectures

| Target | Android Version | Devices |
|--------|-----------------|---------|
| `aarch64` (ARM64) | Android 5.0+ (API 21+) | Modern 64-bit phones and tablets |
| `armv7` (32-bit ARM) | Android 4.1+ (API 16+) | Older 32-bit devices |

## Option 1: Pre-built Binary via Termux

### 1. Install Termux

Download from [F-Droid](https://f-droid.org/packages/com.termux/) (recommended) or GitHub releases.

:::caution
The Google Play Store version of Termux is outdated and unsupported. Use F-Droid.
:::

### 2. Detect Your Architecture

```bash
uname -m
# aarch64 = 64-bit (most modern phones)
# armv7l or armv8l = 32-bit
```

### 3. Download and Install

```bash
# For 64-bit (aarch64) — most devices:
curl -LO https://github.com/auser/agentzero/releases/latest/download/agentzero-linux-aarch64
chmod +x agentzero-linux-aarch64
mv agentzero-linux-aarch64 $PREFIX/bin/agentzero

# For 32-bit (armv7):
curl -LO https://github.com/auser/agentzero/releases/latest/download/agentzero-linux-armv7
chmod +x agentzero-linux-armv7
mv agentzero-linux-armv7 $PREFIX/bin/agentzero
```

### 4. Verify and Set Up

```bash
agentzero --version
agentzero onboard
```

## Option 2: Install Script

The install script auto-detects your architecture:

```bash
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash
```

:::note
The `armv7` architecture may not have a pre-built binary for every release. If the script cannot find one, use `--from-source` to build locally.
:::

## Option 3: Build from Source in Termux

For the latest code or when pre-built binaries are unavailable:

```bash
# Install build dependencies
pkg update && pkg install rust git

# Clone and build
git clone https://github.com/auser/agentzero.git
cd agentzero
cargo build -p agentzero --release

# Install
cp target/release/agentzero $PREFIX/bin/
```

### Minimal Build

For constrained devices with limited storage or RAM, build with minimal features:

```bash
cargo build -p agentzero --release --no-default-features --features memory-sqlite
```

This produces a smaller binary (~15 MB) with just CLI + local SQLite storage, skipping optional subsystems like RAG, hardware peripherals, and extra communication channels.

## Option 4: Cross-Compilation from Desktop

Build on your development machine and push to the device.

### Prerequisites

- [Android NDK](https://developer.android.com/ndk/downloads)
- Rust with Android targets

### Setup

```bash
# Add Android targets
rustup target add aarch64-linux-android armv7-linux-androideabi

# Set NDK path
export ANDROID_NDK_HOME=/path/to/android-ndk
export PATH=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH
```

### Build

```bash
# 64-bit (aarch64)
cargo build -p agentzero --release --target aarch64-linux-android

# 32-bit (armv7)
cargo build -p agentzero --release --target armv7-linux-androideabi
```

### Deploy via ADB

```bash
adb push target/aarch64-linux-android/release/agentzero /data/local/tmp/
adb shell chmod +x /data/local/tmp/agentzero
adb shell /data/local/tmp/agentzero --version
```

:::caution
Running outside Termux requires a rooted device or specific permissions for full functionality.
:::

## Feature Flags

When building from source, optional features can be enabled or disabled:

| Feature | Description | Recommended on Android |
|---------|-------------|----------------------|
| `memory-sqlite` | Local SQLite storage (default) | Yes |
| `hardware` | GPIO and peripheral support | No (not available) |
| `rag` | Retrieval-augmented generation | Optional |
| `channels-standard` | Extra communication channels | Optional |
| `memory-turso` | Remote Turso database | Optional (requires network) |

## Platform Limitations

- **No systemd:** Use Termux's `termux-services` package for daemon mode, or run in the foreground
- **Storage access:** Run `termux-setup-storage` to access shared storage (photos, downloads, etc.)
- **Network binding:** Some features may require Android VPN permission for local port binding
- **No hardware peripherals:** GPIO and peripheral features are not available on Android

## Running the Gateway

Start the gateway in foreground mode:

```bash
agentzero gateway --host 127.0.0.1 --port 8080
```

For auto-start on boot, install `termux-boot`:

```bash
pkg install termux-boot
mkdir -p ~/.termux/boot
cat > ~/.termux/boot/agentzero.sh << 'EOF'
#!/data/data/com.termux/files/usr/bin/sh
termux-wake-lock
agentzero daemon start --host 127.0.0.1 --port 8080
EOF
chmod +x ~/.termux/boot/agentzero.sh
```

## Troubleshooting

### "Permission denied"

```bash
chmod +x agentzero
```

### "not found" or wrong binary

Make sure you downloaded the correct architecture for your device. Check with `uname -m`.

### Linker errors during cross-compilation

Verify that `ANDROID_NDK_HOME` is set correctly and the NDK toolchain binaries are on your `PATH`.

### Out of memory during source build

Termux runs within Android's memory limits. Try reducing parallel jobs:

```bash
cargo build -p agentzero --release -j2
```

## Next Steps

- [Quick Start](/quickstart/) — Set up config and run your first agent message
- [Config Reference](/config/reference/) — Full annotated configuration file
- [Gateway Deployment](/guides/deployment/) — Reverse proxy and Docker setup
