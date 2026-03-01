mod catalog;
mod models;
mod openai;

pub use catalog::{find_provider, supported_providers, ProviderDescriptor};
pub use models::{find_models_for_provider, ModelDescriptor};
pub use openai::OpenAiCompatibleProvider;
