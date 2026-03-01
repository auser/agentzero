use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationDescriptor {
    pub id: String,
    pub display_name: String,
    pub enabled_by_default: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IntegrationError {
    #[error("integration id cannot be empty")]
    EmptyId,
    #[error("integration id contains invalid characters: {0}")]
    InvalidId(String),
}

pub fn normalize_integration_id(input: &str) -> Result<String, IntegrationError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(IntegrationError::EmptyId);
    }

    if !trimmed
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(IntegrationError::InvalidId(trimmed.to_string()));
    }

    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_integration_id_accepts_lowercase_dash_ids() {
        let normalized = normalize_integration_id("discord-webhook").expect("valid id");
        assert_eq!(normalized, "discord-webhook");
    }

    #[test]
    fn normalize_integration_id_rejects_invalid_chars() {
        let err = normalize_integration_id("Discord Webhook").expect_err("invalid id should fail");
        assert_eq!(
            err,
            IntegrationError::InvalidId("Discord Webhook".to_string())
        );
    }
}
