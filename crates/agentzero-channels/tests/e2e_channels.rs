//! End-to-end tests for channel implementations.
//!
//! Each test spins up a local mock HTTP server (wiremock) that mimics the
//! real platform API, then exercises the channel's `send()` and (where
//! applicable) `listen()` methods against it.
//!
//! Run with:
//!   cargo test -p agentzero-channels --features channels-standard,tls-rustls --test e2e_channels

#[allow(unused_imports)]
use std::sync::Arc;
#[allow(unused_imports)]
use tokio::sync::mpsc;
#[allow(unused_imports)]
use wiremock::matchers::{header, method, path, path_regex};
#[allow(unused_imports)]
use wiremock::{Mock, MockServer, ResponseTemplate};

#[allow(unused_imports)]
use agentzero_channels::{Channel, SendMessage};

// ---------------------------------------------------------------------------
// Telegram
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-telegram")]
mod telegram {
    use super::*;
    use agentzero_channels::channels::TelegramChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/bot.+/sendMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": {"message_id": 1}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = TelegramChannel::new("test:token".into(), vec![])
            .with_base_url(format!("{}/bot", server.uri()));

        let msg = SendMessage::new("hello from test", "12345");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn send_long_message_splits() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/bot.+/sendMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": {"message_id": 1}
            })))
            .expect(2)
            .mount(&server)
            .await;

        let ch = TelegramChannel::new("test:token".into(), vec![])
            .with_base_url(format!("{}/bot", server.uri()));

        // 4096 is max; send something over the limit to trigger split
        let long_text = "a".repeat(5000);
        let msg = SendMessage::new(long_text, "12345");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn listen_receives_updates() {
        let server = MockServer::start().await;

        // First poll returns one update, second poll returns empty (we abort after)
        Mock::given(method("POST"))
            .and(path_regex(r"/bot.+/getUpdates"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": [{
                    "update_id": 100,
                    "message": {
                        "message_id": 1,
                        "from": {"id": 42, "first_name": "Test"},
                        "chat": {"id": 42},
                        "text": "hello agent",
                        "date": 1700000000
                    }
                }]
            })))
            .mount(&server)
            .await;

        let ch = Arc::new(
            TelegramChannel::new("test:token".into(), vec![])
                .with_base_url(format!("{}/bot", server.uri())),
        );

        let (tx, mut rx) = mpsc::channel(16);
        let ch_clone = ch.clone();
        let handle = tokio::spawn(async move {
            let _ = ch_clone.listen(tx).await;
        });

        let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("should receive a message");

        assert_eq!(received.content, "hello agent");
        assert_eq!(received.channel, "telegram");
        assert_eq!(received.sender, "42");
        assert_eq!(received.reply_target, "42");

        handle.abort();
    }

    #[tokio::test]
    async fn listen_filters_unallowed_users() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/bot.+/getUpdates"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": [
                    {
                        "update_id": 100,
                        "message": {
                            "message_id": 1,
                            "from": {"id": 999, "first_name": "Intruder"},
                            "chat": {"id": 999},
                            "text": "should be filtered",
                            "date": 1700000000
                        }
                    },
                    {
                        "update_id": 101,
                        "message": {
                            "message_id": 2,
                            "from": {"id": 42, "first_name": "Allowed"},
                            "chat": {"id": 42},
                            "text": "allowed message",
                            "date": 1700000001
                        }
                    }
                ]
            })))
            .mount(&server)
            .await;

        let ch = Arc::new(
            TelegramChannel::new("test:token".into(), vec!["42".into()])
                .with_base_url(format!("{}/bot", server.uri())),
        );

        let (tx, mut rx) = mpsc::channel(16);
        let ch_clone = ch.clone();
        let handle = tokio::spawn(async move {
            let _ = ch_clone.listen(tx).await;
        });

        let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("should receive a message");

        assert_eq!(received.content, "allowed message");
        assert_eq!(received.sender, "42");

        handle.abort();
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/bot.+/getMe"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": {"id": 123, "first_name": "TestBot", "is_bot": true}
            })))
            .mount(&server)
            .await;

        let ch = TelegramChannel::new("test:token".into(), vec![])
            .with_base_url(format!("{}/bot", server.uri()));

        assert!(ch.health_check().await);
    }

    #[tokio::test]
    async fn start_typing() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/bot.+/sendChatAction"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .expect(1)
            .mount(&server)
            .await;

        let ch = TelegramChannel::new("test:token".into(), vec![])
            .with_base_url(format!("{}/bot", server.uri()));

        ch.start_typing("12345")
            .await
            .expect("typing should succeed");
    }
}

