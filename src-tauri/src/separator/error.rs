use thiserror::Error;

#[derive(Error, Debug)]
pub enum SeparationError {
    #[error("song {0} was not found in the library")]
    SongNotFound(String),

    #[error("failed to decode audio: {0}")]
    AudioDecodeFailed(String),

    #[error("separation failed: {0}")]
    Failed(String),
}
