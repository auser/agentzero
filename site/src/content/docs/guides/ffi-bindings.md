---
title: FFI Bindings
description: Use AgentZero from Swift, Kotlin, Python, or TypeScript via the agentzero-ffi crate.
---

The `agentzero-ffi` crate exposes the AgentZero agent runtime to non-Rust
languages through a single unified crate with feature-gated backends:

| Language           | Backend  | Feature flag | Output                         |
| ------------------ | -------- | ------------ | ------------------------------ |
| Swift              | UniFFI   | `uniffi`     | `.swift` + `.h` + `.modulemap` |
| Kotlin             | UniFFI   | `uniffi`     | `.kt`                          |
| Python             | UniFFI   | `uniffi`     | `.py`                          |
| TypeScript/Node.js | napi-rs  | `node`       | `.node` native addon           |

## Prerequisites

- Rust toolchain (1.80+)
- [just](https://github.com/casey/just) task runner
- For Node.js bindings: Node.js 18+ and npm

## Generating bindings

### Swift, Kotlin, and Python (UniFFI)

Generate all three at once:

```bash
just ffi
```

Or generate individually:

```bash
just ffi-swift
just ffi-kotlin
just ffi-python
```

Generated files appear in:

```
crates/agentzero-ffi/bindings/
  swift/
  kotlin/
  python/
```

### TypeScript / Node.js (napi-rs)

```bash
just ffi-node
```

This produces a native `.node` addon in `target/release/`.

## API overview

All language bindings expose the same core API through the `AgentZeroController`:

### Types

| Type                 | Description                                         |
| -------------------- | --------------------------------------------------- |
| `AgentZeroConfig`    | Configuration: config path, workspace root, provider/model overrides |
| `AgentResponse`      | Agent reply: `text` + `metrics_json`                |
| `ChatMessage`        | History entry: `role`, `content`, `timestamp_ms`    |
| `AgentStatus`        | Enum: `Idle`, `Running`, `Error { message }`        |
| `AgentZeroError`     | Enum: `ConfigError`, `RuntimeError`, `ProviderError`, `TimeoutError` |

### Controller methods

| Method              | Description                                         |
| ------------------- | --------------------------------------------------- |
| `new(config)`       | Create a controller with full configuration         |
| `with_defaults(config_path, workspace_root)` | Create with minimal config |
| `send_message(msg)` | Send a message and get the agent's response         |
| `send_message_async(msg)` | Send a message asynchronously (Node.js only) |
| `status()`          | Get the current agent status                        |
| `get_history()`     | Retrieve conversation history                       |
| `clear_history()`   | Clear conversation history                          |
| `get_config()`      | Read the current configuration                      |
| `update_config(c)`  | Update configuration for subsequent calls           |
| `register_tool(name, description)` | Register a custom tool from the host language |
| `registered_tool_names()` | List names of all registered FFI tools        |
| `version()`         | Return the crate version string                     |

## Usage examples

### Swift

```swift
import AgentZeroFFI

let config = AgentZeroConfig(
    configPath: "agentzero.toml",
    workspaceRoot: FileManager.default.currentDirectoryPath,
    provider: "anthropic",
    model: nil,
    profile: nil
)

let controller = AgentZeroController(config: config)

do {
    let response = try controller.sendMessage(message: "Hello from Swift!")
    print(response.text)
} catch let error as AgentZeroError {
    print("Error: \(error)")
}
```

### Kotlin

```kotlin
import uniffi.agentzero_ffi.*

val config = AgentZeroConfig(
    configPath = "agentzero.toml",
    workspaceRoot = System.getProperty("user.dir"),
    provider = "anthropic",
    model = null,
    profile = null
)

val controller = AgentZeroController(config)

try {
    val response = controller.sendMessage("Hello from Kotlin!")
    println(response.text)
} catch (e: AgentZeroError) {
    println("Error: $e")
}
```

### Python

```python
from agentzero_ffi import AgentZeroConfig, AgentZeroController, AgentZeroError

config = AgentZeroConfig(
    config_path="agentzero.toml",
    workspace_root=".",
    provider="anthropic",
    model=None,
    profile=None,
)

controller = AgentZeroController(config)

try:
    response = controller.send_message("Hello from Python!")
    print(response.text)
except AgentZeroError as e:
    print(f"Error: {e}")
```

### TypeScript / Node.js

```typescript
import { AgentZeroController } from "agentzero-ffi";

const controller = new AgentZeroController({
  configPath: "agentzero.toml",
  workspaceRoot: process.cwd(),
  provider: "anthropic",
  model: undefined,
  profile: undefined,
});

const response = controller.sendMessage("Hello from TypeScript!");
console.log(response.text);
```

### Node.js: Async and Tool Registration

The Node.js bindings include additional methods not available in UniFFI bindings:

```typescript
// Register a custom tool
controller.registerTool("my_tool", "Description of what this tool does");

// List registered tools
const tools = controller.registeredToolNames();
console.log(tools); // ["my_tool"]

// Async message sending (non-blocking)
const response = await controller.sendMessageAsync("Hello!");
console.log(response.text);
```

`sendMessageAsync()` runs the agent on a separate thread via `spawn_blocking`, keeping the Node.js event loop free.

## Architecture

The crate uses a single-crate, dual-backend design:

```
agentzero-ffi/
  src/
    lib.rs             # Core types + AgentZeroController + UniFFI scaffolding
    node_bindings.rs   # napi-rs wrappers (behind "node" feature)
  uniffi-bindgen.rs    # UniFFI CLI for generating bindings
  build.rs             # Conditional napi-build setup
  package.json         # npm metadata for napi-rs
```

- **`uniffi` feature** (default) compiles UniFFI scaffolding and derive macros.
  The `uniffi-bindgen` binary uses the compiled `.dylib` to generate
  Swift/Kotlin/Python source files.
- **`node` feature** (opt-in) compiles napi-rs wrappers that delegate to the
  same `AgentZeroController`. Building produces a `.node` native addon.

Both backends share the same Rust implementation — the controller manages a
global Tokio runtime and bridges synchronous FFI calls to the async
`agentzero-infra` runtime module.

## Cross-compilation

### iOS (Swift)

Build a universal static library for iOS simulators and devices:

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios

cargo build -p agentzero-ffi --release --target aarch64-apple-ios
cargo build -p agentzero-ffi --release --target aarch64-apple-ios-sim

just ffi-swift
```

Then link `libagentzero_ffi.a` and the generated `.swift`/`.h` files into your
Xcode project. See the [iOS support plan](/agentzero/roadmap/) for the full
XCFramework packaging workflow.

### Android (Kotlin)

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

# Requires Android NDK — set ANDROID_NDK_HOME
cargo build -p agentzero-ffi --release --target aarch64-linux-android

just ffi-kotlin
```

See the [Android guide](/agentzero/guides/android/) for NDK setup and Gradle
integration details.

## Troubleshooting

### `UniFfiTag` not found

The `uniffi::setup_scaffolding!()` macro must live in the crate root (`lib.rs`),
not in a submodule. If you see this error after restructuring, ensure the macro
call is directly in `lib.rs`.

### `uniffi-bindgen` binary not found

The bindgen binary requires the `uniffi-cli` feature:

```bash
cargo run -p agentzero-ffi --features uniffi-cli --bin uniffi-bindgen generate \
    --library target/release/libagentzero_ffi.dylib \
    --language swift \
    --out-dir bindings/swift
```

### napi-rs build fails

Ensure Node.js 18+ is installed and the `node` feature is enabled:

```bash
cargo build -p agentzero-ffi --release --no-default-features --features node
```

## Next steps

- [Quick Start](/agentzero/quickstart/) — try the CLI first
- [Android guide](/agentzero/guides/android/) — full Android compilation walkthrough
- [Raspberry Pi guide](/agentzero/guides/raspberry-pi/) — ARM deployment
- [Architecture](/agentzero/architecture/) — understand the runtime internals
