use thiserror::Error;

use super::decode::DecodeError;

#[derive(Error, Debug)]
pub enum PlaybackError {
    #[error("song {0} was not found in the library")]
    SongNotFound(String),

    #[error("failed to decode audio: {0}")]
    AudioDecodeFailed(String),

    #[error("audio output unavailable: {0}")]
    AudioOutputUnavailable(String),

    #[error("karaoke not ready: {0}")]
    KaraokeNotReady(String),

    #[error("invalid playback state: {0}")]
    InvalidPlaybackState(String),

    #[error("playback request is stale")]
    StaleRequest,

    #[error("playback failed: {0}")]
    Internal(String),
}

impl From<DecodeError> for PlaybackError {
    fn from(err: DecodeError) -> Self {
        match err {
            DecodeError::FileOpenFailed(msg)
            | DecodeError::ProbeFailed(msg)
            | DecodeError::DecoderCreationFailed(msg)
            | DecodeError::PacketReadFailed(msg)
            | DecodeError::DecodeFailed(msg) => PlaybackError::AudioDecodeFailed(msg),
            DecodeError::NoDefaultTrack
            | DecodeError::NoSamples
            | DecodeError::MissingSampleRate(_)
            | DecodeError::MissingChannels(_)
            | DecodeError::ResetNotSupported => PlaybackError::AudioDecodeFailed(err.to_string()),
            DecodeError::Internal(msg) => PlaybackError::Internal(msg),
        }
    }
}
