---
title: Channel Integrations
description: Connect AgentZero to messaging platforms — Telegram, Discord, Slack, Signal, WhatsApp, Matrix, Email, and more.
---

Channels connect the agent to messaging platforms. Each channel runs as a listener that forwards messages to the agent loop and sends responses back to the platform.

## Supported Channels

| Channel | Config Key | Transport | Notes |
|---|---|---|---|
| **Telegram** | `channels.telegram` | Bot API (polling) | Supports groups, inline queries |
| **Discord** | `channels.discord` | Gateway WebSocket | Supports guilds, threads |
| **Discord History** | `channels.discord_history` | Gateway WebSocket | Backfill guild history |
| **Slack** | `channels.slack` | Socket Mode | Requires bot + app tokens |
| **Mattermost** | `channels.mattermost` | WebSocket | Self-hosted or cloud |
| **iMessage** | `channels.imessage` | AppleScript + SQLite | macOS only |
| **Matrix** | `channels.matrix` | Client-Server API | Federated, E2EE-capable |
| **Signal** | `channels.signal` | signal-cli REST API | Requires signal-cli daemon |
| **WhatsApp** | `channels.whatsapp` | Cloud API | Meta Business Platform |
| **WhatsApp Web** | `channels.whatsapp_web` | Multi-device protocol | QR code or pairing code |
| **WhatsApp Storage** | `channels.whatsapp_storage` | In-memory ring buffer | Message persistence layer |
| **WATI** | `channels.wati` | WATI API | WhatsApp Team Inbox |
| **MQTT** | `channels.mqtt` | MQTT broker | Pub/sub messaging |
| **Transcription** | `channels.transcription` | Whisper-compatible API | Audio-to-text (Groq default) |
| **Linq** | `channels.linq` | Linq API | Linq messaging platform |
| **NextCloud Talk** | `channels.nextcloud_talk` | Spreed OCS API | Self-hosted collaboration |
| **Email** | `channels.email` | SMTP + IMAP | Send and receive email |
| **Gmail Push** | `channels.gmail_push` | Google Pub/Sub | Real-time Gmail notifications |
| **IRC** | `channels.irc` | TLS socket | Any IRC network |
| **Lark** | `channels.lark` | Open Platform API | ByteDance Lark (international) |
| **Feishu** | `channels.feishu` | Open Platform API | ByteDance Feishu (China) |
| **DingTalk** | `channels.dingtalk` | Outgoing webhook | Alibaba DingTalk |
| **QQ Official** | `channels.qq_official` | Bot Open Platform API | Tencent QQ |
| **Nostr** | `channels.nostr` | Relay WebSocket | Decentralized |
| **ClawdTalk** | `channels.clawdtalk` | REST API | Self-hosted chat |
| **Voice Wake Word** | `channels.voice_wake` | Audio energy detection | Wake word + Whisper |
| **Webhook** | `channels.webhook` | HTTP POST | Generic HTTP integration |
| **Napcat** | `channels.napcat` | OneBot v11 HTTP | QQ via Napcat/OneBot |
| **ACP** | `channels.acp` | Agent Client Protocol | Agent-to-agent communication |
| **SMS** | `channels.sms` | Twilio REST API | Send and receive SMS |

## Quick Start

### 1. Build with channel support

If building from source, enable the `channels-standard` feature:

```bash
cargo build -p agentzero --release --features channels-standard
```

Pre-built binaries include all channels by default.

### 2. Add channel config

Add a `[channels.<name>]` section to your `agentzero.toml`:

```toml
[channels.telegram]
bot_token = "123456:ABC-DEF..."
```

### 3. Start the gateway

```bash
agentzero gateway
```

The gateway automatically starts all configured channels.

### 4. Manage channels

```bash
agentzero channel list              # List all configured channels
agentzero channel add telegram      # Add a channel
agentzero channel remove telegram   # Remove a channel
agentzero channel start             # Start all configured channels
agentzero channel test telegram     # Send a test message through a channel
agentzero channel doctor            # Run channel diagnostics
```

---

## Channel Configuration

### Telegram

```toml
[channels.telegram]
bot_token = "YOUR_TELEGRAM_BOT_TOKEN"      # from @BotFather
allowed_users = []                          # empty = allow all users
privacy_boundary = ""                       # "" | "local_only" | "encrypted_only"
```

