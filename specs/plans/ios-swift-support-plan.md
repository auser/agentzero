# iOS Swift Support Plan via UniFFI

## Context

AgentZero (via the ZeroClaw externals) already has an Android bridge crate at
`externals/zeroclaw/clients/android-bridge/` using UniFFI v0.27 procedural macros.
The same UniFFI annotations that generate Kotlin bindings also generate Swift bindings
— no `.udl` files are needed. This plan covers the full roadmap for iOS support
using Swift, from shared bridge refactoring through CI/CD and App Store readiness.

The Android bridge currently defines these FFI-exported types via proc macros:
- `AgentStatus` (`#[uniffi::Enum]`)
- `ZeroClawConfig` (`#[uniffi::Record]`)
- `ChatMessage` (`#[uniffi::Record]`)
- `SendResult` (`#[uniffi::Record]`)
- `ZeroClawController` (`#[uniffi::Object]` with `#[uniffi::export]` methods)
- `ZeroClawError` (`#[uniffi::Error]`)

All method implementations are currently mock/stub. The `zeroclaw` crate dependency
is commented out in the bridge's `Cargo.toml`.

**Status**: PLANNED (2026-03-03). No implementation started.

---

## Phase 1: Shared Bridge Crate Refactoring

**Goal**: Extract FFI types into a platform-neutral crate that produces both `.so` (Android) and `.a` (iOS).

### 1.1 Create Shared Bridge Crate

Create `externals/zeroclaw/clients/bridge/` with:

```
bridge/
├── Cargo.toml
├── uniffi-bindgen.rs
└── src/
    └── lib.rs
```

**`Cargo.toml`:**
```toml
[package]
name = "zeroclaw-bridge"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "staticlib"]
name = "zeroclaw_bridge"

[dependencies]
zeroclaw = { path = "../.." }
uniffi = { version = "0.27" }
tokio = { version = "1", features = ["rt-multi-thread", "sync"] }
anyhow = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[[bin]]
name = "uniffi-bindgen"
path = "uniffi-bindgen.rs"
```

Key differences from `android-bridge`:
- `crate-type` includes both `cdylib` (Android .so) and `staticlib` (iOS .a)
- Library name is `zeroclaw_bridge` (platform-neutral, not `zeroclaw_android`)
- The `zeroclaw` dependency is **uncommented** and wired

### 1.2 Move Types from Android Bridge

Move all UniFFI-annotated types and the `uniffi::setup_scaffolding!()` call from
`android-bridge/src/lib.rs` into `bridge/src/lib.rs`. This includes:
- `AgentStatus`, `ZeroClawConfig`, `ChatMessage`, `SendResult`
- `ZeroClawController` with all `#[uniffi::export]` methods
- `ZeroClawError`
- Helper functions (`uuid_v4`, `current_timestamp_ms`, `runtime()`)

### 1.3 Wire Real ZeroClaw Dependency

Replace mock implementations in `ZeroClawController`:
- `start()` → actually start the gateway via zeroclaw runtime
- `stop()` → actually stop the gateway
- `send_message()` → forward to the agent and return real responses

Keep the `OnceLock<Runtime>` pattern for the global Tokio runtime — it works well
for both Android (JNI thread pool) and iOS (Swift async bridge).

### 1.4 Update Android Bridge

Convert `android-bridge/` to a thin wrapper that re-exports from the shared crate:

```toml
# android-bridge/Cargo.toml
[dependencies]
zeroclaw-bridge = { path = "../bridge" }
```

This ensures existing Android builds continue to work during the transition.

### 1.5 Tests

- Port the 3 existing tests from android-bridge
- Add compilation tests verifying both `cdylib` and `staticlib` targets build

---

## Phase 2: iOS Target Compilation

### 2.1 Required Rust Targets

| Target | Purpose | Command |
|--------|---------|---------|
| `aarch64-apple-ios` | Physical iOS devices (iPhone, iPad) | `rustup target add aarch64-apple-ios` |
| `aarch64-apple-ios-sim` | Simulator on Apple Silicon Macs | `rustup target add aarch64-apple-ios-sim` |
| `x86_64-apple-ios` | Simulator on Intel Macs | `rustup target add x86_64-apple-ios` |

### 2.2 Build Commands

```bash
# Device build
cargo build -p zeroclaw-bridge --release --target aarch64-apple-ios

# Simulator builds
cargo build -p zeroclaw-bridge --release --target aarch64-apple-ios-sim
cargo build -p zeroclaw-bridge --release --target x86_64-apple-ios
```

### 2.3 SDK Configuration

