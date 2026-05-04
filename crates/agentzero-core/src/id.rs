use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! typed_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(String);

        impl $name {
            /// Create a new random identifier.
            pub fn new() -> Self {
                Self(Uuid::new_v4().to_string())
            }

            /// Create from an existing string value.
            pub fn from_string(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            /// Return the identifier as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

typed_id!(AgentId, "Unique identifier for an agent instance.");
typed_id!(SessionId, "Unique identifier for a session.");
typed_id!(
    ExecutionId,
    "Unique identifier for a single execution step."
);
typed_id!(SkillId, "Unique identifier for a skill.");
typed_id!(ToolId, "Unique identifier for a tool.");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let a = AgentId::new();
        let b = AgentId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn id_from_string_roundtrips() {
        let id = SessionId::from_string("test-session-1");
        assert_eq!(id.as_str(), "test-session-1");
        assert_eq!(id.to_string(), "test-session-1");
    }

    #[test]
    fn id_default_is_random() {
        let a = ToolId::default();
        let b = ToolId::default();
        assert_ne!(a, b);
    }
}
