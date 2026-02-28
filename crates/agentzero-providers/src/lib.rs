mod catalog;
mod openai;

pub use catalog::{find_provider, supported_providers, ProviderDescriptor};
pub use openai::OpenAiCompatibleProvider;
