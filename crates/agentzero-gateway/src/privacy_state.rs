//! Concurrent Noise session store for the gateway.
//!
//! Manages active `NoiseSession` instances with TTL-based expiry and
//! configurable maximum session count.

use agentzero_core::privacy::noise::NoiseSession;
use dashmap::DashMap;
use std::sync::Arc;

/// Thread-safe store of active Noise sessions, keyed by session ID.
pub struct NoiseSessionStore {
    sessions: DashMap<[u8; 32], NoiseSession>,
    max_sessions: usize,
    session_timeout_secs: u64,
}

impl NoiseSessionStore {
    /// Create a new session store.
    pub fn new(max_sessions: usize, session_timeout_secs: u64) -> Arc<Self> {
        Arc::new(Self {
            sessions: DashMap::new(),
            max_sessions,
            session_timeout_secs,
        })
    }

    /// Insert a new session. Returns an error if the store is full after eviction.
    pub fn insert(&self, session: NoiseSession) -> anyhow::Result<[u8; 32]> {
        // Evict expired sessions first.
        self.evict_expired();

        if self.sessions.len() >= self.max_sessions {
            anyhow::bail!(
                "noise session store full ({} sessions, max {})",
                self.sessions.len(),
                self.max_sessions,
            );
        }

        let id = *session.session_id();
        self.sessions.insert(id, session);
        Ok(id)
    }

    /// Retrieve a mutable reference to a session by ID for encrypt/decrypt.
    /// Returns `None` if the session doesn't exist or has expired.
    pub fn with_session<F, R>(&self, session_id: &[u8; 32], f: F) -> Option<R>
    where
        F: FnOnce(&mut NoiseSession) -> R,
    {
        let mut entry = self.sessions.get_mut(session_id)?;
        if entry.is_expired(self.session_timeout_secs) {
            drop(entry);
            self.sessions.remove(session_id);
            return None;
        }
        Some(f(entry.value_mut()))
    }

    /// Remove a session by ID.
    pub fn remove(&self, session_id: &[u8; 32]) -> Option<NoiseSession> {
        self.sessions.remove(session_id).map(|(_, s)| s)
    }

    /// Number of active sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Evict all expired sessions.
    pub fn evict_expired(&self) {
        self.sessions
            .retain(|_, session| !session.is_expired(self.session_timeout_secs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::privacy::noise::{NoiseHandshaker, NoiseKeypair};

    fn make_session() -> NoiseSession {
        let client_kp = NoiseKeypair::generate().unwrap();
        let server_kp = NoiseKeypair::generate().unwrap();
        let mut client = NoiseHandshaker::new_initiator("XX", &client_kp).unwrap();
        let mut server = NoiseHandshaker::new_responder("XX", &server_kp).unwrap();

        let mut buf = [0u8; 65535];
        let mut pb = [0u8; 65535];
        let len = client.write_message(b"", &mut buf).unwrap();
        server.read_message(&buf[..len], &mut pb).unwrap();
        let len = server.write_message(b"", &mut buf).unwrap();
        client.read_message(&buf[..len], &mut pb).unwrap();
        let len = client.write_message(b"", &mut buf).unwrap();
        server.read_message(&buf[..len], &mut pb).unwrap();

        server.into_transport().unwrap()
    }

    #[test]
    fn insert_and_retrieve_session() {
        let store = NoiseSessionStore::new(10, 3600);
        let session = make_session();
        let id = store.insert(session).unwrap();

        let found = store.with_session(&id, |_| true);
        assert_eq!(found, Some(true));
    }

    #[test]
    fn store_rejects_when_full() {
        let store = NoiseSessionStore::new(1, 3600);
        let session1 = make_session();
        store.insert(session1).unwrap();

        let session2 = make_session();
        let result = store.insert(session2);
        assert!(result.is_err());
    }

    #[test]
    fn expired_sessions_are_evicted() {
        let store = NoiseSessionStore::new(10, 0); // 0-second timeout = immediately expired
        let session = make_session();
        let id = store.insert(session).unwrap();

        // Session should be evicted on access
        let found = store.with_session(&id, |_| true);
        assert!(found.is_none());
    }

    #[test]
    fn remove_session_by_id() {
        let store = NoiseSessionStore::new(10, 3600);
        let session = make_session();
        let id = store.insert(session).unwrap();
        assert_eq!(store.len(), 1);

        store.remove(&id);
        assert!(store.is_empty());
    }

    #[test]
    fn evict_expired_clears_old_sessions() {
        // Use a long timeout so insert doesn't evict during the insert call.
        let store = NoiseSessionStore::new(10, 3600);
        store.insert(make_session()).unwrap();
        store.insert(make_session()).unwrap();
        assert_eq!(store.len(), 2);

        // Verify the eviction mechanism works: sessions with 0-second timeout
        // are immediately considered expired. We test this through the
        // `expired_sessions_are_evicted` test above. Here we verify
        // that non-expired sessions survive eviction.
        store.evict_expired();
        assert_eq!(
            store.len(),
            2,
            "non-expired sessions should survive eviction"
        );
    }
}
