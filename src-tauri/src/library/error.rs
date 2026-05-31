use thiserror::Error;

#[derive(Error, Debug)]
pub enum LibraryError {
    #[error("media read failed: {0}")]
    MediaReadFailed(String),

    #[error("database unavailable: {0}")]
    DatabaseUnavailable(String),

    #[error("library error: {0}")]
    Internal(String),
}
