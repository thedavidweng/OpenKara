use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    DatabaseUnavailable,
    RemoteRepositoryUnavailable,
    MediaReadFailed,
    SongNotFound,
    ModelUnavailable,
    AudioDecodeFailed,
    AudioOutputUnavailable,
    KaraokeNotReady,
    LyricsNotReady,
    NetworkUnavailable,
    InvalidPlaybackState,
    ExecutionProviderUnavailable,
    RuntimePostDownloadTimeout,
    SeparationFailed,
    OnlineSourceDisabled,
    StreamingAuthFailed,
    StreamingSessionExpired,
    VideoSourceUnavailable,
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

pub fn remote_repository_unavailable(message: impl ToString) -> CommandError {
    CommandError::new(
        ErrorCode::RemoteRepositoryUnavailable,
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

pub fn online_source_disabled(message: impl ToString) -> CommandError {
    CommandError::new(
        ErrorCode::OnlineSourceDisabled,
        message.to_string(),
        false,
        FallbackAction::KeepCurrentState,
    )
}

pub fn streaming_auth_failed(message: impl ToString) -> CommandError {
    CommandError::new(
        ErrorCode::StreamingAuthFailed,
        message.to_string(),
        false,
        FallbackAction::KeepCurrentState,
    )
}

pub fn streaming_session_expired(message: impl ToString) -> CommandError {
    CommandError::new(
        ErrorCode::StreamingSessionExpired,
        message.to_string(),
        true,
        FallbackAction::Retry,
    )
}

pub fn video_source_unavailable(message: impl ToString) -> CommandError {
    CommandError::new(
        ErrorCode::VideoSourceUnavailable,
        message.to_string(),
        false,
        FallbackAction::KeepCurrentState,
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

pub fn runtime_post_download_timeout(message: impl ToString) -> CommandError {
    CommandError::new(
        ErrorCode::RuntimePostDownloadTimeout,
        message.to_string(),
        true,
        FallbackAction::Retry,
    )
}

pub fn execution_provider_unavailable(
    provider: crate::config::ExecutionProviderPreference,
) -> CommandError {
    CommandError::new(
        ErrorCode::ExecutionProviderUnavailable,
        format!(
            "execution provider '{}' is not compatible with this device; switch to CPU in Settings",
            provider.as_str()
        ),
        false,
        FallbackAction::KeepCurrentState,
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
            StaleRequest => CommandError::new(
                ErrorCode::Internal,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incompatible_execution_provider_error_points_to_cpu() {
        let error =
            execution_provider_unavailable(crate::config::ExecutionProviderPreference::DirectMl);

        assert_eq!(error.code, ErrorCode::ExecutionProviderUnavailable);
        assert!(!error.retryable);
        assert_eq!(error.fallback, FallbackAction::KeepCurrentState);
        assert!(error.message.contains("directml"));
        assert!(error.message.contains("switch to CPU in Settings"));
    }
}
