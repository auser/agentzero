#[cfg(feature = "channel-whatsapp-web")]
#[allow(dead_code)]
mod impl_ {
    use serde::{Deserialize, Serialize};
    use std::collections::VecDeque;
    use std::path::PathBuf;

    super::super::channel_meta!(
        WHATSAPP_STORAGE_DESCRIPTOR,
        "whatsapp-storage",
        "WhatsApp Storage"
    );

    /// Maximum messages to keep in the in-memory ring buffer per chat.
    const DEFAULT_HISTORY_LIMIT: usize = 500;

    /// A stored WhatsApp message.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct StoredMessage {
        /// WhatsApp message ID.
        pub message_id: String,
        /// Sender JID.
        pub sender: String,
        /// Chat JID (individual or group).
        pub chat: String,
        /// Message text content.
        pub content: String,
        /// Unix timestamp in seconds.
        pub timestamp: u64,
        /// Whether this is an outgoing message from the agent.
        pub is_outgoing: bool,
    }

    /// In-memory message history for a WhatsApp session.
    /// A real implementation would back this with SQLite or similar.
    #[derive(Debug)]
    pub struct WhatsappSessionStore {
        /// Session data directory for persistence.
        pub session_path: PathBuf,
        /// In-memory message ring buffer per chat.
        history: std::collections::HashMap<String, VecDeque<StoredMessage>>,
        /// Maximum messages per chat.
        history_limit: usize,
    }

    impl WhatsappSessionStore {
        pub fn new(session_path: PathBuf) -> Self {
            Self {
                session_path,
                history: std::collections::HashMap::new(),
                history_limit: DEFAULT_HISTORY_LIMIT,
            }
        }

        /// Store a message in the history ring buffer.
        pub fn store_message(&mut self, msg: StoredMessage) {
            let chat_history = self.history.entry(msg.chat.clone()).or_default();
            if chat_history.len() >= self.history_limit {
                chat_history.pop_front();
            }
            chat_history.push_back(msg);
        }

        /// Get message history for a chat, most recent first.
        pub fn get_history(&self, chat: &str, limit: usize) -> Vec<&StoredMessage> {
            self.history
                .get(chat)
                .map(|h| h.iter().rev().take(limit).collect())
                .unwrap_or_default()
        }

        /// Get total message count across all chats.
        pub fn total_messages(&self) -> usize {
            self.history.values().map(|h| h.len()).sum()
        }

        /// List all chats with stored messages.
        pub fn chats(&self) -> Vec<&str> {
            self.history.keys().map(String::as_str).collect()
        }

        /// Clear all stored messages.
        pub fn clear(&mut self) {
            self.history.clear();
        }
    }

    // WhatsApp Storage is not a Channel — it's a backing store used by
    // WhatsappWebChannel. We still register it in the catalog for visibility
    // but use a stub Channel impl that explains its role.

    pub struct WhatsappStorageChannel;

    #[async_trait::async_trait]
    impl crate::Channel for WhatsappStorageChannel {
        fn name(&self) -> &str {
            "whatsapp-storage"
        }

        async fn send(&self, _message: &crate::SendMessage) -> anyhow::Result<()> {
            anyhow::bail!(
                "whatsapp-storage is a backing store, not a messaging channel; use whatsapp-web instead"
            )
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<crate::ChannelMessage>,
        ) -> anyhow::Result<()> {
            anyhow::bail!(
                "whatsapp-storage is a backing store, not a messaging channel; use whatsapp-web instead"
            )
        }

        async fn health_check(&self) -> bool {
            false
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::Channel;
        use std::path::PathBuf;

        #[test]
        fn storage_store_and_retrieve() {
            let mut store = WhatsappSessionStore::new(PathBuf::from("/tmp/wa-test"));
            store.store_message(StoredMessage {
                message_id: "msg-1".to_string(),
                sender: "alice@s.whatsapp.net".to_string(),
                chat: "alice@s.whatsapp.net".to_string(),
                content: "hello".to_string(),
                timestamp: 1000,
                is_outgoing: false,
            });
            store.store_message(StoredMessage {
                message_id: "msg-2".to_string(),
                sender: "agent".to_string(),
                chat: "alice@s.whatsapp.net".to_string(),
                content: "hi there".to_string(),
                timestamp: 1001,
                is_outgoing: true,
            });

            assert_eq!(store.total_messages(), 2);
            let history = store.get_history("alice@s.whatsapp.net", 10);
            assert_eq!(history.len(), 2);
            assert_eq!(history[0].content, "hi there"); // most recent first
        }

        #[test]
        fn storage_ring_buffer_evicts_oldest() {
            let mut store = WhatsappSessionStore::new(PathBuf::from("/tmp/wa-test"));
            store.history_limit = 3;

            for i in 0..5 {
                store.store_message(StoredMessage {
                    message_id: format!("msg-{i}"),
                    sender: "user".to_string(),
                    chat: "chat-1".to_string(),
                    content: format!("message {i}"),
                    timestamp: i as u64,
                    is_outgoing: false,
                });
            }

            assert_eq!(store.total_messages(), 3);
            let history = store.get_history("chat-1", 10);
            assert_eq!(history[0].content, "message 4"); // newest
            assert_eq!(history[2].content, "message 2"); // oldest surviving
        }

        #[test]
        fn storage_chats_listing() {
            let mut store = WhatsappSessionStore::new(PathBuf::from("/tmp/wa-test"));
            store.store_message(StoredMessage {
                message_id: "1".to_string(),
                sender: "a".to_string(),
                chat: "chat-a".to_string(),
                content: "hi".to_string(),
                timestamp: 1,
                is_outgoing: false,
            });
            store.store_message(StoredMessage {
                message_id: "2".to_string(),
                sender: "b".to_string(),
                chat: "chat-b".to_string(),
                content: "hey".to_string(),
                timestamp: 2,
                is_outgoing: false,
            });

            let chats = store.chats();
            assert_eq!(chats.len(), 2);
            assert!(chats.contains(&"chat-a"));
            assert!(chats.contains(&"chat-b"));
        }

        #[test]
        fn storage_clear() {
            let mut store = WhatsappSessionStore::new(PathBuf::from("/tmp/wa-test"));
            store.store_message(StoredMessage {
                message_id: "1".to_string(),
                sender: "a".to_string(),
                chat: "chat-a".to_string(),
                content: "hi".to_string(),
                timestamp: 1,
                is_outgoing: false,
            });
            assert_eq!(store.total_messages(), 1);
            store.clear();
            assert_eq!(store.total_messages(), 0);
        }

        #[test]
        fn storage_channel_name() {
            let ch = WhatsappStorageChannel;
            assert_eq!(crate::Channel::name(&ch), "whatsapp-storage");
        }

        #[tokio::test]
        async fn storage_channel_send_fails() {
            let ch = WhatsappStorageChannel;
            let msg = crate::SendMessage::new("test", "user");
            let err = ch.send(&msg).await.expect_err("should fail");
            assert!(err.to_string().contains("backing store"));
        }
    }
}

#[cfg(feature = "channel-whatsapp-web")]
pub use impl_::*;

#[cfg(not(feature = "channel-whatsapp-web"))]
super::channel_stub!(
    WhatsappStorageChannel,
    WHATSAPP_STORAGE_DESCRIPTOR,
    "whatsapp-storage",
    "WhatsApp Storage"
);