No Android NDK equivalent needed — Xcode provides the iOS SDK. Set `SDKROOT` if
building outside Xcode:

```bash
# Device
export SDKROOT=$(xcrun --sdk iphoneos --show-sdk-path)

# Simulator
export SDKROOT=$(xcrun --sdk iphonesimulator --show-sdk-path)
```

### 2.4 Cargo Config

Add to `externals/zeroclaw/.cargo/config.toml`:

```toml
[target.aarch64-apple-ios]
# No special linker needed — Xcode SDK handles this

[target.aarch64-apple-ios-sim]
# No special linker needed

[target.x86_64-apple-ios]
# No special linker needed
```

### 2.5 Feature Flag Considerations

- Disable `hardware` feature on iOS (no GPIO access)
- `memory-sqlite` works on iOS (bundled rusqlite compiles the C code)
- `reqwest` with `rustls-tls` works — avoid any OpenSSL dependency
- Consider a new `mobile` feature flag to gate out desktop-only functionality

---

## Phase 3: Swift Binding Generation

### 3.1 Generate Bindings

UniFFI proc macros generate Swift code via the `uniffi-bindgen` binary:

```bash
cargo run -p zeroclaw-bridge --bin uniffi-bindgen generate \
  --library target/aarch64-apple-ios/release/libzeroclaw_bridge.a \
  --language swift \
  --out-dir generated/swift
```

This produces:
- `zeroclaw_bridge.swift` — Swift types, protocols, and class wrappers
- `zeroclaw_bridgeFFI.h` — C header for the FFI layer
- `zeroclaw_bridgeFFI.modulemap` — Clang module map

### 3.2 Generated Type Mapping

| Rust Type | UniFFI Attribute | Swift Type |
|-----------|-----------------|------------|
| `AgentStatus` | `#[uniffi::Enum]` | `enum AgentStatus` |
| `ZeroClawConfig` | `#[uniffi::Record]` | `struct ZeroClawConfig` |
| `ChatMessage` | `#[uniffi::Record]` | `struct ChatMessage` |
| `SendResult` | `#[uniffi::Record]` | `struct SendResult` |
| `ZeroClawController` | `#[uniffi::Object]` | `class ZeroClawController` |
| `ZeroClawError` | `#[uniffi::Error]` | `enum ZeroClawError: Error` |

### 3.3 No `.udl` Files Required

The project uses UniFFI v0.27 procedural macros exclusively. The `uniffi-bindgen.rs`
binary calls `uniffi::uniffi_bindgen_main()` which introspects the compiled library
to generate bindings. No interface definition files needed.

---

## Phase 4: XCFramework Packaging

### 4.1 Build Script

Create `externals/zeroclaw/clients/ios/build-xcframework.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

CRATE="zeroclaw-bridge"
LIB="libzeroclaw_bridge"

# Build for all iOS targets
cargo build -p $CRATE --release --target aarch64-apple-ios
cargo build -p $CRATE --release --target aarch64-apple-ios-sim
cargo build -p $CRATE --release --target x86_64-apple-ios

# Create fat library for simulator (ARM64 + x86_64)
mkdir -p target/universal-ios-sim/release
lipo -create \
  target/aarch64-apple-ios-sim/release/${LIB}.a \
  target/x86_64-apple-ios/release/${LIB}.a \
  -output target/universal-ios-sim/release/${LIB}.a

# Generate Swift bindings
cargo run -p $CRATE --bin uniffi-bindgen generate \
  --library target/aarch64-apple-ios/release/${LIB}.a \
  --language swift \
  --out-dir generated/swift

# Create XCFramework
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/${LIB}.a \
  -headers generated/swift/ \
  -library target/universal-ios-sim/release/${LIB}.a \
  -headers generated/swift/ \
  -output ZeroClawBridge.xcframework

echo "XCFramework created: ZeroClawBridge.xcframework"
```

### 4.2 XCFramework Layout

```
ZeroClawBridge.xcframework/
├── ios-arm64/
│   ├── libzeroclaw_bridge.a
│   └── Headers/
│       ├── zeroclaw_bridgeFFI.h
│       └── zeroclaw_bridgeFFI.modulemap
├── ios-arm64_x86_64-simulator/
│   ├── libzeroclaw_bridge.a
│   └── Headers/
│       ├── zeroclaw_bridgeFFI.h
│       └── zeroclaw_bridgeFFI.modulemap
└── Info.plist
```

---

## Phase 5: Swift Package Manager Integration

### 5.1 Package.swift

Create `externals/zeroclaw/clients/ios/Package.swift`:

