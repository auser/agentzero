//! Storage backends for AgentZero.
//!
//! Provides encrypted key-value storage, SQLite/SQLCipher conversation
//! memory, and an optional Turso (libSQL) remote backend. The crypto
//! module handles AES-256-GCM encryption for secrets at rest.

pub mod crypto;
pub mod event_bus;
pub mod memory;
mod queue;
mod store;

pub use crypto::StorageKey;
pub use event_bus::SqliteEventBus;
pub use queue::{EncryptedQueue, QueuedItem};
pub use store::EncryptedJsonStore;