// ---------------------------------------------------------------------------
// Discord (send-side only — listen uses WebSocket)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-discord")]
mod discord {
    use super::*;
    use agentzero_channels::channels::DiscordChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/channels/.+/messages"))
            .and(header("Authorization", "Bot test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "msg1",
                "content": "hello"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = DiscordChannel::new("test-token".into(), vec![]).with_base_url(server.uri());

        let msg = SendMessage::new("hello from test", "channel123");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn send_long_message_splits() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/channels/.+/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "msg1",
                "content": "chunk"
            })))
            .expect(2)
            .mount(&server)
            .await;

        let ch = DiscordChannel::new("test-token".into(), vec![]).with_base_url(server.uri());

        // Discord max is 2000
        let long_text = "b".repeat(3000);
        let msg = SendMessage::new(long_text, "channel123");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn send_fails_on_error_response() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/channels/.+/messages"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
            .mount(&server)
            .await;

        let ch = DiscordChannel::new("bad-token".into(), vec![]).with_base_url(server.uri());

        let msg = SendMessage::new("should fail", "channel123");
        let result = ch.send(&msg).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("discord send failed"));
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/users/@me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "123",
                "username": "TestBot"
            })))
            .mount(&server)
            .await;

        let ch = DiscordChannel::new("test-token".into(), vec![]).with_base_url(server.uri());

        assert!(ch.health_check().await);
    }

    #[tokio::test]
    async fn health_check_fails_on_bad_token() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/users/@me"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let ch = DiscordChannel::new("bad-token".into(), vec![]).with_base_url(server.uri());

        assert!(!ch.health_check().await);
    }

    #[tokio::test]
    async fn start_typing() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/channels/.+/typing"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let ch = DiscordChannel::new("test-token".into(), vec![]).with_base_url(server.uri());

        ch.start_typing("channel123")
            .await
            .expect("typing should succeed");
    }
}

// ---------------------------------------------------------------------------
// Slack (send + polling listen — skips socket mode which needs WebSocket)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-slack")]
mod slack {
    use super::*;
    use agentzero_channels::channels::SlackChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "1234567890.123456"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch =
            SlackChannel::new("xoxb-test".into(), None, None, vec![]).with_base_url(server.uri());

        let msg = SendMessage::new("hello slack", "C123");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn send_with_thread_ts() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "1234567890.123456"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch =
            SlackChannel::new("xoxb-test".into(), None, None, vec![]).with_base_url(server.uri());

        let mut msg = SendMessage::new("reply in thread", "C123");
        msg.thread_ts = Some("1234567890.000001".into());
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn listen_polling_mode() {
        let server = MockServer::start().await;

        // Mock auth.test to get bot user ID
        Mock::given(method("POST"))
            .and(path("/auth.test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "user_id": "UBOT123"
            })))
            .mount(&server)
            .await;

        // Mock conversations.history with one message
        Mock::given(method("GET"))
            .and(path("/conversations.history"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [{
                    "user": "U456",
                    "text": "hello from slack",
                    "ts": "1700000001.000001"
                }]
            })))
            .mount(&server)
            .await;

        let ch = Arc::new(
            SlackChannel::new("xoxb-test".into(), None, Some("C123".into()), vec![])
                .with_base_url(server.uri()),
        );

        let (tx, mut rx) = mpsc::channel(16);
        let ch_clone = ch.clone();
        let handle = tokio::spawn(async move {
            let _ = ch_clone.listen(tx).await;
        });

        let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("should receive a message");

        assert_eq!(received.content, "hello from slack");
        assert_eq!(received.channel, "slack");
        assert_eq!(received.sender, "U456");

        handle.abort();
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/auth.test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "user_id": "UBOT"
            })))
            .mount(&server)
            .await;

        let ch =
            SlackChannel::new("xoxb-test".into(), None, None, vec![]).with_base_url(server.uri());

        assert!(ch.health_check().await);
    }
}

