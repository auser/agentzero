# AgentZero External-Dependency Backlog

Items moved out of `SPRINT.md` because they require external tools, services, or platform-specific toolchains that AgentZero doesn't control. AgentZero is standalone — everything in `SPRINT.md` must be buildable with `cargo build` and no external dependencies.

---

## Channel Tier 3: Voice Wake Word (MEDIUM)

The `channel-voice-wake` integration is classified as **Tier 3** (see
`site/src/content/docs/reference/channels.md`). It depends on `cpal` (a C audio
library that requires OS audio subsystems) and `hound` for WAV encoding. These
dependencies cannot be satisfied in a standard headless CI environment.

**Requires:** A working audio input device, OS audio subsystem (ALSA/CoreAudio/WASAPI), `cpal` C library

**Feature flag:** `channel-voice-wake`

**Re-evaluation criteria:** A PR that introduces a mock/stub audio backend
(e.g., a `#[cfg(test)]` shim that feeds pre-recorded PCM frames instead of
reading from hardware) and demonstrates green CI would qualify this channel for
promotion to Tier 2.

- [ ] **Stub audio backend** — Add a `MockAudioSource` that replays a static WAV buffer, gated behind `#[cfg(test)]`
- [ ] **Wiremock e2e test** — Mock the Whisper transcription HTTP endpoint; feed a WAV with a known wake word; assert a `ChannelMessage` is emitted
- [ ] **CI slot** — Add `channel-voice-wake` to the e2e test matrix once the above land
- [ ] **Tier 2 promotion PR** — Reference this backlog entry and the `reference/channels.md` promotion checklist

---

## iOS Swift Support (HIGH)

Full iOS support via UniFFI: XCFramework packaging, Swift Package Manager integration, and SwiftUI reference app. Large effort (~12-18 days).

**Requires:** Xcode, iOS toolchain (`aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios`)

**Plan:** `specs/plans/02-ios-swift-support.md`

- [ ] **Phase 1:** Shared bridge crate refactoring — Extract FFI types into platform-neutral crate
- [ ] **Phase 2:** iOS target compilation — `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios`
- [ ] **Phase 3:** Swift binding generation — `uniffi-bindgen` auto-generates Swift types
- [ ] **Phase 4:** XCFramework packaging — Bundle static library + headers for Xcode
- [ ] **Phase 5:** Swift Package Manager integration — `Package.swift` for SPM distribution
- [ ] **Phase 6:** SwiftUI reference app — Demo app exercising core agent functionality
- [ ] **Phase 7:** CI/CD — GitHub Actions multi-arch iOS builds
- [ ] **Phase 8:** Testing — Rust-level + Swift-level + integration tests

---

## Redis / NATS Event Bus Backend (MEDIUM)

Add Redis pub/sub (and future NATS) as alternative event bus backends for horizontal scaling beyond gossip mesh. Gossip bus (shipped Sprint 40) works for small clusters; Redis/NATS better for large deployments.

**Requires:** Redis server (or NATS server)

**Plan:** `specs/plans/09-distributed-event-bus.md`

- [ ] **Redis backend** — Feature-gated `bus-redis`. `RedisEventBus` implementing `EventBus` trait via redis pub/sub + capped list persistence.
- [ ] **Config** — `event_bus = "redis"` + `redis_url` in `[swarm]`.
- [ ] **Horizontal scaling** — Multiple instances share Redis, route events via correlation_id.
- [ ] **NATS alternative** (future) — Extensible trait-based design accommodates NATS JetStream.

---

## Fleet Mode (mvmctl + mvmd Integration) (HIGH)

Agent-as-a-Service backed by Firecracker microVM isolation via mvmctl/mvmd. Feature-gated behind `"fleet"`.

**Requires:** mvmd (gomicrovm.com), Firecracker, Linux KVM

- [ ] AgentStore backend that delegates to mvmd for Firecracker-based isolation
- [ ] Warm sandbox pool integration (sub-second agent provisioning)
- [ ] Sleep/wake with wake-on-message (webhook triggers snapshot restore)
- [ ] agentzero Firecracker template (Nix flake for rootfs)
- [ ] Config/secrets drive injection
- [ ] Autoscaling across cloud providers (Hetzner, AWS, GCP, DigitalOcean)
- [ ] Per-agent Turso auto-provisioning for memory durability across instances

---

## Multi-Node Orchestration — Full Distributed (HIGH)

Full multi-node distributed orchestration beyond gossip event bus.

**Requires:** Multiple networked nodes, cluster infrastructure

- [ ] Node registry (capabilities, health status)
- [ ] Task routing to best-fit node
- [ ] Result aggregation from distributed sub-agents
- [ ] Remote delegation with `node` parameter
- [ ] Gateway `node_control` endpoint

---

## Binary Size Profiling & Compression

**Requires:** `cargo-bloat` CLI tool, UPX compressor

- [ ] **cargo-bloat audit** — Profile with `cargo bloat --release --crates`, eliminate hidden size contributors.
- [ ] **Binary compression** — Evaluate UPX for deployment-time compression.
- [ ] **CI: cargo-bloat report** — Add size breakdown as CI artifact for tracking trends.

---

## MicroVM Agent Backends (MEDIUM)

`MicroVmSandbox` (in `crates/agentzero-orchestrator/src/sandbox.rs`) exists as a
proof-of-concept for Firecracker-based agent isolation (shipped Sprint 72F). Production
Firecracker isolation for AgentZero is delegated to the [mvm project](https://gomicrovm.com).

**Requires:** mvmd daemon (gomicrovm.com), Firecracker binary, Linux KVM

**Status:** `MicroVmSandbox` is maintenance-only — the struct and trait impl are
preserved for reference and to avoid breaking the `AgentSandbox` trait surface.
New Firecracker investment (warm pools, sleep/wake, multi-tenant isolation) belongs
in an `mvm`-backed integration, not in `MicroVmSandbox` directly.

**Re-evaluation criteria:** When the `mvm` interface stabilises and a
`MvmAgentSandbox` wrapper can be written with a clean external-dependency boundary
(feature-gated, no unconditional kvmd/Firecracker install required), this entry can
be moved to a proper sprint task.

- [ ] **`MvmAgentSandbox`** — Implement `AgentSandbox` trait wrapping `mvmctl` subprocess
- [ ] **Feature gate** — `#[cfg(feature = "sandbox-mvm")]` — not enabled by default
- [ ] **Warm pool integration** — Sub-second agent provisioning via mvm snapshot restore
- [ ] **Sleep/wake** — Wake-on-message via webhook triggers snapshot restore
- [ ] **Per-agent secret injection** — Config/secrets drive injection via mvm
- [ ] **Autoscaling** — Across cloud providers (Hetzner, AWS, GCP, DigitalOcean) via mvm
- [ ] **CI** — Skip mvm sandbox tests in standard CI; add an opt-in matrix entry for Linux KVM environments

---


