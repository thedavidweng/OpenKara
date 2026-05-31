use thiserror::Error;

#[derive(Error, Debug)]
pub enum LyricsError {
    #[error("song {0} was not found in the library")]
    SongNotFound(String),

    #[error("lyrics not ready: {0}")]
    LyricsNotReady(String),

    #[error("network unavailable: {0}")]
    NetworkUnavailable(String),

    #[error("database unavailable: {0}")]
    DatabaseUnavailable(String),

    #[error("lyrics error: {0}")]
    Internal(String),
}
