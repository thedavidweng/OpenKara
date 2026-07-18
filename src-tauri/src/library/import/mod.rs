//! Library import write path: classify paths, copy media into LibraryRoot, upsert songs.
//!
//! Callers open a `Connection` + `LibraryRoot`. Remote Pre-Mutation Refresh /
//! Publish Song hooks stay at the command/remote layer — this module only
//! mutates the local library storage and files.

mod cover;
mod expand;
mod ingest;
mod preview;
mod types;

pub use cover::extract_embedded_cover_art_from_connection;
pub use expand::collect_expandable_import_paths;
pub use preview::{display_audio_format, inspect_import_candidate};
pub use types::{
    DeleteSongsFailure, DeleteSongsResult, ExpandedImportPaths, ExtractEmbeddedCoverArtFailure,
    ExtractEmbeddedCoverArtResult, ImportCandidateDetails, ImportSongsOptions, SongProperties,
};

use crate::{
    cache,
    commands::error::{database_error, CommandError},
    library::{artwork, error::LibraryError, ImportFailure, ImportSongsResult, Song},
    library_root::LibraryRoot,
};
use rusqlite::Connection;
use std::collections::HashSet;

use expand::{build_selected_cdg_lookup, classify_import_paths};
use ingest::{build_and_store_media_g_zip, build_and_store_song, try_extract_embedded_lyrics};
use rayon::prelude::*;

/// Generate and persist derivative paths only after the song upsert commits.
/// If derivative generation fails, clear any paths retained from a previous
/// cover so an old thumbnail can never be paired with newly imported cover
/// bytes. Any files generated immediately before a failed path update are
/// removed only when reference counting proves they are unreferenced.
fn persist_artwork_derivatives_after_upsert(
    connection: &Connection,
    library: &LibraryRoot,
    song: &Song,
) {
    let old_paths = match cache::get_artwork_record(connection, &song.hash) {
        Ok(Some(record)) => [record.artwork_thumb_path, record.artwork_preview_path]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        Ok(None) => Vec::new(),
        Err(error) => {
            tracing::warn!(
                "failed to read existing artwork derivative paths for {}: {error}",
                song.hash
            );
            Vec::new()
        }
    };
    let (thumb_path, preview_path) =
        ingest::try_generate_artwork_derivatives_for_song(song, library);

    if let Err(error) = cache::update_artwork_derivative_paths(
        connection,
        &song.hash,
        thumb_path.as_deref(),
        preview_path.as_deref(),
    ) {
        for path in [&thumb_path, &preview_path].into_iter().flatten() {
            let _ = artwork::delete_artwork_derivative_if_unreferenced(connection, library, path);
        }
        tracing::warn!(
            "failed to persist artwork derivative paths for {}: {error}",
            song.hash
        );
        return;
    }

    let mut seen = HashSet::new();
    for old_path in old_paths {
        if !seen.insert(old_path.clone())
            || thumb_path.as_deref() == Some(old_path.as_str())
            || preview_path.as_deref() == Some(old_path.as_str())
        {
            continue;
        }
        let _ = artwork::delete_artwork_derivative_if_unreferenced(connection, library, &old_path);
    }
}

/// Import songs from absolute filesystem paths into the library.
pub fn import_songs_from_paths(
    connection: &Connection,
    library: &LibraryRoot,
    paths: &[String],
) -> ImportSongsResult {
    import_songs_from_paths_with_options(connection, library, paths, &ImportSongsOptions::default())
}

