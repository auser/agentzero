//! Storage backends for AgentZero.
//!
//! Provides encrypted key-value storage, SQLite/SQLCipher conversation
//! memory, and an optional Turso (libSQL) remote backend. The crypto
//! module handles AES-256-GCM encryption for secrets at rest.

// ---------------------------------------------------------------------------
// Compile-time feature guards
// ---------------------------------------------------------------------------

#[cfg(all(feature = "storage-encrypted", feature = "storage-plain"))]
compile_error!(
    "features `storage-encrypted` and `storage-plain` are mutually exclusive.\n\
     Reason: they pull conflicting `rusqlite` C symbols — only one SQLite \
     build can be linked at a time.\n\
     Fix: pick exactly one — `--features storage-encrypted` (SQLCipher, default) \
     or `--no-default-features --features storage-plain` (plaintext SQLite)."
);

pub mod crypto;
pub mod discord;
pub mod event_bus;
pub mod memory;
pub mod message_queue;
mod queue;
mod store;

pub use crypto::StorageKey;
pub use event_bus::SqliteEventBus;
pub use queue::{EncryptedQueue, QueuedItem};
pub use store::EncryptedJsonStore;
