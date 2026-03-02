mod anthropic;
mod catalog;
mod models;
mod openai;
pub(crate) mod transport;

pub use anthropic::AnthropicProvider;
pub use catalog::{find_provider, supported_providers, ProviderDescriptor};
pub use models::{find_models_for_provider, ModelDescriptor};
pub use openai::OpenAiCompatibleProvider;

/// Declarative macro that generates the `build_provider` factory from a
/// list of `(kind-pattern => ProviderType)` entries. The last `_ =>` arm
/// is the catch-all for OpenAI-compatible providers.
macro_rules! register_providers {
    ( $( $pat:pat => $ty:ident ),+ $(,)? ) => {
        pub fn build_provider(
            kind: &str,
            base_url: String,
            api_key: String,
            model: String,
        ) -> Box<dyn agentzero_core::Provider> {
            match kind {
                $( $pat => Box::new($ty::new(base_url, api_key, model)), )+
            }
        }
    };
}

register_providers! {
    "anthropic" => AnthropicProvider,
    _ => OpenAiCompatibleProvider,
}