Create a bot via [@BotFather](https://t.me/BotFather) on Telegram and copy the token.

### Discord

```toml
[channels.discord]
bot_token = "YOUR_DISCORD_BOT_TOKEN"
privacy_boundary = ""
```

Create a bot in the [Discord Developer Portal](https://discord.com/developers/applications), enable the Message Content intent, and invite it to your server.

### Discord History

```toml
[channels.discord_history]
bot_token = "YOUR_DISCORD_BOT_TOKEN"
```

Uses the same bot as the Discord channel. Backfills guild message history for context.

### Slack

```toml
[channels.slack]
bot_token = "xoxb-YOUR-SLACK-BOT-TOKEN"    # Bot User OAuth Token
app_token = "xapp-YOUR-SLACK-APP-TOKEN"    # App-Level Token (Socket Mode)
privacy_boundary = ""
```

Slack requires two tokens:
1. **Bot token** (`xoxb-...`) — from OAuth & Permissions
2. **App token** (`xapp-...`) — from Basic Information → App-Level Tokens (enable Socket Mode)

### Mattermost

```toml
[channels.mattermost]
base_url = "https://your-mattermost.example.com"
token = "YOUR_MATTERMOST_TOKEN"
channel_id = "YOUR_CHANNEL_ID"
```

### iMessage

```toml
[channels.imessage]
allowed_users = []                          # phone numbers or iCloud emails
```

macOS only. Uses AppleScript to send messages and polls the iMessage SQLite database for incoming messages. No token needed — uses the logged-in macOS user's Messages app.

### Matrix

```toml
[channels.matrix]
homeserver = "https://matrix.org"
access_token = "YOUR_MATRIX_TOKEN"
room_id = "!roomid:matrix.org"
```

### Signal

```toml
[channels.signal]
base_url = "http://localhost:8080"          # signal-cli REST API endpoint
channel_id = "+1234567890"                  # your Signal phone number
```

Requires a running [signal-cli REST API](https://github.com/bbernhard/signal-cli-rest-api) daemon.

### WhatsApp

```toml
[channels.whatsapp]
access_token = "YOUR_WHATSAPP_ACCESS_TOKEN"
channel_id = "YOUR_PHONE_NUMBER_ID"         # from Meta Business Platform
```

Uses the WhatsApp Cloud API via Meta Business Platform.

### WhatsApp Web

```toml
[channels.whatsapp_web]
# session_path = "./whatsapp-session"       # optional session persistence
# pairing_mode = "qr"                       # "qr" or "code"
```

Connects via the WhatsApp Web multi-device protocol. On first run, scan the QR code or enter a pairing code.

### WhatsApp Storage

```toml
[channels.whatsapp_storage]
# session_path = "./whatsapp-storage"       # optional persistence path
```

In-memory message ring buffer for WhatsApp message persistence. Typically used alongside another WhatsApp channel.

### WATI

```toml
[channels.wati]
base_url = "https://live-server-XXXXX.wati.io"
token = "YOUR_WATI_API_TOKEN"
```

[WATI](https://www.wati.io/) is a WhatsApp Team Inbox platform.

### MQTT

```toml
[channels.mqtt]
base_url = "mqtt://localhost:1883"          # broker URL
channel_name = "agentzero/inbox"            # subscribe topic
# channel_id = "agentzero/outbox"           # publish topic (optional)
```

Connects to any MQTT broker. Subscribes to the configured topic for incoming messages and publishes responses.

### Transcription

```toml
[channels.transcription]
token = "YOUR_API_KEY"                      # Groq or Whisper-compatible API key
# base_url = "https://api.groq.com"         # API endpoint (default: Groq)
```

Transcribes audio input to text using a Whisper-compatible API.

### Linq

```toml
[channels.linq]
base_url = "https://api.linq.chat"
token = "YOUR_LINQ_API_KEY"
```

### NextCloud Talk

```toml
[channels.nextcloud_talk]
base_url = "https://your-nextcloud.example.com"
username = "bot-user"
password = "bot-password"
room_id = "YOUR_ROOM_TOKEN"
```

Uses the NextCloud Spreed OCS API.

### Email

```toml
[channels.email]
smtp_host = "smtp.gmail.com"
smtp_port = 587
imap_host = "imap.gmail.com"
imap_port = 993
username = "you@gmail.com"
password = "app-specific-password"         # use Gmail App Passwords
from_address = "you@gmail.com"
```

### Gmail Push

```toml
[channels.gmail_push]
access_token = "YOUR_GOOGLE_ACCESS_TOKEN"
# channel_id = "projects/my-project/topics/gmail"  # Pub/Sub topic
```

Uses Google Pub/Sub for real-time Gmail push notifications.

### IRC

```toml
[channels.irc]
server = "irc.libera.chat"
port = 6697
nick = "agentzero-bot"
channel_name = "#your-channel"
password = ""                              # NickServ password (optional)
```

### Lark

```toml
[channels.lark]
token = "YOUR_LARK_APP_ID"
app_token = "YOUR_LARK_APP_SECRET"
```

Create an app in the [Lark Developer Console](https://open.larksuite.com/).

### Feishu

```toml
[channels.feishu]
token = "YOUR_FEISHU_APP_ID"
app_token = "YOUR_FEISHU_APP_SECRET"
```

Same as Lark but for the China region (Feishu).

### DingTalk

```toml
[channels.dingtalk]
access_token = "YOUR_DINGTALK_TOKEN"
```

Uses DingTalk outgoing webhook integration.

### QQ Official

```toml
[channels.qq_official]
token = "YOUR_QQ_APP_ID"
bot_token = "YOUR_QQ_BOT_TOKEN"
```

Uses the QQ Bot Open Platform API.

### Nostr

```toml
[channels.nostr]
relay_url = "wss://relay.example.com"
private_key_hex = "YOUR_NOSTR_PRIVATE_KEY_HEX"
```

### ClawdTalk

```toml
[channels.clawdtalk]
base_url = "https://your-clawdtalk.example.com"
token = "YOUR_CLAWDTALK_API_KEY"
room_id = "YOUR_ROOM_ID"
```

Self-hosted chat platform.

### Voice Wake Word

```toml
[channels.voice_wake]
# wake_words = ["hey agent"]               # custom wake words
# channel_id = "0.6"                       # energy threshold (0.0-1.0)
```

Listens for a wake word via audio energy detection, then transcribes speech with Whisper.

### Webhook

```toml
[channels.webhook]
base_url = "http://localhost:8080/webhook"
```

The webhook channel sends agent responses as HTTP POST requests to the configured URL.

### Napcat (QQ via OneBot)

```toml
[channels.napcat]
base_url = "http://localhost:3000"          # Napcat OneBot v11 HTTP endpoint
access_token = "YOUR_ACCESS_TOKEN"          # optional
```

Uses the OneBot v11 HTTP API via [Napcat](https://github.com/NapNeko/NapCatQQ).

### ACP (Agent Client Protocol)

```toml
[channels.acp]
base_url = "http://localhost:9000"
channel_id = "my-agent"                     # agent ID
# token = "YOUR_API_KEY"                    # optional authentication
```

Agent-to-agent communication channel using the Agent Client Protocol.

### SMS

```toml
[channels.sms]
token = "YOUR_TWILIO_AUTH_TOKEN"
channel_id = "YOUR_TWILIO_ACCOUNT_SID"      # Account SID
# from_number = "+15550001234"              # Twilio sending number (E.164)
```

Uses the Twilio REST API for SMS.

---

## Global Channel Settings

These settings apply to all channels:

```toml
[channels_config]
message_timeout_secs = 300                 # max time to process a message
stream_mode = "off"                        # off | partial | full
draft_update_interval_ms = 500             # interval for draft updates (streaming)
interrupt_on_new_message = false           # cancel current response on new message
```

### Group Reply Behavior

Control how the bot responds in group chats:

```toml
[channels_config.group_reply.telegram]
mode = "mention_only"                      # all_messages | mention_only
bot_name = "MyBot"                         # triggers on @MyBot mentions
allowed_sender_ids = []                    # empty = allow all
```

### Acknowledgment Reactions

Send a reaction emoji when a message is received (before the response is ready):

```toml
[channels_config.ack_reaction.telegram]
enabled = true
emoji_pool = ["👍", "👀", "🤔"]
strategy = "random"                        # random | first | round_robin
sample_rate = 1.0                          # probability of sending ack (0.0-1.0)
```

Conditional reactions based on message content:

```toml
[[channels_config.ack_reaction.telegram.rules]]
contains_any = ["urgent", "asap"]
emoji_override = ["🚨"]
```

---

## Privacy Boundaries

Each channel can enforce a privacy boundary that restricts which tools and data flows are allowed:

| Mode | Effect |
|---|---|
| `""` (empty) | No restrictions — inherits from global config |
| `"local_only"` | Blocks all outbound network tools (web search, HTTP, etc.) |
| `"encrypted_only"` | Requires encrypted transport for all operations |

```toml
[channels.telegram]
bot_token = "..."
privacy_boundary = "local_only"            # this channel can only use local tools
```

---

## Outbound Leak Guard

The leak guard prevents sensitive data (API keys, tokens, passwords) from being sent through channels:

```toml
[security.outbound_leak_guard]
enabled = true
action = "redact"                          # redact | block | warn
sensitivity = 0.7                          # detection threshold (0.0-1.0)
```

| Action | Behavior |
|---|---|
| `redact` | Replace detected secrets with `[REDACTED]` |
| `block` | Drop the entire message |
| `warn` | Send the message but log a warning |

---

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| Channel not starting | Missing token | Check `bot_token` / `access_token` in config |
| Bot not responding in groups | Group reply mode | Set `mode = "all_messages"` or mention the bot |
| Messages timing out | Slow LLM response | Increase `message_timeout_secs` |
| Sensitive data in responses | Leak guard disabled | Enable `[security.outbound_leak_guard]` |
| Channel not in `channel list` | Feature not compiled | Build with `--features channels-standard` |
