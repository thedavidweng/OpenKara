use crate::{
    cache,
    commands::error::{database_error, CommandError, ErrorCode, FallbackAction},
    library::{artwork, error::LibraryError, Song},
    library_root::LibraryRoot,
    media_g::{self, MEDIA_G_ZIP},
    metadata,
};
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::collections::HashSet;

use super::types::{ExtractEmbeddedCoverArtFailure, ExtractEmbeddedCoverArtResult};

pub(super) fn extract_embedded_cover_art_for_song(
    connection: &Connection,
    library: &LibraryRoot,
    song_id: &str,
) -> Result<Song, CommandError> {
    let song = cache::get_song_by_hash(connection, song_id)
        .map_err(database_error)?
        .ok_or_else(|| {
            CommandError::new(
                ErrorCode::SongNotFound,
                format!("song {song_id} not found"),
                false,
                FallbackAction::RefreshLibrary,
            )
        })?;

    let cover_art = read_embedded_cover_art(library, &song).map_err(|error| {
        let message = error.to_string();
        if message.contains("does not contain embedded cover art") {
            return CommandError::new(
                ErrorCode::MediaReadFailed,
                message,
                false,
                FallbackAction::KeepCurrentState,
            );
        }

        CommandError::from(LibraryError::Internal(message))
    })?;

    // Regenerate artwork derivatives whenever authoritative cover bytes change.
    // Derivative identity is the SHA-256 of the cover bytes, so a new cover
    // produces new filenames and cannot serve stale imagery. Failure is
    // non-fatal: the original cover art is still persisted below, and lazy
    // repair will regenerate derivatives on the next cover art read.
    let (thumb_path, preview_path) =
        match artwork::generate_artwork_derivatives(library, &cover_art) {
            Ok(derivatives) => (Some(derivatives.thumb_path), Some(derivatives.preview_path)),
            Err(e) => {
                tracing::warn!(
                    "artwork derivative generation failed for song {}: {e}",
                    song.hash
                );
                (None, None)
            }
        };

    // Write the original bytes and both derivative paths in one atomic UPDATE
    // so a crash cannot leave a row with new paths referencing unwritten files
    // (or new bytes with stale paths). Returns the previous paths so we can
    // clean up on-disk files whose digest changed after the commit.
    let (old_thumb, old_preview) = match cache::replace_cover_art_and_derivatives(
        connection,
        &song.hash,
        Some(&cover_art),
        thumb_path.as_deref(),
        preview_path.as_deref(),
    ) {
        Ok(previous) => previous,
        Err(error) => {
            // The original row was not updated, so derivatives just created
            // for this failed replacement are unreferenced unless another
            // song already shares their content-addressed paths.
            for path in [&thumb_path, &preview_path].into_iter().flatten() {
                let _ =
                    artwork::delete_artwork_derivative_if_unreferenced(connection, library, path);
            }
            return Err(database_error(error.to_string()));
        }
    };

    // After the transaction commits, delete old derivative files whose digest
    // differs from the new cover and that no other song row references. Two
    // songs can share the same cover digest, so the reference count is checked.
    let mut seen: HashSet<String> = HashSet::new();
    for old_path in [old_thumb, old_preview].into_iter().flatten() {
        if !seen.insert(old_path.clone()) {
            continue;
        }
        if Some(old_path.as_str()) == thumb_path.as_deref()
            || Some(old_path.as_str()) == preview_path.as_deref()
        {
            // Same digest — the file is still in use by this song.
            continue;
        }
        let _ = artwork::delete_artwork_derivative_if_unreferenced(connection, library, &old_path);
    }

    cache::get_song_by_hash(connection, &song.hash)
        .map_err(database_error)?
        .ok_or_else(|| {
            CommandError::new(
                ErrorCode::SongNotFound,
                format!("song {} not found after updating cover art", song.hash),
                false,
                FallbackAction::RefreshLibrary,
            )
        })
}

pub(super) fn read_embedded_cover_art(library: &LibraryRoot, song: &Song) -> Result<Vec<u8>> {
    let Some(song_path) = song.file_path.as_deref() else {
        anyhow::bail!("song {} does not have a local file path", song.hash);
    };
    let resolved_path = library.resolve(song_path);

    let metadata = match song.media_g_container.as_deref() {
        Some(MEDIA_G_ZIP) => {
            let asset = media_g::inspect_zip_for_media_g(&resolved_path)?;
            metadata::read_from_bytes(&asset.audio_bytes, &asset.audio_extension)?
        }
        _ => metadata::read_from_path(&resolved_path)?,
    };

    metadata
        .cover_art
        .with_context(|| format!("song {} does not contain embedded cover art", song.hash))
}

pub fn extract_embedded_cover_art_from_connection(
    connection: &Connection,
    library: &LibraryRoot,
    song_ids: &[String],
) -> ExtractEmbeddedCoverArtResult {
    let mut updated_songs = Vec::new();
    let mut failed = Vec::new();

    for song_id in song_ids {
        match extract_embedded_cover_art_for_song(connection, library, song_id) {
            Ok(song) => updated_songs.push(song),
            Err(error) => failed.push(ExtractEmbeddedCoverArtFailure {
                song_id: song_id.clone(),
                error,
            }),
        }
    }

    ExtractEmbeddedCoverArtResult {
        updated_songs,
        failed,
    }
}
