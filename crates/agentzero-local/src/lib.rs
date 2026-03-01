pub mod discovery;
pub mod health;
pub mod models;
pub mod pull;

pub use discovery::{discover_local_services, DiscoveredService, DiscoveryOptions, ServiceStatus};
pub use health::{check_health, HealthCheckResult};
pub use models::{list_models, LiveModel};
pub use pull::pull_model;
