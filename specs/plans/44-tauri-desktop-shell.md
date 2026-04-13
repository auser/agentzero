# Plan: AgentZero Desktop — Full-Featured Tauri 2 Shell

## Context

AgentZero has no desktop shell today — just a CLI, an HTTP/WebSocket gateway, and a React SPA in [ui/](ui/). A React dashboard has already been started at [ui/src/routes/dashboard/index.tsx](ui/src/routes/dashboard/index.tsx) with 15 routes covering most of the feature surface, but it runs in a browser against a separately-launched gateway. The goal is to turn this into a **first-class desktop application** that exposes **every** AgentZero capability — agents, workflows, tools, channels, plugins, cron, memory, models, config, chat, canvas, approvals, runs, events, audit, hardware, autopilot, credentials, and screen/audio capture — as one cohesive Tauri 2 app.

The inspiration for the local-first capture experience (floating bar, local library, transcription, chat-over-recordings) comes from an MIT-licensed open-source reference, but the build target is broader: a complete AgentZero control surface, not a single-purpose recorder. The reference app is a thin JS/Electron wrapper over a closed-source native binary, so there's no meaningful code to port — only UX ideas. Everything here is Rust-native.

**Why this matters.** The dashboard is already usable but lives in a browser tab, has gaps (no UI for plugins, audit, hardware, autopilot), requires running the gateway as a separate process, and has a second orphaned React app at `crates/agentzero-config-ui/`. Unifying these into a native desktop shell gives users one installable binary, offline-first, with OS integration (system tray, global hotkeys, keychain, notifications, file dialogs, screen capture).

## Approach

Add **two new workspace members**, consolidate the orphan config UI into [ui/](ui/), and fill the four gap routes. The existing React SPA is wrapped unchanged — Tauri points at it, and the gateway is linked in-process inside the Tauri backend.

```
bin/agentzero-desktop/            # Tauri 2 shell, embeds gateway, wraps ui/
crates/agentzero-capture/         # Screen / audio / camera capture + local transcription
```

### Gateway: embedded in-process, with optional remote URL

The Tauri backend links [agentzero-gateway](crates/agentzero-gateway/) as a library and starts its Axum router on `127.0.0.1:<ephemeral>` inside the desktop process. The React UI's API client points at that local URL. A Settings option lets users switch to a remote gateway URL for the headless-server case. Benefits:

- **Single binary, zero setup.** Users launch the app; the gateway is already running.
- **Offline-first.** No external processes, no port conflicts with a dev gateway.
- **No architectural rewrite.** `agentzero-gateway` already exposes its router as a library — Tauri just `tokio::spawn`s it on startup.
- **Remote mode preserved.** Users running `agentzero` headless on a server can still point the desktop at it via a URL setting.