// ---------------------------------------------------------------------------
// Signal (already has configurable base_url)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-signal")]
mod signal {
    use super::*;
    use agentzero_channels::channels::SignalChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/v2/send"))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        let ch = SignalChannel::new(server.uri(), "+15551234567".into(), vec![]);

        let msg = SendMessage::new("hello signal", "+15559876543");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn listen_receives_messages() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/v1/receive/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "envelope": {
                        "sourceNumber": "+15559876543",
                        "timestamp": 1700000000000_i64,
                        "dataMessage": {
                            "message": "hello from signal",
                            "timestamp": 1700000000000_i64
                        }
                    }
                }
            ])))
            .mount(&server)
            .await;

        let ch = Arc::new(SignalChannel::new(
            server.uri(),
            "+15551234567".into(),
            vec![],
        ));

        let (tx, mut rx) = mpsc::channel(16);
        let ch_clone = ch.clone();
        let handle = tokio::spawn(async move {
            let _ = ch_clone.listen(tx).await;
        });

        let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("should receive a message");

        assert_eq!(received.content, "hello from signal");
        assert_eq!(received.channel, "signal");

        handle.abort();
    }
}

// ---------------------------------------------------------------------------
// Mattermost (already has configurable base_url)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-mattermost")]
mod mattermost {
    use super::*;
    use agentzero_channels::channels::MattermostChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v4/posts"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "id": "post1",
                "message": "hello"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = MattermostChannel::new(
            server.uri(),
            "test-token".into(),
            Some("channel123".into()),
            vec![],
        );

        let msg = SendMessage::new("hello mattermost", "channel123");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn listen_receives_posts() {
        let server = MockServer::start().await;

        // create_at must be > last_post_time in the listener (now_epoch_secs * 1000).
        // Use a large offset to avoid race between test setup and listener start.
        let future_ts = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_millis() as u64)
            + 30_000;

        // Mock posts endpoint
        Mock::given(method("GET"))
            .and(path_regex(r"/api/v4/channels/.+/posts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "order": ["post1"],
                "posts": {
                    "post1": {
                        "id": "post1",
                        "user_id": "user1",
                        "channel_id": "channel123",
                        "message": "hello from mattermost",
                        "create_at": future_ts
                    }
                }
            })))
            .mount(&server)
            .await;

        // Mock users/me
        Mock::given(method("GET"))
            .and(path("/api/v4/users/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "bot_user"
            })))
            .mount(&server)
            .await;

        let ch = Arc::new(MattermostChannel::new(
            server.uri(),
            "test-token".into(),
            Some("channel123".into()),
            vec![],
        ));

        let (tx, mut rx) = mpsc::channel(16);
        let ch_clone = ch.clone();
        let handle = tokio::spawn(async move {
            let _ = ch_clone.listen(tx).await;
        });

        let received = tokio::time::timeout(std::time::Duration::from_secs(15), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("should receive a message");

        assert_eq!(received.content, "hello from mattermost");
        assert_eq!(received.channel, "mattermost");

        handle.abort();
    }
}

