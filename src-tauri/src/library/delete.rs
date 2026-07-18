use crate::{
    cache,
    library::Song,
    library_root::LibraryRoot,
    media_g::{MEDIA_G_PAIRED, MEDIA_G_ZIP},
};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::fs;

pub fn delete_song_from_library(
    connection: &Connection,
    library: &LibraryRoot,
    song_id: &str,
) -> Result<()> {
    let song = cache::get_song_by_hash(connection, song_id)
        .context("failed to load song from library")?
        .with_context(|| format!("song with hash {song_id} was not found in the library"))?;

    // Collect artwork derivative paths before DB rows are deleted so we can
    // clean up the on-disk files afterwards (only if no other song references
    // them — two songs can share the same cover art bytes/digest).
    let artwork_paths = collect_artwork_derivative_paths(connection, song_id)?;

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

    // Delete DB rows (lyrics, play_history, stems, song).
    delete_song_rows_from_database(connection, library, song_id)?;
    // Clean up the on-disk stem directory (not handled by the DB-only delete).
    delete_stem_files_from_working_copy(library, song_id)?;
    // Clean up artwork derivative files (best-effort, only if unreferenced).
    for path in artwork_paths {
        let _ = crate::library::artwork::delete_artwork_derivative_if_unreferenced(
            connection, library, &path,
        );
    }
    Ok(())
}

/// Collect recorded artwork derivative paths for a song before its DB row is
/// deleted. Returns paths that exist in the database (may be empty).
pub(crate) fn collect_artwork_derivative_paths(
    connection: &Connection,
    song_id: &str,
) -> Result<Vec<String>> {
    let record = cache::get_artwork_record(connection, song_id)
        .context("failed to load artwork record for song")?;
    let Some(record) = record else {
        return Ok(Vec::new());
    };
    let mut paths = Vec::new();
    if let Some(p) = record.artwork_thumb_path {
        paths.push(p);
    }
    if let Some(p) = record.artwork_preview_path {
        paths.push(p);
    }
    Ok(paths)
}

/// Delete only the database rows for a song (lyrics, play_history, stems, song).
/// Unlike `delete_song_from_library`, this does NOT touch the filesystem —
/// safe to call inside a SQLite transaction. The caller is responsible for
/// deleting any working-copy or cloud files separately.
pub fn delete_song_rows_from_database(
    connection: &Connection,
    _library: &LibraryRoot,
    song_id: &str,
) -> Result<()> {
    // DB-only stem delete — no filesystem side effects, safe inside a transaction.
    cache::stems::delete_stem_cache_entry_db_only(connection, song_id)?;
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

/// Delete working-copy files for a song (audio, CDG, media_g containers, stems,
/// artwork derivatives). Does NOT touch the database — safe to call after a DB
/// transaction has already committed. Used by mirror sync to clean up the
/// remote working copy after transactional DB deletes.
pub fn delete_song_files_from_working_copy(library: &LibraryRoot, song: &Song) -> Result<()> {
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
    // Artwork derivative files are cleaned up separately by
    // delete_artwork_derivative_if_unreferenced (called from
    // delete_song_from_library and mirror sync) because they require a
    // DB reference count check — two songs can share the same cover digest.
    Ok(())
}

/// Delete the stem directory for a song from the working copy filesystem.
/// Does NOT touch the database — safe to call after a DB transaction.
pub fn delete_stem_files_from_working_copy(library: &LibraryRoot, song_hash: &str) -> Result<()> {
    if !is_safe_stem_directory_name(song_hash) {
        anyhow::bail!("refusing to delete a stem directory for an invalid song hash");
    }

    let stems_root = library.stems_dir();
    let stems_root_metadata = match fs::symlink_metadata(&stems_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to inspect stem root at {}", stems_root.display())
            });
        }
    };
    if stems_root_metadata.file_type().is_symlink() || !stems_root_metadata.is_dir() {
        anyhow::bail!("stem root must be a real directory");
    }

    let dir = crate::cache::stems::stem_directory(&stems_root, song_hash);
    let metadata = match fs::symlink_metadata(&dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect stem directory at {}", dir.display()));
        }
    };

    if metadata.file_type().is_symlink() {
        // Removing the link itself is safe; never recurse through a link.
        // Windows represents a directory symlink differently from a file
        // symlink, so fall back to `remove_dir` without ever following it.
        if let Err(remove_file_error) = fs::remove_file(&dir) {
            fs::remove_dir(&dir).with_context(|| {
                format!(
                    "failed to remove stem symlink at {} after remove_file failed: {remove_file_error}",
                    dir.display()
                )
            })?;
        }
    } else if metadata.is_dir() {
        fs::remove_dir_all(&dir)
            .with_context(|| format!("failed to remove stem directory at {}", dir.display()))?;
    } else {
        anyhow::bail!("stem cache path must be a directory or symlink");
    }
    Ok(())
}

