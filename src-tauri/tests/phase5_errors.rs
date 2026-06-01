use openkara_lib::audio::error::PlaybackError;
use openkara_lib::commands::error::{CommandError, ErrorCode, FallbackAction};
use openkara_lib::library::error::LibraryError;
use openkara_lib::lyrics::error::LyricsError;
use openkara_lib::separator::error::SeparationError;

#[test]
fn playback_errors_map_decode_failures_to_reimport_song_fallback() {
    let error = CommandError::from(PlaybackError::AudioDecodeFailed(
        "failed to decode audio for /tmp/corrupt.mp3".to_owned(),
    ));

    assert_eq!(error.code, ErrorCode::AudioDecodeFailed);
    assert_eq!(error.fallback, FallbackAction::ReimportSong);
    assert!(!error.retryable);
}

#[test]
fn playback_errors_map_missing_stems_to_original_mode_fallback() {
    let error = CommandError::from(PlaybackError::KaraokeNotReady(
        "song with hash song-a does not have cached stems".to_owned(),
    ));

    assert_eq!(error.code, ErrorCode::KaraokeNotReady);
    assert_eq!(error.fallback, FallbackAction::StayInOriginalMode);
    assert!(error.retryable);
}

#[test]
fn lyrics_errors_map_missing_cache_to_empty_state_fallback() {
    let error = CommandError::from(LyricsError::LyricsNotReady(
        "song with hash song-a does not have cached lyrics".to_owned(),
    ));

    assert_eq!(error.code, ErrorCode::LyricsNotReady);
    assert_eq!(error.fallback, FallbackAction::ShowEmptyState);
    assert!(error.retryable);
}

#[test]
fn lyrics_errors_map_network_failures_to_retry_fallback() {
    let error = CommandError::from(LyricsError::NetworkUnavailable(
        "failed to request timed lyrics".to_owned(),
    ));

    assert_eq!(error.code, ErrorCode::NetworkUnavailable);
    assert_eq!(error.fallback, FallbackAction::Retry);
    assert!(error.retryable);
}

#[test]
fn separation_errors_map_worker_failures_to_retry_fallback() {
    let error = CommandError::from(SeparationError::Failed(
        "failed to separate stems for song song-a".to_owned(),
    ));

    assert_eq!(error.code, ErrorCode::SeparationFailed);
    assert_eq!(error.fallback, FallbackAction::Retry);
    assert!(error.retryable);
}

#[test]
fn library_errors_map_missing_media_to_reimport_fallback() {
    let error = CommandError::from(LibraryError::MediaReadFailed(
        "failed to open audio file at /tmp/missing.mp3".to_owned(),
    ));

    assert_eq!(error.code, ErrorCode::MediaReadFailed);
    assert_eq!(error.fallback, FallbackAction::ReimportSong);
    assert!(!error.retryable);
}