// ---------------------------------------------------------------------------
// Matrix (already has configurable homeserver)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-matrix")]
mod matrix {
    use super::*;
    use agentzero_channels::channels::MatrixChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("PUT"))
            .and(path_regex(
                r"/_matrix/client/v3/rooms/.+/send/m.room.message/.+",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "event_id": "$event1"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = MatrixChannel::new(
            server.uri(),
            "test-access-token".into(),
            "!room:test.com".into(),
            vec![],
        );

        let msg = SendMessage::new("hello matrix", "!room:test.com");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn listen_receives_events() {
        let server = MockServer::start().await;

        // Mock /sync endpoint
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "next_batch": "batch2",
                "rooms": {
                    "join": {
                        "!room:test.com": {
                            "timeline": {
                                "events": [{
                                    "type": "m.room.message",
                                    "sender": "@user:test.com",
                                    "event_id": "$event1",
                                    "content": {
                                        "msgtype": "m.text",
                                        "body": "hello from matrix"
                                    },
                                    "origin_server_ts": 1700000000000_i64
                                }]
                            }
                        }
                    }
                }
            })))
            .mount(&server)
            .await;

        // Mock whoami to get bot user
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/account/whoami"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "user_id": "@bot:test.com"
            })))
            .mount(&server)
            .await;

        let ch = Arc::new(MatrixChannel::new(
            server.uri(),
            "test-access-token".into(),
            "!room:test.com".into(),
            vec![],
        ));

        let (tx, mut rx) = mpsc::channel(16);
        let ch_clone = ch.clone();
        let handle = tokio::spawn(async move {
            let _ = ch_clone.listen(tx).await;
        });

        let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("should receive a message");

        assert_eq!(received.content, "hello from matrix");
        assert_eq!(received.channel, "matrix");

        handle.abort();
    }
}

// ---------------------------------------------------------------------------
// WhatsApp (send only — listen is webhook-based)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-whatsapp")]
mod whatsapp {
    use super::*;
    use agentzero_channels::channels::WhatsappChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/.+/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "messages": [{"id": "wamid.123"}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = WhatsappChannel::new(
            "test-token".into(),
            "12345".into(),
            "verify-tok".into(),
            vec![],
        )
        .with_base_url(server.uri());

        let msg = SendMessage::new("hello whatsapp", "+15559876543");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn send_fails_on_error() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/.+/messages"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
            .mount(&server)
            .await;

        let ch = WhatsappChannel::new(
            "bad-token".into(),
            "12345".into(),
            "verify-tok".into(),
            vec![],
        )
        .with_base_url(server.uri());

        let msg = SendMessage::new("should fail", "+15559876543");
        let result = ch.send(&msg).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/12345"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "12345"})),
            )
            .mount(&server)
            .await;

        let ch = WhatsappChannel::new(
            "test-token".into(),
            "12345".into(),
            "verify-tok".into(),
            vec![],
        )
        .with_base_url(server.uri());

        assert!(ch.health_check().await);
    }
}

// ---------------------------------------------------------------------------
// SMS / Twilio (send only — listen is webhook-based)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-sms")]
mod sms {
    use super::*;
    use agentzero_channels::channels::SmsChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/Accounts/.+/Messages.json"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "sid": "SM123",
                "status": "queued"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = SmsChannel::new(
            "AC_test_sid".into(),
            "auth_token_123".into(),
            "+15551234567".into(),
            vec![],
        )
        .with_base_url(server.uri());

        let msg = SendMessage::new("hello sms", "+15559876543");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn send_long_message_splits() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/Accounts/.+/Messages.json"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "sid": "SM123",
                "status": "queued"
            })))
            .expect(2)
            .mount(&server)
            .await;

        let ch = SmsChannel::new(
            "AC_test_sid".into(),
            "auth_token_123".into(),
            "+15551234567".into(),
            vec![],
        )
        .with_base_url(server.uri());

        // Twilio max is 1600
        let long_text = "x".repeat(2000);
        let msg = SendMessage::new(long_text, "+15559876543");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/Accounts/.+\.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sid": "AC_test_sid",
                "status": "active"
            })))
            .mount(&server)
            .await;

        let ch = SmsChannel::new(
            "AC_test_sid".into(),
            "auth_token_123".into(),
            "+15551234567".into(),
            vec![],
        )
        .with_base_url(server.uri());

        assert!(ch.health_check().await);
    }
}