/// Song identifiers are used as direct child directory names in `stems/`.
/// Treat database contents as untrusted at the filesystem boundary so a
/// malformed row cannot turn cleanup into a traversal outside the library.
fn is_safe_stem_directory_name(song_hash: &str) -> bool {
    !song_hash.is_empty()
        && song_hash != "."
        && song_hash != ".."
        && !song_hash.contains('/')
        && !song_hash.contains('\\')
        && !song_hash.contains('\0')
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_library() -> (TempDir, LibraryRoot) {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("library");
        let library = LibraryRoot::create(&root).unwrap();
        (temp, library)
    }

    #[test]
    fn stem_directory_name_rejects_path_traversal_and_platform_separators() {
        for invalid in ["", ".", "..", "../outside", "a/b", r"a\b", "bad\0hash"] {
            assert!(!is_safe_stem_directory_name(invalid), "{invalid:?}");
        }
        assert!(is_safe_stem_directory_name("song-hash_123"));
    }

    #[test]
    fn stem_cleanup_rejects_traversal_before_touching_the_filesystem() {
        let (temp, library) = test_library();
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("must-remain.txt");
        fs::write(&sentinel, b"keep").unwrap();

        assert!(delete_stem_files_from_working_copy(&library, "../outside").is_err());
        assert!(sentinel.exists());
    }

    #[test]
    fn stem_cleanup_removes_only_a_direct_child_directory() {
        let (_temp, library) = test_library();
        let stem_dir = library.stems_dir().join("song-hash_123");
        fs::create_dir_all(&stem_dir).unwrap();
        fs::write(stem_dir.join("vocals.wav"), b"stem").unwrap();

        delete_stem_files_from_working_copy(&library, "song-hash_123").unwrap();
        assert!(!stem_dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn stem_cleanup_unlinks_a_direct_child_symlink_without_following_it() {
        use std::os::unix::fs::symlink;

        let (temp, library) = test_library();
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("must-remain.txt");
        fs::write(&sentinel, b"keep").unwrap();
        let symlink_path = library.stems_dir().join("song-hash_123");
        symlink(&outside, &symlink_path).unwrap();

        delete_stem_files_from_working_copy(&library, "song-hash_123").unwrap();
        assert!(!symlink_path.exists());
        assert!(sentinel.exists());
    }

    #[cfg(unix)]
    #[test]
    fn stem_cleanup_refuses_a_symlinked_stem_root() {
        use std::os::unix::fs::symlink;

        let (temp, library) = test_library();
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("must-remain.txt");
        fs::write(&sentinel, b"keep").unwrap();
        let stems_root = library.stems_dir();
        fs::remove_dir(&stems_root).unwrap();
        symlink(&outside, &stems_root).unwrap();

        assert!(delete_stem_files_from_working_copy(&library, "song-hash_123").is_err());
        assert!(sentinel.exists());
    }
}
