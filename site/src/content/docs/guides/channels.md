---
title: Channel Integrations
description: Connect AgentZero to messaging platforms — Telegram, Discord, Slack, Matrix, Email, IRC, and more.
---

Channels connect the agent to messaging platforms. Each channel runs as a listener that forwards messages to the agent loop and sends responses back to the platform.

## Supported Channels

| Channel | Config Key | Transport | Notes |
|---|---|---|---|
| **Telegram** | `channels.telegram` | Bot API (polling) | Supports groups, inline queries |
| **Discord** | `channels.discord` | Gateway WebSocket | Supports guilds, threads |
| **Slack** | `channels.slack` | Socket Mode | Requires bot + app tokens |
| **Mattermost** | `channels.mattermost` | WebSocket | Self-hosted or cloud |
| **Matrix** | `channels.matrix` | Client-Server API | Federated, E2EE-capable |
| **Email** | `channels.email` | SMTP + IMAP | Send and receive email |
| **IRC** | `channels.irc` | TLS socket | Any IRC network |
| **Nostr** | `channels.nostr` | Relay WebSocket | Decentralized |
| **Webhook** | `channels.webhook` | HTTP POST | Generic HTTP integration |

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
agentzero channel enable telegram   # Enable a channel
agentzero channel disable telegram  # Disable a channel
agentzero channel test telegram     # Send a test message
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

### Matrix

```toml
[channels.matrix]
homeserver = "https://matrix.org"
access_token = "YOUR_MATRIX_TOKEN"
room_id = "!roomid:matrix.org"
```

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

### IRC

```toml
[channels.irc]
server = "irc.libera.chat"
port = 6697
nick = "agentzero-bot"
channel_name = "#your-channel"
password = ""                              # NickServ password (optional)
```

### Nostr

```toml
[channels.nostr]
relay_url = "wss://relay.example.com"
private_key_hex = "YOUR_NOSTR_PRIVATE_KEY_HEX"
```

### Webhook

```toml
[channels.webhook]
base_url = "http://localhost:8080/webhook"
```

The webhook channel sends agent responses as HTTP POST requests to the configured URL.

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