// ---------------------------------------------------------------------------
// Webhook (local channel, no mock server needed)
// ---------------------------------------------------------------------------

mod webhook {
    use super::*;
    use agentzero_channels::channels::WebhookChannel;
    use agentzero_channels::ChannelMessage;

    #[tokio::test]
    async fn inject_and_receive() {
        let ch = Arc::new(WebhookChannel::new());
        let (tx, mut rx) = mpsc::channel(16);

        let ch_clone = ch.clone();
        let handle = tokio::spawn(async move {
            ch_clone.listen(tx).await.expect("listen should succeed");
        });

        let msg = ChannelMessage {
            id: "test-id".into(),
            sender: "webhook-sender".into(),
            reply_target: "webhook-sender".into(),
            content: "webhook payload".into(),
            channel: "webhook".into(),
            timestamp: 1700000000,
            thread_ts: None,
            privacy_boundary: String::new(),
            attachments: Vec::new(),
        };

        ch.inject_message(msg).await.expect("inject should succeed");

        let received = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("should receive a message");

        assert_eq!(received.content, "webhook payload");
        assert_eq!(received.channel, "webhook");
        assert_eq!(received.sender, "webhook-sender");

        handle.abort();
    }

    #[tokio::test]
    async fn send_is_noop() {
        let ch = WebhookChannel::new();
        let msg = SendMessage::new("test", "recipient");
        ch.send(&msg).await.expect("send (no-op) should succeed");
    }

    #[tokio::test]
    async fn double_listen_fails() {
        let ch = Arc::new(WebhookChannel::new());
        let (tx1, _rx1) = mpsc::channel(16);
        let (tx2, _rx2) = mpsc::channel(16);

        let ch_clone = ch.clone();
        let _handle = tokio::spawn(async move {
            let _ = ch_clone.listen(tx1).await;
        });

        // Give the first listener time to start
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let result = ch.listen(tx2).await;
        assert!(result.is_err());
    }
}

// ---------------------------------------------------------------------------
// CLI channel (basic tests — stdin/stdout not exercised in e2e)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-cli")]
mod cli {
    use agentzero_channels::channels::CliChannel;
    use agentzero_channels::Channel;

    #[test]
    fn channel_name() {
        let ch = CliChannel;
        assert_eq!(ch.name(), "cli");
    }
}

// ---------------------------------------------------------------------------
// ClawdTalk (already has configurable base_url)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-clawdtalk")]
mod clawdtalk {
    use super::*;
    use agentzero_channels::channels::ClawdtalkChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .expect(1)
            .mount(&server)
            .await;

        let ch = ClawdtalkChannel::new(server.uri(), "test-key".into(), "room1".into(), vec![]);

        let msg = SendMessage::new("hello clawdtalk", "room1");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn listen_receives_messages() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/api/v1/messages/stream"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "cursor": "cursor2",
                "messages": [{
                    "sender": "user1",
                    "text": "hello from clawdtalk"
                }]
            })))
            .mount(&server)
            .await;

        let ch = Arc::new(ClawdtalkChannel::new(
            server.uri(),
            "test-key".into(),
            "room1".into(),
            vec![],
        ));

        let (tx, mut rx) = mpsc::channel(16);
        let ch_clone = ch.clone();
        let handle = tokio::spawn(async move {
            let _ = ch_clone.listen(tx).await;
        });

        let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("should receive a message");

        assert_eq!(received.content, "hello from clawdtalk");
        assert_eq!(received.channel, "clawdtalk");

        handle.abort();
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/v1/health"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let ch = ClawdtalkChannel::new(server.uri(), "test-key".into(), "room1".into(), vec![]);

        assert!(ch.health_check().await);
    }
}

