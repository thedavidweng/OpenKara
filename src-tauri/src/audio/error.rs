use thiserror::Error;

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

    #[error("playback failed: {0}")]
    Internal(String),
}
