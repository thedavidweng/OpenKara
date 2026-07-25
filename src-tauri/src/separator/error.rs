use thiserror::Error;

#[derive(Error, Debug)]
pub enum SeparationError {
    #[error("song {0} was not found in the library")]
    SongNotFound(String),

    #[error("failed to decode audio: {0}")]
    AudioDecodeFailed(String),

    #[error("separation failed: {0}")]
    Failed(String),

    /// A run was aborted at a cancellation checkpoint. The streaming writers
    /// are dropped without finalizing, so no partial stem set is promoted.
    #[error("separation was cancelled")]
    Cancelled,
}

/// True when `error` (or any error it wraps via `anyhow` context) is a
/// [`SeparationError::Cancelled`]. Used to distinguish an intentional
/// cancellation from a genuine failure.
pub fn is_cancelled(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<SeparationError>()
        .is_some_and(|e| matches!(e, SeparationError::Cancelled))
}