// ---------------------------------------------------------------------------
// NapCat (OneBot, already has configurable base_url)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-napcat")]
mod napcat {
    use super::*;
    use agentzero_channels::channels::NapcatChannel;

    #[tokio::test]
    async fn send_private_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/send_private_msg"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": "ok"})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let ch = NapcatChannel::new(server.uri(), None, vec![]);

        let msg = SendMessage::new("hello napcat", "user:12345");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn send_group_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/send_group_msg"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": "ok"})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let ch = NapcatChannel::new(server.uri(), None, vec![]);

        let msg = SendMessage::new("hello group", "group:67890");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn listen_receives_events() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/get_latest_events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "post_type": "message",
                    "sender": {"user_id": 42},
                    "raw_message": "hello from napcat",
                    "message_type": "private",
                    "group_id": null
                }]
            })))
            .mount(&server)
            .await;

        let ch = Arc::new(NapcatChannel::new(server.uri(), None, vec![]));

        let (tx, mut rx) = mpsc::channel(16);
        let ch_clone = ch.clone();
        let handle = tokio::spawn(async move {
            let _ = ch_clone.listen(tx).await;
        });

        let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("should receive a message");

        assert_eq!(received.content, "hello from napcat");
        assert_eq!(received.channel, "napcat");

        handle.abort();
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/get_status"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": "ok"})),
            )
            .mount(&server)
            .await;

        let ch = NapcatChannel::new(server.uri(), None, vec![]);

        assert!(ch.health_check().await);
    }
}

// ---------------------------------------------------------------------------
// Nextcloud Talk (already has configurable base_url)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-nextcloud-talk")]
mod nextcloud_talk {
    use super::*;
    use agentzero_channels::channels::NextcloudTalkChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/ocs/v2.php/apps/spreed/api/v1/chat/.+"))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        let ch = NextcloudTalkChannel::new(
            server.uri(),
            "admin".into(),
            "password".into(),
            "room-tok".into(),
            vec![],
        );

        let msg = SendMessage::new("hello nextcloud", "room-tok");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn listen_receives_messages() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/ocs/v2.php/apps/spreed/api/v1/chat/.+"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ocs": {
                    "data": [{
                        "id": 100,
                        "actorId": "user1",
                        "message": "hello from nextcloud"
                    }]
                }
            })))
            .mount(&server)
            .await;

        let ch = Arc::new(NextcloudTalkChannel::new(
            server.uri(),
            "admin".into(),
            "password".into(),
            "room-tok".into(),
            vec![],
        ));

        let (tx, mut rx) = mpsc::channel(16);
        let ch_clone = ch.clone();
        let handle = tokio::spawn(async move {
            let _ = ch_clone.listen(tx).await;
        });

        let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("should receive a message");

        assert_eq!(received.content, "hello from nextcloud");
        assert_eq!(received.channel, "nextcloud-talk");

        handle.abort();
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/ocs/v2.php/apps/spreed/api/v1/room/.+"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let ch = NextcloudTalkChannel::new(
            server.uri(),
            "admin".into(),
            "password".into(),
            "room-tok".into(),
            vec![],
        );

        assert!(ch.health_check().await);
    }
}

// ---------------------------------------------------------------------------
// Linq (already has configurable base_url)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-linq")]
mod linq {
    use super::*;
    use agentzero_channels::channels::LinqChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .expect(1)
            .mount(&server)
            .await;

        let ch = LinqChannel::new(server.uri(), "test-key".into(), vec![]);

