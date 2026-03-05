//! Privacy primitives for AgentZero.
//!
//! Provides Noise Protocol session management for E2E encrypted communication,
//! sealed envelope encryption for zero-knowledge packet routing, and identity
//! key management. All types are gated behind the `privacy` feature flag.

pub mod boundary;
pub mod envelope;
pub mod keyring;
pub mod noise;
pub mod noise_client;
