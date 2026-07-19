//! Song metadata and flag write path against an open library connection.
//!
//! Remote mutation wrappers (prepare / publish / mirror) stay at the command
//! layer; these helpers only touch SQLite via `cache`.

use crate::{
    audio::decode,
    cache,
    commands::error::{database_error, internal_error, CommandError, CommandResult},
    library::{
        error::LibraryError,
        import::{display_audio_format, SongProperties},
        Song,
    },
    library_root::LibraryRoot,
    media_g::{self, MEDIA_G_PAIRED, MEDIA_G_ZIP},
};
use rusqlite::Connection;

pub fn update_song_metadata(
    connection: &Connection,
    hash: &str,
    title: Option<&str>,
    artist: Option<&str>,
) -> CommandResult<Song> {
    cache::update_song_title_artist(connection, hash, title, artist).map_err(database_error)?;

    cache::get_song_by_hash(connection, hash)
        .map_err(database_error)?
        .ok_or_else(|| database_error(format!("song with hash {hash} not found")))
}

pub fn set_songs_instrumental(
    connection: &Connection,
    song_ids: &[String],
    instrumental: bool,
) -> CommandResult<Vec<Song>> {
    let mut updated_songs = Vec::with_capacity(song_ids.len());

    for song_id in song_ids {
        let updated = cache::update_song_instrumental(connection, song_id, instrumental)
            .map_err(|error| database_error(error.to_string()))?;
        if updated == 0 {
            return Err(database_error(format!(
                "song with hash {song_id} not found"
            )));
        }

        let song = cache::get_song_by_hash(connection, song_id)
            .map_err(|error| database_error(error.to_string()))?
            .ok_or_else(|| database_error(format!("song with hash {song_id} not found")))?;
        updated_songs.push(song);
    }

    Ok(updated_songs)
}

pub fn set_songs_language(
    connection: &Connection,
    song_ids: &[String],
    language: Option<&str>,
) -> CommandResult<Vec<Song>> {
    let mut updated_songs = Vec::with_capacity(song_ids.len());

    for song_id in song_ids {
        let updated = cache::update_song_language(connection, song_id, language)
            .map_err(|error| database_error(error.to_string()))?;
        if updated == 0 {
            return Err(database_error(format!(
                "song with hash {song_id} not found"
            )));
        }

        let song = cache::get_song_by_hash(connection, song_id)
            .map_err(|error| database_error(error.to_string()))?
            .ok_or_else(|| database_error(format!("song with hash {song_id} not found")))?;
        updated_songs.push(song);
    }

    Ok(updated_songs)
}

/// Callers that serve remote libraries must ensure remote working-copy files
/// are cached before calling this (see
/// `commands::remote_library::ensure_remote_file_cached`).
pub fn get_song_properties(
    connection: &Connection,
    library: &LibraryRoot,
    song_id: &str,
) -> CommandResult<SongProperties> {
    let song = cache::get_song_by_hash(connection, song_id)
        .map_err(database_error)?
        .ok_or_else(|| database_error(format!("song with hash {song_id} not found")))?;

    let Some(song_path) = song.file_path.as_deref() else {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "song {} does not have a local file path",
            song_id
        ))));
    };

    let file_path = library.resolve(song_path);
    let ext = song
        .original_ext
        .as_deref()
        .or_else(|| file_path.extension().and_then(|e| e.to_str()))
        .unwrap_or("bin");

    let (decoded, file_size, format) = if song.media_g_container.as_deref() == Some(MEDIA_G_ZIP) {
        let asset = media_g::inspect_zip_for_media_g(&file_path).map_err(|error| {
            CommandError::from(LibraryError::MediaReadFailed(error.to_string()))
        })?;
        let decoded = decode::decode_bytes(asset.audio_bytes, ext).map_err(|e| {
            internal_error(format!("failed to decode audio for {}: {}", song_id, e))
        })?;
        let file_size = std::fs::metadata(&file_path)
            .map_err(|e| {
                CommandError::from(LibraryError::MediaReadFailed(format!(
                    "failed to open Media+G ZIP at {}: {}",
                    file_path.display(),
                    e
                )))
            })?
            .len();
        (
            decoded,
            file_size,
            format!("{}+G ZIP", display_audio_format(ext)),
        )
    } else {
        let decoded = decode::decode_file(&file_path).map_err(|e| {
            internal_error(format!(
                "failed to decode audio for {}: {}",
                file_path.display(),
                e
            ))
        })?;
        let file_size = std::fs::metadata(&file_path)
            .map_err(|e| {
                CommandError::from(LibraryError::MediaReadFailed(format!(
                    "failed to open audio file at {}: {}",
                    file_path.display(),
                    e
                )))
            })?
            .len();
        let format = if song.media_g_container.as_deref() == Some(MEDIA_G_PAIRED) {
            format!("{}+G", display_audio_format(ext))
        } else {
            display_audio_format(ext).to_owned()
        };
        (decoded, file_size, format)
    };

    let bit_rate = if song.duration_ms > 0 {
        let duration_secs = song.duration_ms as f64 / 1000.0;
        Some(((file_size as f64 * 8.0) / duration_secs / 1000.0).round() as u32)
    } else {
        None
    };

    Ok(SongProperties {
        format,
        sample_rate: Some(decoded.sample_rate),
        channels: Some(decoded.channels as u16),
        bit_rate,
        file_size,
        duration_ms: song.duration_ms,
        hash: song.hash,
    })
}

pub fn delete_songs(
    connection: &Connection,
    library: &LibraryRoot,
    song_ids: &[String],
) -> crate::library::import::DeleteSongsResult {
    use crate::library::{
        delete::delete_song_from_library,
        import::{DeleteSongsFailure, DeleteSongsResult},
    };

    let mut deleted_song_ids = Vec::new();
    let mut failed = Vec::new();

    for song_id in song_ids {
        match delete_song_from_library(connection, library, song_id) {
            Ok(()) => deleted_song_ids.push(song_id.clone()),
            Err(error) => failed.push(DeleteSongsFailure {
                song_id: song_id.clone(),
                error: CommandError::from(LibraryError::Internal(error.to_string())),
            }),
        }
    }

    DeleteSongsResult {
        deleted_song_ids,
        failed,
    }
}
