pub mod package;
mod package_ref;
pub mod wasm;
#[cfg(feature = "plugin-dev")]
pub mod watcher;

pub use package_ref::{parse_plugin_package_ref, PluginPackageRef, PluginRefError};