/// Import songs with optional CDG pairing overrides.
pub fn import_songs_from_paths_with_options(
    connection: &Connection,
    library: &LibraryRoot,
    paths: &[String],
    options: &ImportSongsOptions,
) -> ImportSongsResult {
    let mut imported = Vec::new();
    let mut failed = Vec::new();
    let classified = classify_import_paths(paths);
    let selected_cdg_by_stem = build_selected_cdg_lookup(&classified.cdg_paths);

    // Phase 1: Read metadata and copy files in parallel (I/O bound).
    // Phase 2: Insert into database sequentially (SQLite single-writer).
    let prepared: Vec<_> = classified
        .audio_paths
        .par_iter()
        .map(|audio_path| {
            let result = build_and_store_song(
                audio_path,
                library,
                &selected_cdg_by_stem,
                &options.explicit_cdg_by_audio_path,
                &options.skip_cdg_for_audio_paths,
                &mut HashSet::<std::path::PathBuf>::new(),
            );
            (audio_path, result)
        })
        .collect();

    // Collect consumed CDG source paths from successful imports.
    // Uses the original source paths (not library-relative) so the standalone
    // CDG check below can match against classified.cdg_paths.
    let mut consumed_cdg_paths = HashSet::new();

    for (audio_path, result) in prepared {
        match result {
            Ok(build_result) => {
                if let Some(cdg_source) = build_result.consumed_cdg_source {
                    consumed_cdg_paths.insert(cdg_source);
                }
                let song = build_result.song;
                match cache::upsert_song(connection, &song) {
                    Ok(()) => {
                        persist_artwork_derivatives_after_upsert(connection, library, &song);
                        try_extract_embedded_lyrics(connection, &song, library);
                        imported.push(song);
                    }
                    Err(error) => failed.push(ImportFailure {
                        path: audio_path.display().to_string(),
                        error: database_error(error.to_string()),
                    }),
                }
            }
            Err(error) => failed.push(ImportFailure {
                path: audio_path.display().to_string(),
                error: CommandError::from(LibraryError::MediaReadFailed(error.to_string())),
            }),
        }
    }

    for zip_path in &classified.zip_paths {
        match build_and_store_media_g_zip(zip_path, library) {
            Ok(song) => match cache::upsert_song(connection, &song) {
                Ok(()) => {
                    persist_artwork_derivatives_after_upsert(connection, library, &song);
                    imported.push(song);
                }
                Err(error) => failed.push(ImportFailure {
                    path: zip_path.display().to_string(),
                    error: database_error(error.to_string()),
                }),
            },
            Err(error) => failed.push(ImportFailure {
                path: zip_path.display().to_string(),
                error: CommandError::from(LibraryError::MediaReadFailed(error.to_string())),
            }),
        }
    }

    for cdg_path in &classified.cdg_paths {
        if consumed_cdg_paths.contains(cdg_path) {
            continue;
        }

        failed.push(ImportFailure {
            path: cdg_path.display().to_string(),
            error: CommandError::from(LibraryError::Internal(format!(
                "standalone .cdg file {} does not have a matching audio track",
                cdg_path.display()
            ))),
        });
    }

    ImportSongsResult { imported, failed }
}

/// List all songs from an open library connection.
pub fn get_library_from_connection(connection: &Connection) -> rusqlite::Result<Vec<Song>> {
    cache::list_songs(connection)
}

#[cfg(test)]
mod tests {
    use super::expand::collect_expandable_import_paths;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(path: &std::path::Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create test directory");
        }
        fs::write(path, b"test").expect("failed to write test file");
    }

    #[test]
    fn expands_importable_files_recursively() {
        let tempdir = TempDir::new().expect("failed to create tempdir");
        let root = tempdir.path();

        write_file(&root.join("one.mp3"));
        write_file(&root.join("two.lrc"));
        write_file(&root.join("three.cdg"));
        write_file(&root.join("nested/four.zip"));
        write_file(&root.join("nested/deeper/five.flac"));
        write_file(&root.join("nested/deeper/too/deep/hidden.mp3"));

        let result = collect_expandable_import_paths(&[root.display().to_string()]);

        assert_eq!(result.song_count, 3);
        assert!(result.paths.iter().any(|path| path.ends_with("one.mp3")));
        assert!(result.paths.iter().any(|path| path.ends_with("two.lrc")));
        assert!(result.paths.iter().any(|path| path.ends_with("three.cdg")));
        assert!(result
            .paths
            .iter()
            .any(|path| path.ends_with("nested/four.zip")));
        assert!(result
            .paths
            .iter()
            .any(|path| path.ends_with("nested/deeper/five.flac")));
        assert!(!result
            .paths
            .iter()
            .any(|path| path.ends_with("nested/deeper/too/deep/hidden.mp3")));
    }

    #[test]
    fn caps_recursive_import_scanning_depth() {
        let tempdir = TempDir::new().expect("failed to create tempdir");
        let root = tempdir.path();

        write_file(&root.join("level-0.mp3"));
        write_file(&root.join("level-1/level-1.mp3"));
        write_file(&root.join("level-1/level-2/level-2.mp3"));
        write_file(&root.join("level-1/level-2/level-3/level-3.mp3"));
        write_file(&root.join("level-1/level-2/level-3/level-4/level-5/level-5.mp3"));

        let result = collect_expandable_import_paths(&[root.join("level-1").display().to_string()]);

        assert_eq!(result.song_count, 3);
        assert!(result
            .paths
            .iter()
            .any(|path| path.ends_with("level-1/level-1.mp3")));
        assert!(result
            .paths
            .iter()
            .any(|path| path.ends_with("level-1/level-2/level-2.mp3")));
        assert!(result
            .paths
            .iter()
            .any(|path| path.ends_with("level-1/level-2/level-3/level-3.mp3")));
        assert!(!result
            .paths
            .iter()
            .any(|path| path.ends_with("level-1/level-2/level-3/level-4/level-5/level-5.mp3")));
    }
}
