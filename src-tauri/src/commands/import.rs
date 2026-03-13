use crate::{
    cache,
    library::{ImportFailure, ImportSongsResult, Song},
    metadata, AppState,
};
use anyhow::{Context, Result};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;

#[tauri::command]
pub fn import_songs(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<ImportSongsResult, String> {
    let connection =
        cache::open_database(&state.database_path).map_err(|error| error.to_string())?;

    Ok(import_songs_from_paths(&connection, &paths))
}

#[tauri::command]
pub fn get_library(state: State<'_, AppState>) -> Result<Vec<Song>, String> {
    let connection =
        cache::open_database(&state.database_path).map_err(|error| error.to_string())?;

    get_library_from_connection(&connection).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn search_library(state: State<'_, AppState>, query: String) -> Result<Vec<Song>, String> {
    let connection =
        cache::open_database(&state.database_path).map_err(|error| error.to_string())?;

    cache::search_songs(&connection, &query).map_err(|error| error.to_string())
}

pub fn import_songs_from_paths(connection: &Connection, paths: &[String]) -> ImportSongsResult {
    let mut imported = Vec::new();
    let mut failed = Vec::new();

    for path in paths {
        match build_song_from_path(Path::new(path)) {
            Ok(song) => match cache::upsert_song(connection, &song) {
                Ok(()) => imported.push(song),
                Err(error) => failed.push(ImportFailure {
                    path: path.clone(),
                    error: error.to_string(),
                }),
            },
            Err(error) => failed.push(ImportFailure {
                path: path.clone(),
                error: error.to_string(),
            }),
        }
    }

    ImportSongsResult { imported, failed }
}

pub fn get_library_from_connection(connection: &Connection) -> rusqlite::Result<Vec<Song>> {
    cache::list_songs(connection)
}

fn build_song_from_path(path: &Path) -> Result<Song> {
    let metadata = metadata::read_from_path(path)?;
    let hash = sha256_for_file(path)?;
    let imported_at = current_unix_timestamp()?;
    let file_path = normalize_path(path)?;
    let title = metadata.title.or_else(|| {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
    });

    Ok(Song {
        hash,
        file_path,
        title,
        artist: metadata.artist,
        album: metadata.album,
        duration_ms: metadata.duration_ms,
        cover_art: metadata.cover_art,
        imported_at,
    })
}

fn sha256_for_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open audio file at {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];

    loop {
        let bytes_read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read audio file at {}", path.display()))?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    // The file hash becomes the stable identity for library rows and later cache
    // artifacts, so compute it from raw bytes instead of path-derived metadata.
    Ok(format!("{:x}", hasher.finalize()))
}

fn normalize_path(path: &Path) -> Result<String> {
    let absolute_path = path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize path {}", path.display()))?;

    Ok(absolute_path.display().to_string())
}

fn current_unix_timestamp() -> Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is set before Unix epoch")?;

    Ok(duration.as_secs() as i64)
}