        let msg = SendMessage::new("hello linq", "recipient1");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn listen_receives_messages() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/api/v1/messages/poll"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "cursor": "c2",
                "messages": [{
                    "from": "sender1",
                    "text": "hello from linq"
                }]
            })))
            .mount(&server)
            .await;

        let ch = Arc::new(LinqChannel::new(server.uri(), "test-key".into(), vec![]));

        let (tx, mut rx) = mpsc::channel(16);
        let ch_clone = ch.clone();
        let handle = tokio::spawn(async move {
            let _ = ch_clone.listen(tx).await;
        });

        let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("should receive a message");

        assert_eq!(received.content, "hello from linq");
        assert_eq!(received.channel, "linq");

        handle.abort();
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/v1/health"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let ch = LinqChannel::new(server.uri(), "test-key".into(), vec![]);

        assert!(ch.health_check().await);
    }
}

// ---------------------------------------------------------------------------
// WATI (already has configurable base_url, send only)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-wati")]
mod wati {
    use super::*;
    use agentzero_channels::channels::WatiChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/api/v1/sendSessionMessage/.+"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": true})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let ch = WatiChannel::new(server.uri(), "test-token".into(), vec![]);

        let msg = SendMessage::new("hello wati", "+15559876543");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/v1/getContacts"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": true})),
            )
            .mount(&server)
            .await;

        let ch = WatiChannel::new(server.uri(), "test-token".into(), vec![]);

        assert!(ch.health_check().await);
    }
}

// ---------------------------------------------------------------------------
// ACP — Agent Client Protocol (already has configurable base_url)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-acp")]
mod acp {
    use super::*;
    use agentzero_channels::channels::AcpChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .expect(1)
            .mount(&server)
            .await;

        let ch = AcpChannel::new(
            server.uri(),
            "agent-1".into(),
            Some("key-123".into()),
            vec![],
        );

        let msg = SendMessage::new("hello acp", "agent-2");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn listen_receives_messages() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/messages/receive"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "messages": [{
                    "from": "agent-2",
                    "content": "hello from acp"
                }]
            })))
            .mount(&server)
            .await;

        let ch = Arc::new(AcpChannel::new(
            server.uri(),
            "agent-1".into(),
            Some("key-123".into()),
            vec![],
        ));

        let (tx, mut rx) = mpsc::channel(16);
        let ch_clone = ch.clone();
        let handle = tokio::spawn(async move {
            let _ = ch_clone.listen(tx).await;
        });

        let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("should receive a message");

        assert_eq!(received.content, "hello from acp");
        assert_eq!(received.channel, "acp");

        handle.abort();
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let ch = AcpChannel::new(
            server.uri(),
            "agent-1".into(),
            Some("key-123".into()),
            vec![],
        );

        assert!(ch.health_check().await);
    }
}

// ---------------------------------------------------------------------------
// Lark (newly configurable base_url, send + health only)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-lark")]
mod lark {
    use super::*;
    use agentzero_channels::channels::LarkChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        // Mock tenant token endpoint
        Mock::given(method("POST"))
            .and(path("/open-apis/auth/v3/tenant_access_token/internal"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "tenant_access_token": "test-tenant-token",
                "expire": 7200
            })))
            .mount(&server)
            .await;

        // Mock message send
        Mock::given(method("POST"))
            .and(path("/open-apis/im/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": {"message_id": "om_123"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = LarkChannel::new("app-id".into(), "app-secret".into(), vec![])
            .with_base_url(server.uri());

        let msg = SendMessage::new("hello lark", "oc_chat123");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/open-apis/auth/v3/tenant_access_token/internal"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "tenant_access_token": "test-token",
                "expire": 7200
            })))
            .mount(&server)
            .await;

        let ch = LarkChannel::new("app-id".into(), "app-secret".into(), vec![])
            .with_base_url(server.uri());

        assert!(ch.health_check().await);
    }

    #[tokio::test]
    async fn health_check_fails_on_bad_credentials() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/open-apis/auth/v3/tenant_access_token/internal"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 10003,
                "msg": "invalid app_id"
            })))
            .mount(&server)
            .await;

        let ch = LarkChannel::new("bad-id".into(), "bad-secret".into(), vec![])
            .with_base_url(server.uri());

        assert!(!ch.health_check().await);
    }
}

