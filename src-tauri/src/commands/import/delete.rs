use crate::{
    cache,
    library::Song,
    library_root::LibraryRoot,
    media_g::{MEDIA_G_PAIRED, MEDIA_G_ZIP},
};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::fs;

pub(crate) fn delete_song_from_library(
    connection: &Connection,
    library: &LibraryRoot,
    song_id: &str,
) -> Result<()> {
    let song = cache::get_song_by_hash(connection, song_id)
        .context("failed to load song from library")?
        .with_context(|| format!("song with hash {song_id} was not found in the library"))?;

    if let Some(container) = song.media_g_container.as_deref() {
        match container {
            MEDIA_G_PAIRED => {
                if let Some(relative_path) = song.file_path.as_deref() {
                    delete_relative_file(library, relative_path)?;
                }
                if let Some(cdg_path) = song.cdg_path.as_deref() {
                    delete_relative_file(library, cdg_path)?;
                }
            }
            MEDIA_G_ZIP => {
                if let Some(relative_path) = song.file_path.as_deref() {
                    delete_relative_file(library, relative_path)?;
                }
            }
            _ => {}
        }
    } else {
        if let Some(relative_path) = song.file_path.as_deref() {
            delete_relative_file(library, relative_path)?;
        }
    }

    delete_song_rows_from_database(connection, library, song_id)?;
    Ok(())
}

/// Delete only the database rows for a song (lyrics, play_history, stems, song).
/// Unlike `delete_song_from_library`, this does NOT touch the filesystem —
/// safe to call inside a SQLite transaction. The caller is responsible for
/// deleting any working-copy or cloud files separately.
pub(crate) fn delete_song_rows_from_database(
    connection: &Connection,
    _library: &LibraryRoot,
    song_id: &str,
) -> Result<()> {
    // DB-only stem delete — no filesystem side effects, safe inside a transaction.
    cache::stems::delete_stem_cache_entry_db_only(connection, song_id).ok();
    connection
        .execute("DELETE FROM lyrics WHERE song_hash = ?1", params![song_id])
        .context("failed to delete cached lyrics for song")?;
    if table_exists(connection, "play_history")? {
        connection
            .execute(
                "DELETE FROM play_history WHERE song_hash = ?1",
                params![song_id],
            )
            .context("failed to delete play history for song")?;
    }
    connection
        .execute("DELETE FROM songs WHERE hash = ?1", params![song_id])
        .context("failed to delete song row from database")?;
    Ok(())
}

/// Delete working-copy files for a song (audio, CDG, media_g containers, stems).
/// Does NOT touch the database — safe to call after a DB transaction has
/// already committed. Used by mirror sync to clean up the remote working
/// copy after transactional DB deletes.
pub(crate) fn delete_song_files_from_working_copy(
    library: &LibraryRoot,
    song: &Song,
) -> Result<()> {
    if let Some(container) = song.media_g_container.as_deref() {
        match container {
            MEDIA_G_PAIRED => {
                if let Some(relative_path) = song.file_path.as_deref() {
                    delete_relative_file(library, relative_path)?;
                }
                if let Some(cdg_path) = song.cdg_path.as_deref() {
                    delete_relative_file(library, cdg_path)?;
                }
            }
            MEDIA_G_ZIP => {
                if let Some(relative_path) = song.file_path.as_deref() {
                    delete_relative_file(library, relative_path)?;
                }
            }
            _ => {}
        }
    } else {
        if let Some(relative_path) = song.file_path.as_deref() {
            delete_relative_file(library, relative_path)?;
        }
    }
    Ok(())
}

/// Delete the stem directory for a song from the working copy filesystem.
/// Does NOT touch the database — safe to call after a DB transaction.
pub(crate) fn delete_stem_files_from_working_copy(
    library: &LibraryRoot,
    song_hash: &str,
) -> Result<()> {
    let dir = crate::cache::stems::stem_directory(&library.stems_dir(), song_hash);
    if dir.exists() {
        fs::remove_dir_all(&dir)
            .with_context(|| format!("failed to remove stem directory at {}", dir.display()))?;
    }
    Ok(())
}

fn delete_relative_file(library: &LibraryRoot, relative_path: &str) -> Result<()> {
    let absolute = library.resolve(relative_path);
    if absolute.exists() {
        fs::remove_file(&absolute)
            .with_context(|| format!("failed to remove {}", absolute.display()))?;
    }
    Ok(())
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .context("failed to inspect sqlite tables")?;
    Ok(count > 0)
}