```swift
// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "ZeroClawBridge",
    platforms: [.iOS(.v16)],
    products: [
        .library(
            name: "ZeroClawBridge",
            targets: ["ZeroClawBridge", "ZeroClawBridgeFFI"]
        ),
    ],
    targets: [
        .target(
            name: "ZeroClawBridge",
            dependencies: ["ZeroClawBridgeFFI"],
            path: "Sources/ZeroClawBridge"
        ),
        .binaryTarget(
            name: "ZeroClawBridgeFFI",
            path: "ZeroClawBridge.xcframework"
        ),
    ]
)
```

### 5.2 Source Layout

```
externals/zeroclaw/clients/ios/
├── Package.swift
├── Sources/
│   └── ZeroClawBridge/
│       └── zeroclaw_bridge.swift      # Generated by uniffi-bindgen
├── ZeroClawBridge.xcframework/        # Built artifact
├── build-xcframework.sh
└── README.md
```

---

## Phase 6: Swift Client App

### 6.1 App Structure

Create `externals/zeroclaw/clients/ios-app/` as a SwiftUI project:

```
ios-app/
├── ZeroClaw.xcodeproj/
└── ZeroClaw/
    ├── ZeroClawApp.swift
    ├── ContentView.swift
    ├── Views/
    │   ├── ChatView.swift
    │   ├── SettingsView.swift
    │   └── StatusView.swift
    ├── Services/
    │   ├── AgentService.swift         # Wraps ZeroClawController
    │   └── NotificationService.swift
    ├── Models/
    │   └── AppState.swift
    └── Resources/
        ├── Assets.xcassets
        └── Info.plist
```

### 6.2 Minimum SwiftUI Integration

```swift
import ZeroClawBridge

@MainActor
class AgentService: ObservableObject {
    @Published var status: AgentStatus = .stopped
    @Published var messages: [ChatMessage] = []

    private let controller: ZeroClawController

    init(dataDir: String) {
        self.controller = ZeroClawController.withDefaults(dataDir: dataDir)
    }

    func start() throws {
        try controller.start()
        status = controller.getStatus()
    }

    func stop() throws {
        try controller.stop()
        status = controller.getStatus()
    }

    func sendMessage(_ content: String) -> SendResult {
        let result = controller.sendMessage(content: content)
        messages = controller.getMessages()
        return result
    }
}
```

### 6.3 iOS-Specific Considerations

- **Background execution**: Use `BGAppRefreshTask` for periodic agent checks
- **Notifications**: Local notifications for agent responses when app is backgrounded
- **Keychain**: Store API keys in iOS Keychain (not `UserDefaults`)
- **App Transport Security**: All API endpoints must use HTTPS
- **No daemon mode**: iOS apps cannot run persistent background processes; the app is a client that communicates with a remote gateway or runs the agent in-process during foreground time

---

## Phase 7: CI/CD

### 7.1 GitHub Actions Workflow

Create `.github/workflows/ios-build.yml`:

```yaml
name: iOS Build
on:
  push:
    paths:
      - 'externals/zeroclaw/clients/bridge/**'
      - 'externals/zeroclaw/clients/ios/**'
      - '.github/workflows/ios-build.yml'
  pull_request:
    paths:
      - 'externals/zeroclaw/clients/bridge/**'
      - 'externals/zeroclaw/clients/ios/**'

jobs:
  build-ios:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-apple-ios,aarch64-apple-ios-sim,x86_64-apple-ios
      - name: Build bridge for iOS
        run: |
          cargo build -p zeroclaw-bridge --release --target aarch64-apple-ios
          cargo build -p zeroclaw-bridge --release --target aarch64-apple-ios-sim
          cargo build -p zeroclaw-bridge --release --target x86_64-apple-ios
      - name: Generate Swift bindings
        run: |
          cargo run -p zeroclaw-bridge --bin uniffi-bindgen generate \
            --library target/aarch64-apple-ios/release/libzeroclaw_bridge.a \
            --language swift --out-dir generated/swift
      - name: Build XCFramework
        run: bash externals/zeroclaw/clients/ios/build-xcframework.sh
      - name: Build Swift package
        run: |
          cd externals/zeroclaw/clients/ios
          swift build
```

### 7.2 Release Integration

- Attach `ZeroClawBridge.xcframework.zip` to GitHub releases alongside existing binaries
- macOS runners cost ~2x Linux runners — run iOS builds only on:
  - Tagged releases
  - Changes to `externals/zeroclaw/clients/bridge/**` or `ios/**`
