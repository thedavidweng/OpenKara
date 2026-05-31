use anyhow::{Context, Result};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    DatabaseUnavailable,
    MediaReadFailed,
    SongNotFound,
    ModelUnavailable,
    AudioDecodeFailed,
    AudioOutputUnavailable,
    KaraokeNotReady,
    LyricsNotReady,
    NetworkUnavailable,
    InvalidPlaybackState,
    SeparationFailed,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackAction {
    Retry,
    RefreshLibrary,
    ReimportSong,
    CheckAudioOutputDevice,
    StayInOriginalMode,
    ShowEmptyState,
    KeepCurrentState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub fallback: FallbackAction,
}

pub type CommandResult<T> = std::result::Result<T, CommandError>;

impl CommandError {
    pub fn new(
        code: ErrorCode,
        message: impl Into<String>,
        retryable: bool,
        fallback: FallbackAction,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            fallback,
        }
    }
}

pub fn database_error(message: impl ToString) -> CommandError {
    CommandError::new(
        ErrorCode::DatabaseUnavailable,
        message.to_string(),
        true,
        FallbackAction::Retry,
    )
}

pub fn state_lock_error(message: impl ToString) -> CommandError {
    CommandError::new(
        ErrorCode::Internal,
        message.to_string(),
        false,
        FallbackAction::KeepCurrentState,
    )
}

pub fn internal_error(message: impl ToString) -> CommandError {
    CommandError::new(
        ErrorCode::Internal,
        message.to_string(),
        true,
        FallbackAction::Retry,
    )
}

pub fn model_bootstrap_error(message: impl ToString) -> CommandError {
    CommandError::new(
        ErrorCode::ModelUnavailable,
        message.to_string(),
        true,
        FallbackAction::Retry,
    )
}

// --- Typed error conversions ---

impl From<crate::separator::error::SeparationError> for CommandError {
    fn from(err: crate::separator::error::SeparationError) -> Self {
        use crate::separator::error::SeparationError::*;
        match err {
            SongNotFound(id) => CommandError::new(
                ErrorCode::SongNotFound,
                format!("song {id} was not found in the library"),
                false,
                FallbackAction::RefreshLibrary,
            ),
            AudioDecodeFailed(msg) => CommandError::new(
                ErrorCode::AudioDecodeFailed,
                msg,
                false,
                FallbackAction::ReimportSong,
            ),
            Failed(msg) => CommandError::new(
                ErrorCode::SeparationFailed,
                msg,
                true,
                FallbackAction::Retry,
            ),
        }
    }
}

impl From<crate::audio::error::PlaybackError> for CommandError {
    fn from(err: crate::audio::error::PlaybackError) -> Self {
        use crate::audio::error::PlaybackError::*;
        match err {
            SongNotFound(id) => CommandError::new(
                ErrorCode::SongNotFound,
                format!("song {id} was not found in the library"),
                false,
                FallbackAction::RefreshLibrary,
            ),
            AudioDecodeFailed(msg) => CommandError::new(
                ErrorCode::AudioDecodeFailed,
                msg,
                false,
                FallbackAction::ReimportSong,
            ),
            AudioOutputUnavailable(msg) => CommandError::new(
                ErrorCode::AudioOutputUnavailable,
                msg,
                true,
                FallbackAction::CheckAudioOutputDevice,
            ),
            KaraokeNotReady(msg) => CommandError::new(
                ErrorCode::KaraokeNotReady,
                msg,
                true,
                FallbackAction::StayInOriginalMode,
            ),
            InvalidPlaybackState(msg) => CommandError::new(
                ErrorCode::InvalidPlaybackState,
                msg,
                false,
                FallbackAction::KeepCurrentState,
            ),
            Internal(msg) => CommandError::new(
                ErrorCode::Internal,
                msg,
                true,
                FallbackAction::Retry,
            ),
        }
    }
}

impl From<crate::library::error::LibraryError> for CommandError {
    fn from(err: crate::library::error::LibraryError) -> Self {
        use crate::library::error::LibraryError::*;
        match err {
            MediaReadFailed(msg) => CommandError::new(
                ErrorCode::MediaReadFailed,
                msg,
                false,
                FallbackAction::ReimportSong,
            ),
            DatabaseUnavailable(msg) => database_error(msg),
            Internal(msg) => internal_error(msg),
        }
    }
}

impl From<crate::lyrics::error::LyricsError> for CommandError {
    fn from(err: crate::lyrics::error::LyricsError) -> Self {
        use crate::lyrics::error::LyricsError::*;
        match err {
            SongNotFound(id) => CommandError::new(
                ErrorCode::SongNotFound,
                format!("song {id} was not found in the library"),
                false,
                FallbackAction::RefreshLibrary,
            ),
            LyricsNotReady(msg) => CommandError::new(
                ErrorCode::LyricsNotReady,
                msg,
                true,
                FallbackAction::ShowEmptyState,
            ),
            NetworkUnavailable(msg) => CommandError::new(
                ErrorCode::NetworkUnavailable,
                msg,
                true,
                FallbackAction::Retry,
            ),
            DatabaseUnavailable(msg) => database_error(msg),
            Internal(msg) => internal_error(msg),
        }
    }
}

pub fn current_unix_timestamp() -> Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is set before Unix epoch")?;

    Ok(duration.as_secs() as i64)
}
