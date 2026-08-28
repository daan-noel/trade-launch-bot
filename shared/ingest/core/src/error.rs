use thiserror::Error;

/// Engine-level errors. Wire failures are **not** here — a transport reports
/// them as [`crate::feed::FeedError`], in the terms the reconnect policy is
/// written in, so no provider's error type reaches this crate's public surface.
#[derive(Debug, Error)]
pub enum IngestError {
    #[error("invalid endpoint URL: {0}")]
    InvalidEndpoint(String),
    #[error("invalid program ID '{id}': {reason}")]
    InvalidProgramId { id: String, reason: String },
    #[error("feed error: {0}")]
    Feed(#[from] crate::feed::FeedError),
}

pub type Result<T> = std::result::Result<T, IngestError>;
