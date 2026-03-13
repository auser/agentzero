# Plan 11: WhatsApp and SMS Channels

## Context

Two new messaging channels:

1. **WhatsApp Cloud API (Meta)** — A full implementation already exists in `whatsapp.rs` (Graph API v18.0). It is not yet wired into the config-driven registration pipeline (`channel_setup.rs`). No channel code changes needed — just wiring.
2. **SMS via Twilio** — No SMS channel exists. New `sms.rs` follows the established channel pattern (same as `telegram.rs` / `whatsapp.rs`), plus feature-gating, catalog entry, and config wiring.

---

## Part 1: Wire WhatsApp Cloud API

### Files
- `crates/agentzero-channels/src/channels/channel_setup.rs`

### Changes

Add `"whatsapp"` arm in `register_one()` (after `"nostr"`, before `_ => Ok(false)`):

```rust
#[cfg(feature = "channel-whatsapp")]
"whatsapp" => {
    let access_token = config
        .access_token
        .as_ref()
        .ok_or("whatsapp requires access_token")?;
    let phone_number_id = config
        .channel_id
        .as_ref()
        .ok_or("whatsapp requires channel_id (phone_number_id)")?;
    let verify_token = config.token.clone().unwrap_or_default();
    let channel = super::WhatsappChannel::new(
        access_token.clone(),
        phone_number_id.clone(),
        verify_token,
        config.allowed_users.clone(),
    );
    registry.register(Arc::new(channel));
    Ok(true)
}
```

Config field mapping:

| `ChannelInstanceConfig` field | WhatsApp parameter |
|---|---|
| `access_token` | `access_token` |
| `channel_id` | `phone_number_id` |
| `token` | `verify_token` (optional, defaults to `""`) |
| `allowed_users` | `allowed_users` |

Add 2 tests: missing `access_token` returns error; valid config registers the channel.

---

## Part 2: New Twilio SMS Channel

### 2a. New config fields — `channel_setup.rs`

Add to `ChannelInstanceConfig`:

```rust
pub account_sid: Option<String>,
pub from_number: Option<String>,
```

### 2b. New file: `crates/agentzero-channels/src/channels/sms.rs`

Follows the same `channel_meta!` / `impl_` pattern as `whatsapp.rs`.

**Struct:**
```
SmsChannel {
    account_sid: String,
    auth_token: String,       // from config.token
    from_number: String,      // from config.from_number
    allowed_numbers: Vec<String>,  // from config.allowed_users
    client: reqwest::Client,
}
```

**`send()`** — POST to Twilio REST API:
- URL: `https://api.twilio.com/2010-04-01/Accounts/{account_sid}/Messages.json`
- Auth: HTTP Basic (account_sid, auth_token)
- Body: `application/x-www-form-urlencoded` with `To`, `From`, `Body`
- Chunk at 1600 chars (Twilio concatenated SMS limit)

**`listen()`** — webhook stub (same pattern as WhatsApp):
- Log info: webhook registration required
- Loop with sleep to keep listener alive

**`health_check()`** — GET `https://api.twilio.com/2010-04-01/Accounts/{account_sid}.json` with Basic auth

**Tests:**
- Channel name is `"sms"`
- API URL construction
- Missing `account_sid` → registration error
- Valid config → registers successfully

### 2c. `Cargo.toml` — feature flag

```toml
channel-sms = ["reqwest"]
```

Add `channel-sms` to both `channels-standard` and `all-channels`.

### 2d. `channels/mod.rs` — catalog entry

```rust
sms => (SmsChannel, SMS_DESCRIPTOR),
```

### 2e. `channel_setup.rs` — registration arm

```rust
#[cfg(feature = "channel-sms")]
"sms" => {
    let account_sid = config
        .account_sid
        .as_ref()
        .ok_or("sms requires account_sid")?;
    let auth_token = config
        .token
        .as_ref()
        .ok_or("sms requires token (auth_token)")?;
    let from_number = config
        .from_number
        .as_ref()
        .ok_or("sms requires from_number")?;
    let channel = super::SmsChannel::new(
        account_sid.clone(),
        auth_token.clone(),
        from_number.clone(),
        config.allowed_users.clone(),
    );
    registry.register(Arc::new(channel));
    Ok(true)
}
```

---

## TOML Config Reference

```toml
[channels.whatsapp]
access_token = "EAAxxxxxxx"
channel_id = "1234567890"        # phone_number_id from Meta Business console
token = "my-verify-token"        # optional webhook verify token
allowed_users = ["+1234567890"]  # E.164 format; empty = allow all

[channels.sms]
account_sid = "ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
token = "your_auth_token"
from_number = "+15551234567"     # Your Twilio number
allowed_users = ["+15559876543"] # E.164 format; empty = allow all
```

---

## Critical Files

| File | Change |
|---|---|
| `crates/agentzero-channels/src/channels/channel_setup.rs` | Add `account_sid`, `from_number` fields; add `"whatsapp"` and `"sms"` arms + tests |
| `crates/agentzero-channels/src/channels/sms.rs` | **New** — Twilio SMS channel implementation |
| `crates/agentzero-channels/src/channels/mod.rs` | Add `sms` to `channel_catalog!` |
| `crates/agentzero-channels/Cargo.toml` | Add `channel-sms`; include in `channels-standard` + `all-channels` |

`whatsapp.rs` — no changes needed (implementation complete).

---

## Verification

```bash
# Compile both channels
cargo check -p agentzero-channels --features channel-whatsapp,channel-sms

# Tests
cargo test -p agentzero-channels --features channel-whatsapp,channel-sms
cargo test -p agentzero-channels --features channels-standard
cargo test -p agentzero-channels --features all-channels

# Clippy — zero warnings
cargo clippy -p agentzero-channels --features channel-whatsapp,channel-sms -- -D warnings
```
