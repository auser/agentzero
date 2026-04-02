//! WASM plugin runtime for AgentZero.
//!
//! Provides the sandboxed WebAssembly execution environment for plugins.
//! Includes module compilation, ABI v2 dispatch, capability-based isolation,
//! and plugin packaging/discovery utilities.

pub mod overlay;
pub mod package;
mod package_ref;
#[cfg(feature = "signing")]
pub mod signing;
pub mod wasm;
#[cfg(feature = "plugin-dev")]
pub mod watcher;

pub use package_ref::{parse_plugin_package_ref, PluginPackageRef, PluginRefError};
