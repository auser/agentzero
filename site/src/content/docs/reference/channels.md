---
title: Channel Reference
description: Complete channel listing, feature flags, test coverage, and maintenance tier classifications for all AgentZero channel integrations.
---

AgentZero ships **31 channel integrations** organised into three maintenance tiers.
This page is the authoritative reference for the tier each channel belongs to, its
feature flag, and its test coverage status.

For configuration details and setup instructions see the
[Channel Integrations guide](/guides/channels).

---

## Channel Maintenance Tiers

AgentZero uses a three-tier model to communicate maintenance commitment for each
channel integration.

**Tier 1** — Actively maintained. Integration tests required (inline unit tests
_and_ wiremock e2e tests). Included in CI by default via the `channels-standard`
feature flag. Regressions are treated as P0 bugs.

**Tier 2** — Supported best-effort. Feature-gated (not included in default CI
runs). Inline unit tests present; e2e tests welcome but not required. Bugs are
triaged case-by-case. Channels in this tier that lose their maintainer or
accumulate unfixed regressions are eligible for promotion to Tier 3.

**Tier 3** — Archived or heavy external-dependency. Not included in any default
feature set. Moved to `BACKLOG-EXTERNAL.md` if no community usage evidence is
found within a release cycle. PRs that add tests or fix regressions are always
welcome and can trigger re-evaluation for Tier 2.

| Channel | Tier | Feature Flag | Inline Tests | E2E Tests | Notes |
|---------|------|-------------|:------------:|:---------:|-------|
| CLI | 1 | `channel-cli` *(default)* | ✅ | ✅ | No credentials — stdin/stdout |
| Telegram | 1 | `channel-telegram` | ✅ | ✅ | Bot API polling; free bot token |
| Discord | 1 | `channel-discord` | ✅ | ✅ | Gateway WebSocket; free bot |
| Slack | 1 | `channel-slack` | ✅ | ✅ | Socket Mode; free workspace tier |
| Signal | 1 | `channel-signal` | ✅ | ✅ | Requires `signal-cli` REST daemon |
| Webhook | 1 | `channel-webhook` *(default)* | ✅ | ✅ | No credentials — generic HTTP POST |
| Mattermost | 1 | `channel-mattermost` | ✅ | ✅ | Self-hosted or Mattermost Cloud |
| Matrix | 1 | `channel-matrix` | ✅ | ✅ | Federated; E2EE-capable |
| WhatsApp | 1 | `channel-whatsapp` | ✅ | ✅ | Meta Business Cloud API |
| SMS | 1 | `channel-sms` | ✅ | ✅ | Twilio REST API |
| Email | 2 | `channel-email` | ✅ | — | SMTP + IMAP; e2e needs live mail server |
| IRC | 2 | `channel-irc` | ✅ | — | TLS socket; e2e needs live IRC server |
| Nostr | 2 | `channel-nostr` | ✅ | — | WebSocket relay; e2e needs live relay |
| Discord History | 2 | `channel-discord-history` | ✅ | ✅ | Backfills guild history; same bot as Discord |
| Gmail Push | 2 | `channel-gmail-push` | ✅ | ✅ | Google Pub/Sub; requires GCP project |
| Lark | 2 | `channel-lark` | ✅ | ✅ | ByteDance Lark (international) |
| Feishu | 2 | `channel-feishu` | ✅ | ✅ | ByteDance Feishu (China region) |
| DingTalk | 2 | `channel-dingtalk` | ✅ | ✅ | Alibaba DingTalk outgoing webhook |
| QQ Official | 2 | `channel-qq-official` | ✅ | ✅ | Tencent QQ Bot Open Platform |
| Napcat | 2 | `channel-napcat` | ✅ | ✅ | QQ via OneBot v11 HTTP (NapCatQQ) |
| Linq | 2 | `channel-linq` | ✅ | ✅ | Linq messaging platform |
| WATI | 2 | `channel-wati` | ✅ | ✅ | WhatsApp Team Inbox (paid platform) |
| NextCloud Talk | 2 | `channel-nextcloud-talk` | ✅ | ✅ | Self-hosted Spreed OCS API |
| ACP | 2 | `channel-acp` | ✅ | ✅ | Agent Client Protocol (agent-to-agent) |
| ClawdTalk | 2 | `channel-clawdtalk` | ✅ | ✅ | Self-hosted chat platform |
| MQTT | 2 | `channel-mqtt` | ✅ | — | MQTT broker pub/sub; e2e needs broker |
| Transcription | 2 | `channel-transcription` | ✅ | — | Whisper-compatible API; audio-to-text |
| iMessage | 2 | `channel-imessage` | ✅ | — | macOS only; AppleScript + SQLite |
| WhatsApp Web | 2 | `channel-whatsapp-web` | ✅ | — | Multi-device protocol; QR or pairing code |
| WhatsApp Storage | 2 | `channel-whatsapp` | ✅ | — | In-process ring buffer; not a transport |
| Voice Wake | 3 | `channel-voice-wake` | ✅ | — | Requires audio hardware (`cpal`); see below |

---

## Tier 1 Channels

These channels are part of the `channels-standard` feature set and are verified on
every CI run.

### CLI

The built-in command-line channel. Reads from `stdin` and writes to `stdout`.
No credentials required. Enabled by default.

**Feature flag:** `channel-cli` (included in `default`)

### Telegram