- Consider a separate release tag for SPM versioning

### 7.3 Signing

- CI builds for simulator testing do not require Apple Developer certificates
- Device builds and App Store distribution require a paid Apple Developer account ($99/year)
- Use Xcode Cloud or Fastlane for production signing

---

## Phase 8: Testing Strategy

### 8.1 Rust-Level Tests

- Unit tests in the shared bridge crate (port existing 3 tests + add new ones)
- Compilation tests: verify `staticlib` builds produce valid `.a` for all 3 iOS targets
- Run in CI on every push to bridge paths

### 8.2 Swift-Level Tests

- Swift Package tests that import generated bindings
- Test type construction, method calls, error handling
- Mock controller tests that do not need real API keys:

```swift
import XCTest
@testable import ZeroClawBridge

final class BridgeTests: XCTestCase {
    func testControllerCreation() {
        let controller = ZeroClawController.withDefaults(dataDir: "/tmp/test")
        XCTAssertEqual(controller.getStatus(), .stopped)
    }

    func testStartStop() throws {
        let controller = ZeroClawController.withDefaults(dataDir: "/tmp/test")
        try controller.start()
        XCTAssertEqual(controller.getStatus(), .running)
        try controller.stop()
        XCTAssertEqual(controller.getStatus(), .stopped)
    }

    func testSendMessage() {
        let controller = ZeroClawController.withDefaults(dataDir: "/tmp/test")
        let result = controller.sendMessage(content: "Hello")
        XCTAssertTrue(result.success)
        XCTAssertEqual(controller.getMessages().count, 2)
    }
}
```

### 8.3 Integration Tests

- Xcode test scheme exercising the full Swift → Rust → Swift roundtrip
- Simulator-based UI tests for the iOS app (XCUITest)

---

## Implementation Sequence

| Order | Phase | Effort | Dependencies |
|-------|-------|--------|-------------|
| 1 | Phase 1: Shared bridge crate | 2-3 days | None |
| 2 | Phase 2: iOS target compilation | 1 day | Phase 1 |
| 3 | Phase 3: Swift binding generation | 1 day | Phase 2 |
| 4 | Phase 4: XCFramework packaging | 1-2 days | Phase 3 |
| 5 | Phase 5: SPM integration | 1 day | Phase 4 |
| 6 | Phase 7: CI/CD | 1-2 days | Phase 4 |
| 7 | Phase 6: Swift client app | 3-5 days | Phase 5 |
| 8 | Phase 8: Testing | 2-3 days | Phase 6 |

**Total estimated effort: 12-18 days**

---

## Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| UniFFI proc macros may not generate valid Swift for complex types | Test binding generation early (Phase 3) before building the full app |
| iOS background execution limits prevent daemon mode | Document that iOS is client-only; use `BGAppRefreshTask` for periodic checks |
| `rusqlite` bundled SQLite may have issues compiling for iOS | Test early; bundled SQLite + `cc` crate should work but verify on all 3 targets |
| macOS CI runners are expensive (2x cost of Linux) | Run iOS builds only on tagged releases and iOS-path changes |
| Apple Developer certificate required for device testing | CI only needs simulator; document developer account requirement |
| Bridge crate refactoring may break existing Android builds | Run Android build checks in same PR; keep `android-bridge/` as thin wrapper |

---

## Files to Create

| File | Description |
|------|-------------|
| `externals/zeroclaw/clients/bridge/Cargo.toml` | Shared bridge crate manifest |
| `externals/zeroclaw/clients/bridge/src/lib.rs` | Platform-neutral UniFFI types and controller |
| `externals/zeroclaw/clients/bridge/uniffi-bindgen.rs` | Binding generator binary |
| `externals/zeroclaw/clients/ios/Package.swift` | Swift Package Manager manifest |
| `externals/zeroclaw/clients/ios/build-xcframework.sh` | XCFramework build script |
| `externals/zeroclaw/clients/ios/README.md` | iOS integration documentation |
| `externals/zeroclaw/clients/ios-app/` | SwiftUI client app (entire directory) |
| `.github/workflows/ios-build.yml` | CI workflow for iOS builds |

## Files to Modify

| File | Change |
|------|--------|
| `externals/zeroclaw/clients/android-bridge/Cargo.toml` | Depend on shared bridge crate |
| `externals/zeroclaw/clients/android-bridge/src/lib.rs` | Re-export from shared bridge |
| `externals/zeroclaw/Cargo.toml` | Add `bridge` crate to workspace members |
| `externals/zeroclaw/.cargo/config.toml` | Add iOS target entries |
