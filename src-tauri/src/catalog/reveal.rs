use super::types::{CatalogError, RevealTarget, RevealTargets};
use crate::cache;
use crate::library_root::LibraryRoot;
use rusqlite::Connection;
use std::path::Path;

pub fn get_reveal_targets(
    connection: &Connection,
    library: &LibraryRoot,
    song_id: &str,
) -> Result<RevealTargets, CatalogError> {
    let song = cache::get_song_by_hash(connection, song_id)
        .map_err(|error| CatalogError::Internal(error.to_string()))?
        .ok_or_else(|| CatalogError::Internal(format!("song {song_id} was not found")))?;

    let song_file = match song.file_path.as_deref() {
        Some(relative) => {
            let absolute = library.resolve(relative);
            if absolute.exists() {
                RevealTarget {
                    available: true,
                    path: Some(absolute.display().to_string()),
                }
            } else {
                RevealTarget {
                    available: false,
                    path: Some(absolute.display().to_string()),
                }
            }
        }
        None => RevealTarget {
            available: false,
            path: None,
        },
    };

    let stems_dir = crate::cache::stems::stem_directory(&library.stems_dir(), song_id);
    let stems = if stems_dir.exists() {
        RevealTarget {
            available: true,
            path: Some(stems_dir.display().to_string()),
        }
    } else {
        RevealTarget {
            available: false,
            path: Some(stems_dir.display().to_string()),
        }
    };

    Ok(RevealTargets { song_file, stems })
}

pub fn reveal_path(path: &str) -> Result<(), CatalogError> {
    let path = Path::new(path);
    if !path.exists() {
        return Err(CatalogError::Internal(format!(
            "nothing to reveal at {}",
            path.display()
        )));
    }
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|error| {
        CatalogError::Internal(format!("could not reveal {}: {error}", path.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::apply_migrations;
    use crate::library::Song;
    use crate::library_root::LibraryRoot;
    use rusqlite::Connection;

    fn library_with_song(tmp: &tempfile::TempDir) -> (LibraryRoot, Connection, String) {
        let library = LibraryRoot::create(&tmp.path().join("lib")).expect("library");
        let connection = Connection::open_in_memory().expect("db");
        apply_migrations(&connection).expect("migrations");
        let media = library.root().join("media");
        std::fs::create_dir_all(&media).expect("media");
        let relative = "media/abc.mp3";
        std::fs::write(library.resolve(relative), b"audio").expect("write");
        let song = Song {
            hash: "abc".to_owned(),
            file_path: Some(relative.to_owned()),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: Some("Song".to_owned()),
            artist: Some("Artist".to_owned()),
            album: None,
            duration_ms: 1000,
            has_cover_art: false,
            artwork_thumb_path: None,
            cover_art: None,
            imported_at: 1,
            original_ext: Some("mp3".to_owned()),
        };
        cache::upsert_song(&connection, &song).expect("upsert");
        (library, connection, song.hash)
    }

    #[test]
    fn existing_relative_media_resolves_absolute_missing_is_disabled() {
        let tmp = tempfile::tempdir().expect("tmp");
        let (library, connection, hash) = library_with_song(&tmp);
        let targets = get_reveal_targets(&connection, &library, &hash).expect("targets");
        assert!(targets.song_file.available);
        let path = targets.song_file.path.expect("path");
        assert!(path.ends_with("abc.mp3"));
        assert!(std::path::Path::new(&path).is_absolute());
        assert!(!targets.stems.available);

        std::fs::remove_file(library.resolve("media/abc.mp3")).expect("remove");
        let targets = get_reveal_targets(&connection, &library, &hash).expect("targets");
        assert!(!targets.song_file.available);
    }

    #[test]
    fn existing_stem_folder_is_available() {
        let tmp = tempfile::tempdir().expect("tmp");
        let (library, connection, hash) = library_with_song(&tmp);
        let stems = crate::cache::stems::stem_directory(&library.stems_dir(), &hash);
        std::fs::create_dir_all(&stems).expect("stems");
        let targets = get_reveal_targets(&connection, &library, &hash).expect("targets");
        assert!(targets.stems.available);
        assert_eq!(
            targets.stems.path.as_deref(),
            Some(stems.display().to_string().as_str())
        );
    }
}