Connects to the [Telegram Bot API](https://core.telegram.org/bots/api) using
long-polling (`getUpdates`). Supports text messages, group chats, inline image
attachments, and webhook registration (`setWebhook` / `deleteWebhook`).

**Feature flag:** `channel-telegram`

### Discord

Connects to the [Discord Gateway](https://discord.com/developers/docs/topics/gateway)
over WebSocket. Supports guilds, threads, direct messages, and typing indicators.

**Feature flag:** `channel-discord`

### Slack

Connects via [Socket Mode](https://api.slack.com/apis/connections/socket) using both
a bot token (`xoxb-…`) and an app-level token (`xapp-…`). Supports threaded replies.

**Feature flag:** `channel-slack`

### Signal

Connects to a locally-running
[signal-cli REST API](https://github.com/bbernhard/signal-cli-rest-api) daemon.
No direct dependency on the Signal servers — all traffic goes through `signal-cli`.

**Feature flag:** `channel-signal`

### Webhook

Generic inbound HTTP channel. The gateway injects messages via `inject_message()`;
the `send()` path is intentionally a no-op (responses are returned in-band by the
caller). No credentials required. Enabled by default.

**Feature flag:** `channel-webhook` (included in `default`)

### Mattermost

Connects to [Mattermost](https://mattermost.com/) via its REST API and WebSocket
event stream. Works with both self-hosted and Mattermost Cloud instances.

**Feature flag:** `channel-mattermost`

### Matrix

Connects to any Matrix homeserver via the
[Client-Server API](https://spec.matrix.org/latest/client-server-api/). Supports
E2EE-capable rooms.

**Feature flag:** `channel-matrix`

### WhatsApp

Connects to the [WhatsApp Cloud API](https://developers.facebook.com/docs/whatsapp/cloud-api)
via Meta Business Platform. Requires a verified business phone number.

**Feature flag:** `channel-whatsapp`

### SMS

Sends and receives SMS messages via the
[Twilio REST API](https://www.twilio.com/docs/sms). Requires a Twilio account SID,
auth token, and a Twilio sending number.

**Feature flag:** `channel-sms`

---

## Tier 2 Channels

These channels are functional and have inline unit tests, but are not included in the
default CI matrix. Build with `all-channels` (or the specific flag) to use them.

| Channel | Why no e2e? |
|---------|------------|
| Email | Requires a live SMTP + IMAP server; wiremock cannot simulate the protocols |
| IRC | Requires a live IRC TCP connection; protocol not HTTP-mockable |
| Nostr | Requires a live WebSocket relay; listen path is WebSocket-only |
| MQTT | Requires a live MQTT broker |
| Transcription | Requires a Whisper-compatible HTTP API; useful only with real audio |
| iMessage | macOS-only; uses AppleScript — no cross-platform mock path |
| WhatsApp Web | Multi-device binary protocol; no HTTP surface to wiremock |
| WhatsApp Storage | Internal ring-buffer utility, not a network transport |

All other Tier 2 channels (Discord History, Gmail Push, Lark, Feishu, DingTalk,
QQ Official, Napcat, Linq, WATI, NextCloud Talk, ACP, ClawdTalk) have full
wiremock e2e tests and can be promoted to Tier 1 with a confirmed CI slot.

---

## Tier 3 Channels

### Voice Wake Word

Real-time voice input using microphone capture (`cpal`), energy-based voice activity
detection, wake-word matching, and Whisper speech-to-text. This channel depends on
`cpal` (a C audio library that requires OS audio subsystems to be present) and
`hound` for WAV encoding. It cannot be tested in a standard headless CI environment.

**Feature flag:** `channel-voice-wake`

**Status:** Tracked in `specs/BACKLOG-EXTERNAL.md`. A PR that adds a mock audio
backend suitable for CI would qualify this channel for Tier 2.

---

## Feature Bundles

| Bundle | Channels Included |
|--------|------------------|
| `default` | `channel-cli`, `channel-webhook` |
| `channels-standard` | All Tier 1 channels (cli, telegram, discord, slack, mattermost, matrix, signal, whatsapp, sms, email, irc, nostr, webhook) |
| `all-channels` | Every channel except `channel-voice-wake` |
| `tls-rustls` | Enables `rustls` TLS backend for `reqwest` and `tokio-tungstenite` |
| `tls-native` | Enables native OS TLS backend |

Build with a specific bundle:

```bash
# Standard channels (recommended for most deployments)
cargo build -p agentzero --release --features channels-standard,tls-rustls

# All channels
cargo build -p agentzero --release --features all-channels,tls-rustls

# Single channel
cargo build -p agentzero --release --features channel-telegram,tls-rustls
```

---

## Running Channel Tests

```bash
# Unit tests only (no network, no credentials)
cargo test -p agentzero-channels

# E2E tests for Tier 1 channels (uses wiremock — no real credentials needed)
cargo test -p agentzero-channels \
  --features channels-standard,tls-rustls \
  --test e2e_channels

# E2E tests for all channels with e2e coverage
cargo test -p agentzero-channels \
  --features all-channels,tls-rustls \
  --test e2e_channels
```

---

## Promoting a Channel

To propose moving a channel from Tier 2 → Tier 1:

1. Add wiremock-based e2e tests to `crates/agentzero-channels/tests/e2e_channels.rs`
   covering at minimum: `send()`, `listen()` (one message received), and
   `health_check()`.
2. Ensure the channel is included in `channels-standard` in `Cargo.toml`.
3. Open a PR — CI will run the e2e suite and confirm green.

To propose moving a channel from Tier 3 → Tier 2:

1. Add at least one `#[test]` block (constructor + name assertion is sufficient).
2. Confirm the channel compiles in CI without the problematic hardware/OS dependency
   (or add a `#[cfg]` guard that stubs the hardware path).
3. Open a PR referencing the `BACKLOG-EXTERNAL.md` entry.