// ---------------------------------------------------------------------------
// Feishu (newly configurable base_url, send + health only)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-feishu")]
mod feishu {
    use super::*;
    use agentzero_channels::channels::FeishuChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/open-apis/auth/v3/tenant_access_token/internal"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "tenant_access_token": "test-tenant-token",
                "expire": 7200
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/open-apis/im/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": {"message_id": "om_123"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = FeishuChannel::new("app-id".into(), "app-secret".into(), vec![])
            .with_base_url(server.uri());

        let msg = SendMessage::new("hello feishu", "oc_chat123");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/open-apis/auth/v3/tenant_access_token/internal"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "tenant_access_token": "test-token",
                "expire": 7200
            })))
            .mount(&server)
            .await;

        let ch = FeishuChannel::new("app-id".into(), "app-secret".into(), vec![])
            .with_base_url(server.uri());

        assert!(ch.health_check().await);
    }
}

// ---------------------------------------------------------------------------
// DingTalk (newly configurable base_url, send + health only)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-dingtalk")]
mod dingtalk {
    use super::*;
    use agentzero_channels::channels::DingtalkChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/robot/send"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "errcode": 0,
                "errmsg": "ok"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch =
            DingtalkChannel::new("test-token".into(), None, vec![]).with_base_url(server.uri());

        let msg = SendMessage::new("hello dingtalk", "ignored");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/robot/send"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "errcode": 0,
                "errmsg": "ok"
            })))
            .mount(&server)
            .await;

        let ch =
            DingtalkChannel::new("test-token".into(), None, vec![]).with_base_url(server.uri());

        assert!(ch.health_check().await);
    }
}

// ---------------------------------------------------------------------------
// QQ Official (newly configurable base_url, send + health only)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-qq-official")]
mod qq_official {
    use super::*;
    use agentzero_channels::channels::QqOfficialChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/channels/.+/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "msg1"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = QqOfficialChannel::new("app-id".into(), "bot-token".into(), false, vec![])
            .with_base_url(server.uri());

        let msg = SendMessage::new("hello qq", "channel123");
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/gateway"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "wss://api.sgroup.qq.com/websocket"
            })))
            .mount(&server)
            .await;

        let ch = QqOfficialChannel::new("app-id".into(), "bot-token".into(), false, vec![])
            .with_base_url(server.uri());

        assert!(ch.health_check().await);
    }

    #[tokio::test]
    async fn health_check_fails_on_401() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/gateway"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let ch = QqOfficialChannel::new("bad".into(), "bad".into(), false, vec![])
            .with_base_url(server.uri());

        assert!(!ch.health_check().await);
    }
}

// ---------------------------------------------------------------------------
// Gmail Push (complex OAuth — test send + health_check only)
// ---------------------------------------------------------------------------

#[cfg(feature = "channel-gmail-push")]
mod gmail_push {
    use super::*;
    use agentzero_channels::channels::GmailPushChannel;

    #[tokio::test]
    async fn send_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/gmail/v1/users/me/messages/send"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "msg-id-123",
                "threadId": "thread-123"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = GmailPushChannel::new(
            "test-access-token".into(),
            "test-project".into(),
            "projects/test/topics/test".into(),
        )
        .with_base_url(server.uri());

        let mut msg = SendMessage::new("hello gmail", "test@example.com");
        msg.subject = Some("Test Subject".into());
        ch.send(&msg).await.expect("send should succeed");
    }

    #[tokio::test]
    async fn health_check() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/profile"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "emailAddress": "bot@example.com"
            })))
            .mount(&server)
            .await;

        let ch = GmailPushChannel::new(
            "test-access-token".into(),
            "test-project".into(),
            "projects/test/topics/test".into(),
        )
        .with_base_url(server.uri());

        assert!(ch.health_check().await);
    }
}