Auth tokens are stored in the OS keychain via [`keyring`](https://crates.io/crates/keyring) (macOS Keychain, Windows Credential Manager, Secret Service on Linux) instead of `localStorage`.

### Feature surface (mapped to existing + new routes)

Existing routes stay as-is and get Tauri-native enhancements (native file dialogs, notifications, tray actions):

| Area | Route | Source crate | Status |
|---|---|---|---|
| Dashboard hub | `/dashboard` | — | exists, extend |
| Workflows + canvas | `/workflows`, `/canvas` | `agentzero-orchestrator` | exists |
| Chat | `/chat` | gateway `/ws/chat` | exists |
| Agents | `/agents` | `agentzero-core`, `agentzero-infra` | exists |
| Runs | `/runs` | `agentzero-infra` | exists |
| Tools | `/tools` | `agentzero-tools` | exists |
| Channels | `/channels` | `agentzero-channels` | exists |
| Models | `/models` | `agentzero-providers` | exists |
| Config | `/config` | `agentzero-config` | exists, extend (see below) |
| Memory | `/memory` | `agentzero-storage` | exists |
| Schedule | `/schedule` | `agentzero-orchestrator` | exists |
| Approvals | `/approvals` | gateway | exists |
| Events | `/events` | gateway | exists |
| Templates | `/templates` | `agentzero-infra` | exists |

**New v1 routes** (filling the gaps):

| Area | Route | Backed by |
|---|---|---|
| Plugins (WASM) | `/plugins` | `agentzero-plugins`, `agentzero-plugin-sdk` |
| Audit log | `/audit` | `agentzero-gateway` audit module |
| Hardware monitor | `/hardware` | Plan 42 Phase A device detection + live telemetry |
| Autopilot missions | `/autopilot` | `agentzero-autopilot` |
| Credentials manager | `/credentials` | `agentzero-auth` (currently embedded in agent forms) |
| Recordings | `/recordings` | new `agentzero-capture` crate |
| Visual config editor | `/config/visual` | consolidated from `agentzero-config-ui` |

### Consolidating `agentzero-config-ui`

Port the visual node-graph config editor from `crates/agentzero-config-ui/ui/` into [ui/src/routes/config/visual/](ui/src/routes/config/visual/) as a nested route. Keep the Rust backend helpers (`toml_bridge.rs`, `schema.rs`, `agents_api.rs`) by moving them into `agentzero-gateway` under a new `/v1/config/visual/*` endpoint group. Delete `crates/agentzero-config-ui/` once the port is verified. This eliminates dual-UI confusion and gives users one desktop app.

### Capture crate (`agentzero-capture`)

Harvested UX concepts — floating always-on-top bar, multi-monitor picker, camera overlay, global hotkey, local library, real-time subtitles, chat-over-transcripts — implemented on Rust crates:

| Function | Crate |
|---|---|
| Screen capture | [`scap`](https://github.com/CapSoftware/scap) (ScreenCaptureKit / WGC / X11) |
| Audio (mic + system) | `cpal` + scap system-audio tap on macOS 13+ |
| Camera | `nokhwa` |
| Encode / mux | `ffmpeg-next` (hardware encoders via `h264_videotoolbox`, `h264_mf`) |
| Transcription | `whisper-rs` (Metal-accelerated on macOS) |
| Library catalog | existing [`agentzero-storage`](crates/agentzero-storage/) + new `recordings` migration |
| Transcript search | planned Tantivy+HNSW retrieval (see `project_retrieval_upgrade`) |

Chat-over-recordings is not a separate feature — it's the existing agent/tool loop with a new `transcript_search` tool that hits the Tantivy+HNSW index.

### Native desktop integration

- **System tray** — record / stop, open dashboard, quit, active run count badge
- **Global shortcuts** via `tauri-plugin-global-shortcut` — record (Cmd+Shift+R), open chat (Cmd+Shift+C), new run (Cmd+Shift+N)
- **Window state** via `tauri-plugin-window-state` — remember sizes, positions, monitor
- **Notifications** via `tauri-plugin-notification` — run completions, approval requests, errors
- **Keychain auth** via `keyring` crate — gateway token, provider API keys
- **Native file dialogs** via `tauri-plugin-dialog` — workflow import/export, model file pickers
- **Deep links** via `tauri-plugin-deep-link` — `agentzero://run/:id`, `agentzero://chat/:id`
- **Auto-update** via `tauri-plugin-updater` — scaffolded, disabled until release signing is set up
- **macOS entitlements** in `tauri.conf.json` — screen recording, camera, microphone, accessibility (for hotkeys), network client

## Files to create

### `bin/agentzero-desktop/`

```
Cargo.toml                    # tauri 2, tauri-build, plugins, agentzero-gateway,
                              # agentzero-capture, agentzero-storage, agentzero-infra,
                              # keyring, tokio, anyhow
tauri.conf.json               # windows: main, floating-bar; entitlements; tray; bundle
build.rs                      # tauri_build::build()
src/main.rs                   # app setup, in-process gateway spawn, command/plugin wiring
src/gateway_embed.rs          # link agentzero-gateway router, bind ephemeral port,
                              # expose base URL to the frontend via a Tauri command
src/commands/mod.rs           # #[tauri::command] handlers grouped by domain
src/commands/capture.rs       # start_recording, stop_recording, list_recordings,
                              # list_displays, list_cameras, open_recording
src/commands/system.rs        # hardware_info, open_path, reveal_in_finder, notify
src/commands/auth.rs          # keychain get/set/delete for gateway token + provider keys
src/commands/settings.rs      # remote_gateway_url get/set
src/floating_bar.rs           # always-on-top transparent window (recording controls)
src/tray.rs                   # system tray menu + live badge
src/hotkey.rs                 # global shortcut registration
src/permissions.rs            # macOS screen-recording / mic / camera prompts
src/deep_link.rs              # agentzero:// scheme routing into the SPA
icons/                        # icon set for bundler
```

### `crates/agentzero-capture/`

```
Cargo.toml                    # scap, cpal, nokhwa, ffmpeg-next, whisper-rs,
                              # tokio, serde, thiserror, agentzero-core, agentzero-storage
src/lib.rs                    # public CaptureSession API
src/session.rs                # state machine: Idle -> Preparing -> Recording ->
                              #                Finalizing -> Done/Error
src/screen.rs                 # scap wrapper: enumerate displays, open stream
src/audio.rs                  # cpal mic + system-audio capture, ring buffer
src/camera.rs                 # nokhwa webcam feed (optional overlay)
src/encode.rs                 # ffmpeg pipeline: video + audio + camera overlay -> MP4
src/transcribe.rs             # whisper-rs streaming over finalized chunks -> subtitle JSON
src/library.rs                # Recording model + CRUD against agentzero-storage
src/error.rs                  # CaptureError with thiserror
tests/session_state.rs        # state-machine tests with a mocked capture source
```

### `ui/` additions

```
ui/src/routes/plugins/           # /plugins route (install / enable / disable)
ui/src/routes/audit/             # /audit route (filterable log viewer)
ui/src/routes/hardware/          # /hardware route (device info + live telemetry charts)
ui/src/routes/autopilot/         # /autopilot route (missions list + control)
ui/src/routes/credentials/       # /credentials route (keychain-backed)
ui/src/routes/recordings/        # library grid, player with subtitle overlay, transcript pane
ui/src/routes/config/visual/     # ported from crates/agentzero-config-ui/ui/
ui/src/lib/api/plugins.ts        # matching API clients
ui/src/lib/api/audit.ts
ui/src/lib/api/hardware.ts
ui/src/lib/api/autopilot.ts
ui/src/lib/api/credentials.ts
ui/src/lib/api/capture.ts        # wraps Tauri invoke; HTTP fallback for dev in browser
ui/src/lib/tauri.ts              # feature-detected Tauri runtime bridge
ui/src/components/tray-status/   # small widgets surfaced in tray and topbar
```

### Existing files to touch

- [Cargo.toml](Cargo.toml) (workspace root) — add `bin/agentzero-desktop`, `crates/agentzero-capture`; remove `crates/agentzero-config-ui` after port
- [crates/agentzero-storage/src/migrations/](crates/agentzero-storage/src/migrations/) — new migration for `recordings` table (`id`, `path`, `started_at`, `duration_ms`, `display_label`, `has_camera`, `transcript_path`, `thumbnail_path`)
- [crates/agentzero-gateway/src/router.rs](crates/agentzero-gateway/src/router.rs) — add `/v1/plugins/*`, `/v1/audit/*`, `/v1/hardware/*`, `/v1/autopilot/*`, `/v1/credentials/*`, `/v1/config/visual/*` endpoint groups
- [crates/agentzero-gateway/src/lib.rs](crates/agentzero-gateway/src/lib.rs) — confirm `Router` export so the desktop crate can mount it in-process
- [ui/src/store/authStore.ts](ui/src/store/authStore.ts) — Tauri-aware: read/write token via keyring command when running under Tauri, fall back to `localStorage` in browser
- [ui/src/lib/api/client.ts](ui/src/lib/api/client.ts) — resolve base URL from Tauri command (`get_gateway_url`) at startup

## Reuse inventory (don't reinvent)

- [ui/](ui/) — React 19 + Vite + TanStack Router + Radix UI + Zustand — wrapped wholesale
- [crates/agentzero-gateway/](crates/agentzero-gateway/) — 42+ routes, WS streams, embedded in-process
- [crates/agentzero-storage/](crates/agentzero-storage/) — SQLite + encryption
- [crates/agentzero-infra/](crates/agentzero-infra/) — agent/tool runtime already used by the gateway
- Plan 42 Phase A hardware detection — feeds the `/hardware` route and picks hw encoders in `agentzero-capture`
- Planned Tantivy+HNSW retrieval upgrade — transcript search and chat-over-recordings
- [crates/agentzero-autopilot/](crates/agentzero-autopilot/) — backs `/autopilot`
- [crates/agentzero-plugins/](crates/agentzero-plugins/) — backs `/plugins`
- [crates/agentzero-auth/](crates/agentzero-auth/) — backs `/credentials`
- All existing dashboard components in [ui/src/components/dashboard/](ui/src/components/dashboard/)

## Implementation phases

Each phase leaves the workspace compilable, clippy-clean, and independently mergeable.

1. **Shell skeleton.** `bin/agentzero-desktop` with Tauri 2 boilerplate, loads `ui/` dev server in `tauri dev` and bundled assets in release. Single main window. No gateway yet — points at an external gateway for now.
2. **In-process gateway.** Link `agentzero-gateway` as a library, spawn its router on an ephemeral port, expose `get_gateway_url` Tauri command, update [ui/src/lib/api/client.ts](ui/src/lib/api/client.ts) to resolve base URL from it. Add Settings toggle for remote gateway URL.
3. **Keychain auth.** `keyring` crate + `commands/auth.rs`; update `authStore.ts` with a Tauri-aware adapter.
4. **Tray, hotkeys, notifications, window state.** Tauri plugins wired; tray shows active run count badge.
5. **Gap routes — backend.** New endpoint groups in `agentzero-gateway`: plugins, audit, hardware, autopilot, credentials.
6. **Gap routes — frontend.** `/plugins`, `/audit`, `/hardware`, `/autopilot`, `/credentials` React routes with their API clients.
7. **Config UI consolidation.** Port `crates/agentzero-config-ui/ui/` into `ui/src/routes/config/visual/`; move Rust helpers into `agentzero-gateway`; delete the old crate.
8. **Capture crate skeleton.** `agentzero-capture` with `scap` + `ffmpeg-next` recording a 10 s MP4 via CLI harness. No audio, camera, or transcription yet.
9. **Audio + camera.** `cpal` mic + system audio, optional `nokhwa` overlay, full `CaptureSession` state machine with tests.
10. **Storage integration.** `recordings` table migration, `library.rs` CRUD, thumbnail generation on recording finalize.
11. **Floating bar + hotkey.** Always-on-top transparent recording controls, Cmd+Shift+R toggles, display picker.
12. **Transcription.** `whisper-rs` streaming with tiny.en default, subtitle JSON alongside MP4, model downloaded on first use.
13. **Recordings route.** Library grid, player with subtitle overlay, transcript pane.
14. **Chat over recordings.** New `transcript_search` tool in `agentzero-tools` hitting the Tantivy+HNSW index; existing chat UI consumes it with zero changes.
15. **Packaging.** `cargo tauri build` for macOS (`.app` + `.dmg`), Windows (`.msi` + `.exe`), Linux (`.AppImage` + `.deb`). Auto-update plugin scaffolded but disabled.

## Verification

- `cargo check --workspace` — all members build, including new crates
- `cargo test -p agentzero-capture` — state-machine unit tests with mocked capture source
- `cargo clippy --workspace -- -D warnings` — honors the project zero-warnings rule
- `cargo tauri dev` in `bin/agentzero-desktop` — app launches, shows dashboard, live gateway routes respond from the embedded in-process server
- Manual smoke: start a run from `/runs`, watch it stream in `/events`, receive a completion notification, click the tray, see the badge clear
- Manual capture: Cmd+Shift+R records 30 s of screen + speech, file lands in `~/Library/Application Support/agentzero/recordings/`, catalog row appears in `/recordings`, subtitle overlay renders, transcript is searchable in chat
- Manual gap routes: `/plugins` installs a WASM module, `/audit` filters by time window, `/hardware` shows device info from Plan 42 Phase A, `/autopilot` starts/stops a mission, `/credentials` writes to the OS keychain
- Manual remote mode: toggle Settings → remote gateway URL → point at a headless agentzero on another host, verify the UI hits it instead of the embedded server
- `cargo tauri build` — produces installable bundles on macOS / Windows / Linux; launches without missing entitlements

## Non-goals for v1

- Auto-update channel (plugin scaffolded but disabled until release signing is configured)
- Linux system-audio capture (PipeWire story is messier; mic works everywhere)
- LAN sharing for recordings (future extension via `agentzero-gateway`)
- Mobile variants — desktop only
- Any cloud-dependent recording / transcription / indexing pipeline — fully local by design
