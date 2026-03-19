# AgentZero External-Dependency Backlog

Items moved out of `SPRINT.md` because they require external tools, services, or platform-specific toolchains that AgentZero doesn't control. AgentZero is standalone — everything in `SPRINT.md` must be buildable with `cargo build` and no external dependencies.

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
