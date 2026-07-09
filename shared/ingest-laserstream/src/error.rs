use thiserror::Error;

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("invalid endpoint URL: {0}")]
    InvalidEndpoint(String),
    #[error("invalid program ID '{id}': {reason}")]
    InvalidProgramId { id: String, reason: String },
    #[error("transport error: {0}")]
    Transport(#[from] tonic::transport::Error),
    #[error("gRPC status: {0}")]
    Status(#[from] tonic::Status),
}

pub type Result<T> = std::result::Result<T, IngestError>;
