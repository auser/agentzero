use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrainError {
    #[error("vault not initialized at {0}. Run `az brain init --root {0}` first.")]
    VaultNotInitialized(String),

    #[error("config parse error: {0}")]
    ConfigError(String),

    #[error("template not found: {0}")]
    TemplateNotFound(String),

    #[error("section not found: {0}")]
    SectionNotFound(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("path traversal denied: {0}")]
    PathTraversal(String),

    #[error("raw directory is immutable (safety.raw_is_immutable = true): {0}")]
    RawImmutable(String),

    #[error("invalid date: {0}")]
    InvalidDate(String),
}
