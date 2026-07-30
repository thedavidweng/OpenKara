use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

pub fn invalid_playback_state(message: impl ToString) -> CommandError {
    CommandError::new(
        ErrorCode::InvalidPlaybackState,
        message.to_string(),
        false,
        FallbackAction::KeepCurrentState,
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

impl From<crate::separator::error::SeparationError> for CommandError {
    fn from(err: crate::separator::error::SeparationError) -> Self {
        use crate::separator::error::SeparationError::*;
        match &err {
            SongNotFound(_) => CommandError::new(
                ErrorCode::SongNotFound,
                err.to_string(),
                false,
                FallbackAction::RefreshLibrary,
            ),
            AudioDecodeFailed(_) => CommandError::new(
                ErrorCode::AudioDecodeFailed,
                err.to_string(),
                false,
                FallbackAction::ReimportSong,
            ),
            Failed(_) => CommandError::new(
                ErrorCode::SeparationFailed,
                err.to_string(),
                true,
                FallbackAction::Retry,
            ),
            Cancelled => CommandError::new(
                ErrorCode::SeparationFailed,
                err.to_string(),
                false,
                FallbackAction::KeepCurrentState,
            ),
        }
    }
}

impl From<crate::audio::error::PlaybackError> for CommandError {
    fn from(err: crate::audio::error::PlaybackError) -> Self {
        use crate::audio::error::PlaybackError::*;
        match &err {
            SongNotFound(_) => CommandError::new(
                ErrorCode::SongNotFound,
                err.to_string(),
                false,
                FallbackAction::RefreshLibrary,
            ),
            AudioDecodeFailed(_) => CommandError::new(
                ErrorCode::AudioDecodeFailed,
                err.to_string(),
                false,
                FallbackAction::ReimportSong,
            ),
            AudioOutputUnavailable(_) => CommandError::new(
                ErrorCode::AudioOutputUnavailable,
                err.to_string(),
                true,
                FallbackAction::CheckAudioOutputDevice,
            ),
            KaraokeNotReady(_) => CommandError::new(
                ErrorCode::KaraokeNotReady,
                err.to_string(),
                true,
                FallbackAction::StayInOriginalMode,
            ),
            InvalidPlaybackState(_) => CommandError::new(
                ErrorCode::InvalidPlaybackState,
                err.to_string(),
                false,
                FallbackAction::KeepCurrentState,
            ),
            Internal(_) => CommandError::new(
                ErrorCode::Internal,
                err.to_string(),
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
        match &err {
            SongNotFound(_) => CommandError::new(
                ErrorCode::SongNotFound,
                err.to_string(),
                false,
                FallbackAction::RefreshLibrary,
            ),
            LyricsNotReady(_) => CommandError::new(
                ErrorCode::LyricsNotReady,
                err.to_string(),
                true,
                FallbackAction::ShowEmptyState,
            ),
            NetworkUnavailable(_) => CommandError::new(
                ErrorCode::NetworkUnavailable,
                err.to_string(),
                true,
                FallbackAction::Retry,
            ),
            DatabaseUnavailable(_) => database_error(err.to_string()),
            Internal(_) => internal_error(err.to_string()),
        }
    }
}

pub fn current_unix_timestamp() -> Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is set before Unix epoch")?;

    Ok(duration.as_secs() as i64)
}

pub fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
