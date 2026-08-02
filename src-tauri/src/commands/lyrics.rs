use crate::{
    cache,
    cache::lyrics::LyricsCacheEntry,
    commands::error::{current_unix_timestamp, database_error, internal_error, CommandResult},
    library::Song,
    library_root::LibraryRoot,
    lyrics::{
        self,
        acquisition::LyricsPersistenceResult,
        acquisition::{LyricsAcquisition, LyricsAcquisitionResult},
        error::LyricsError,
        fetch::{LyricsFetchResult, LyricsSource},
        parser::LyricLine,
    },
    remote, AppState,
};
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, State};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LyricsPayload {
    pub song_id: String,
    pub lines: Vec<LyricLine>,
    pub source: Option<LyricsSource>,
    pub offset_ms: i64,
    pub raw_lrc: String,
}

pub use crate::lyrics::acquisition::LyricsOnlineFetchIntent;

#[tauri::command]
pub async fn fetch_lyrics(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
) -> CommandResult<LyricsPayload> {
    let state = state.inner().clone();
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        fetch_lyrics_on_thread(&state, &app_handle, &song_id)
    })
    .await
    .map_err(|error| internal_error(format!("fetch_lyrics task failed: {error}")))?
}

fn fetch_lyrics_on_thread(
    state: &AppState,
    app_handle: &AppHandle,
    song_id: &str,
) -> CommandResult<LyricsPayload> {
    let library_root = state.library_root()?;
    let connection = cache::open_database(&library_root.database_path()).map_err(database_error)?;
    let acquisition = LyricsAcquisition::new(&state.lrclib_client, &state.lrcapi_client);
    let acquired = acquisition.acquire(&connection, &library_root, song_id)?;

    match acquired {
        LyricsAcquisitionResult::Cached(cached) => {
            Ok(payload_from_cached_entry(song_id.to_owned(), cached)?)
        }
        LyricsAcquisitionResult::NegativeCacheHit
        | LyricsAcquisitionResult::Absent {
            cache_negative: false,
        } => Ok(empty_lyrics_payload(song_id.to_owned())),
        result @ (LyricsAcquisitionResult::Fetched(_)
        | LyricsAcquisitionResult::Absent {
            cache_negative: true,
        }) => {
            let publication = remote::PublishChanges::new(state, app_handle);
            let applied = publication.apply(remote::Change::new(
                remote::ChangeScope::Songs(vec![song_id.to_owned()]),
                move |connection: &Connection, _library: &LibraryRoot| {
                    LyricsAcquisition::persist_acquisition(connection, song_id, &result)
                        .map_err(Into::into)
                },
                |result: &LyricsPersistenceResult| {
                    if result.changed {
                        remote::ChangeScope::Songs(vec![song_id.to_owned()])
                    } else {
                        remote::ChangeScope::None
                    }
                },
            ))?;
            publication.publish(&applied.scope)?;
            match applied.value.fetched {
                Some(fetched) => Ok(payload_from_fetched(song_id.to_owned(), &fetched)?),
                None => Ok(empty_lyrics_payload(song_id.to_owned())),
            }
        }
    }
}

#[tauri::command]
pub async fn set_lyrics_offset(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
    ms: i64,
) -> CommandResult<()> {
    let background_state = state.inner().clone();
    let background_handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        set_lyrics_offset_on_thread(&background_state, &background_handle, &song_id, ms)
    })
    .await
    .map_err(|error| internal_error(format!("set_lyrics_offset task failed: {error}")))?
}

fn set_lyrics_offset_on_thread(
    state: &AppState,
    app_handle: &AppHandle,
    song_id: &str,
    ms: i64,
) -> CommandResult<()> {
    let publication = remote::PublishChanges::new(state, app_handle);
    let applied = publication.apply(remote::Change::new(
        remote::ChangeScope::Songs(vec![song_id.to_owned()]),
        |connection: &Connection, _library: &LibraryRoot| {
            set_lyrics_offset_in_connection(connection, song_id, ms).map_err(Into::into)
        },
        |_: &()| remote::ChangeScope::Songs(vec![song_id.to_owned()]),
    ))?;
    publication.publish(&applied.scope)?;
    Ok(())
}

pub fn set_lyrics_offset_in_connection(
    connection: &Connection,
    song_id: &str,
    ms: i64,
) -> Result<(), LyricsError> {
    cache::get_song_by_hash(connection, song_id)
        .map_err(|e| LyricsError::DatabaseUnavailable(e.to_string()))?
        .ok_or(LyricsError::SongNotFound(song_id.to_string()))?;

    cache::lyrics::get_lyrics_cache_entry(connection, song_id)
        .map_err(|e| LyricsError::DatabaseUnavailable(e.to_string()))?
        .ok_or(LyricsError::LyricsNotReady(format!(
            "song {song_id} does not have cached lyrics"
        )))?;

    cache::lyrics::set_lyrics_offset(connection, song_id, ms)
        .map_err(|e| LyricsError::DatabaseUnavailable(e.to_string()))?;

    Ok(())
}

fn payload_from_cached_entry(
    song_id: String,
    cached: LyricsCacheEntry,
) -> Result<LyricsPayload, LyricsError> {
    if cached.source == LyricsSource::Absent {
        return Ok(empty_lyrics_payload(song_id));
    }

    let mut lines = lyrics::fetch::parse_lyrics_auto(&cached.lrc)
        .map_err(|e| LyricsError::LyricsNotReady(e.to_string()))?;

    if lines.is_empty() {
        lines = plain_text_to_lines(&cached.lrc);
    }

    Ok(LyricsPayload {
        song_id,
        lines,
        source: Some(cached.source),
        offset_ms: cached.offset_ms,
        raw_lrc: cached.lrc,
    })
}

fn payload_from_fetched(
    song_id: String,
    fetched: &LyricsFetchResult,
) -> Result<LyricsPayload, LyricsError> {
    let mut lines = lyrics::fetch::parse_lyrics_auto(&fetched.raw_lrc)
        .map_err(|error| LyricsError::LyricsNotReady(error.to_string()))?;
    if lines.is_empty() {
        lines = plain_text_to_lines(&fetched.raw_lrc);
    }
    let offset_ms = lyrics::parser::parse_lrc_metadata(&fetched.raw_lrc)
        .offset_ms
        .unwrap_or(0);
    Ok(LyricsPayload {
        song_id,
        lines,
        source: Some(fetched.source.clone()),
        offset_ms,
        raw_lrc: fetched.raw_lrc.clone(),
    })
}

#[tauri::command]
pub async fn save_manual_lyrics(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
    text: String,
) -> CommandResult<LyricsPayload> {
    let background_state = state.inner().clone();
    let background_handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        save_manual_lyrics_on_thread(&background_state, &background_handle, &song_id, text)
    })
    .await
    .map_err(|error| internal_error(format!("save_manual_lyrics task failed: {error}")))?
}

fn save_manual_lyrics_on_thread(
    state: &AppState,
    app_handle: &AppHandle,
    song_id: &str,
    text: String,
) -> CommandResult<LyricsPayload> {
    let publish_song_id = song_id.to_owned();
    let publication = remote::PublishChanges::new(state, app_handle);
    let applied = publication.apply(remote::Change::new(
        remote::ChangeScope::Songs(vec![song_id.to_owned()]),
        |connection: &Connection, _library: &LibraryRoot| {
            let lines = match lyrics::fetch::parse_lyrics_auto(&text) {
                Ok(parsed) if !parsed.is_empty() => parsed,
                _ => plain_text_to_lines(&text),
            };

            let source = {
                let trimmed = text.trim();
                if trimmed.starts_with("<?xml") || trimmed.starts_with("<tt") {
                    LyricsSource::ManualTtml
                } else if trimmed
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .is_some_and(|l| {
                        let bytes = l.trim().as_bytes();
                        bytes.starts_with(b"[")
                            && bytes.len() >= 3
                            && bytes[1].is_ascii_digit()
                            && bytes[2] == b']'
                    })
                {
                    LyricsSource::ManualLys
                } else {
                    LyricsSource::Manual
                }
            };

            let raw_lrc = text.clone();

            let offset_ms = lyrics::parser::parse_lrc_metadata(&raw_lrc)
                .offset_ms
                .unwrap_or(0);

            let fetched_at =
                current_unix_timestamp().map_err(|e| LyricsError::Internal(e.to_string()))?;

            cache::lyrics::upsert_lyrics_cache_entry(
                connection,
                &LyricsCacheEntry {
                    song_hash: publish_song_id.clone(),
                    lrc: text,
                    source: source.clone(),
                    offset_ms,
                    fetched_at,
                },
            )
            .map_err(database_error)?;

            Ok(LyricsPayload {
                song_id: publish_song_id,
                lines,
                source: Some(source),
                offset_ms,
                raw_lrc,
            })
        },
        |payload: &LyricsPayload| remote::ChangeScope::Songs(vec![payload.song_id.clone()]),
    ))?;
    publication.publish(&applied.scope)?;
    Ok(applied.value)
}

#[derive(Debug, Clone, Serialize)]
pub struct LyricsMatch {
    pub song_id: String,
    pub lrc_path: String,
    pub song_title: Option<String>,
    pub song_artist: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportLyricsResult {
    pub matched: Vec<LyricsMatch>,
    pub unmatched: Vec<String>,
}

#[tauri::command]
pub async fn import_lyrics_files(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    paths: Vec<String>,
) -> CommandResult<ImportLyricsResult> {
    let background_state = state.inner().clone();
    let background_handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        import_lyrics_files_on_thread(&background_state, &background_handle, paths)
    })
    .await
    .map_err(|error| internal_error(format!("import_lyrics_files task failed: {error}")))?
}

fn import_lyrics_files_on_thread(
    state: &AppState,
    app_handle: &AppHandle,
    paths: Vec<String>,
) -> CommandResult<ImportLyricsResult> {
    let publication = remote::PublishChanges::new(state, app_handle);
    let applied = publication.apply(remote::Change::new(
        remote::ChangeScope::WholeRepository,
        |connection: &Connection, _library: &LibraryRoot| {
            let all_songs = cache::list_songs(connection).map_err(database_error)?;

            let mut matched = Vec::new();
            let mut unmatched = Vec::new();

            for path_str in &paths {
                let path = Path::new(path_str);

                let content = match std::fs::read_to_string(path) {
                    Ok(c) => c,
                    Err(_) => {
                        unmatched.push(path_str.clone());
                        continue;
                    }
                };

                let lrc_stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_lowercase());

                let mut found_song: Option<&Song> = None;

                if let Some(ref stem) = lrc_stem {
                    found_song = all_songs.iter().find(|song| {
                        let Some(song_path) = song.file_path.as_deref() else {
                            return false;
                        };
                        let song_path = Path::new(song_path);
                        let song_stem = song_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .map(|s| s.to_lowercase());
                        song_stem.as_deref() == Some(stem.as_str())
                    });
                }

                if found_song.is_none() {
                    let meta = lyrics::parser::parse_lrc_metadata(&content);
                    if let (Some(ref lrc_artist), Some(ref lrc_title)) = (meta.artist, meta.title) {
                        let artist_lower = lrc_artist.to_lowercase();
                        let title_lower = lrc_title.to_lowercase();
                        found_song = all_songs.iter().find(|song| {
                            let song_artist = song.artist.as_deref().unwrap_or("").to_lowercase();
                            let song_title = song.title.as_deref().unwrap_or("").to_lowercase();
                            song_artist == artist_lower && song_title == title_lower
                        });
                    }
                }

                if let Some(song) = found_song {
                    let offset_ms = lyrics::parser::parse_lrc_metadata(&content)
                        .offset_ms
                        .unwrap_or(0);

                    let fetched_at = current_unix_timestamp()
                        .map_err(|e| LyricsError::Internal(e.to_string()))?;

                    let entry = LyricsCacheEntry {
                        song_hash: song.hash.clone(),
                        lrc: content,
                        source: LyricsSource::Manual,
                        offset_ms,
                        fetched_at,
                    };

                    if let Err(e) = cache::lyrics::upsert_lyrics_cache_entry(connection, &entry) {
                        eprintln!(
                            "failed to cache lyrics for {} ({}): {e}",
                            song.hash, path_str
                        );
                        unmatched.push(path_str.clone());
                        continue;
                    }

                    matched.push(LyricsMatch {
                        song_id: song.hash.clone(),
                        lrc_path: path_str.clone(),
                        song_title: song.title.clone(),
                        song_artist: song.artist.clone(),
                    });
                } else {
                    unmatched.push(path_str.clone());
                }
            }

            Ok(ImportLyricsResult { matched, unmatched })
        },
        |result: &ImportLyricsResult| {
            remote::ChangeScope::Songs(
                result
                    .matched
                    .iter()
                    .map(|entry| entry.song_id.clone())
                    .collect(),
            )
        },
    ))?;
    publication.publish(&applied.scope)?;
    Ok(applied.value)
}

#[tauri::command]
pub async fn extract_embedded_lyrics(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
) -> CommandResult<LyricsPayload> {
    let background_state = state.inner().clone();
    let background_handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        extract_embedded_lyrics_on_thread(&background_state, &background_handle, &song_id)
    })
    .await
    .map_err(|error| internal_error(format!("extract_embedded_lyrics task failed: {error}")))?
}

fn extract_embedded_lyrics_on_thread(
    state: &AppState,
    app_handle: &AppHandle,
    song_id: &str,
) -> CommandResult<LyricsPayload> {
    let publish_song_id = song_id.to_owned();
    let publication = remote::PublishChanges::new(state, app_handle);
    let applied = publication.apply(remote::Change::new(
        remote::ChangeScope::Songs(vec![song_id.to_owned()]),
        |connection: &Connection, library_root: &LibraryRoot| {
            let song = cache::get_song_by_hash(connection, &publish_song_id)
                .map_err(|e| LyricsError::DatabaseUnavailable(e.to_string()))?
                .ok_or(LyricsError::SongNotFound(publish_song_id.clone()))?;

            let Some(song_path) = song.file_path.as_deref() else {
                return Err(LyricsError::Internal(format!(
                    "song {} does not have a local file path",
                    publish_song_id
                ))
                .into());
            };
            let resolved_path = library_root.resolve(song_path);
            let embedded = lyrics::fetch::read_embedded_lyrics(&resolved_path)
                .map_err(|e| LyricsError::Internal(e.to_string()))?
                .ok_or(LyricsError::LyricsNotReady(
                    "No embedded lyrics found in this file".to_owned(),
                ))?;

            let lines = match lyrics::fetch::parse_lyrics_auto(&embedded) {
                Ok(parsed) if !parsed.is_empty() => parsed,
                _ => plain_text_to_lines(&embedded),
            };

            let raw_lrc = embedded.clone();

            let offset_ms = lyrics::parser::parse_lrc_metadata(&raw_lrc)
                .offset_ms
                .unwrap_or(0);

            let fetched_at =
                current_unix_timestamp().map_err(|e| LyricsError::Internal(e.to_string()))?;

            cache::lyrics::upsert_lyrics_cache_entry(
                connection,
                &LyricsCacheEntry {
                    song_hash: publish_song_id.clone(),
                    lrc: embedded,
                    source: LyricsSource::Embedded,
                    offset_ms,
                    fetched_at,
                },
            )
            .map_err(database_error)?;

            Ok(LyricsPayload {
                song_id: publish_song_id,
                lines,
                source: Some(LyricsSource::Embedded),
                offset_ms,
                raw_lrc,
            })
        },
        |payload: &LyricsPayload| remote::ChangeScope::Songs(vec![payload.song_id.clone()]),
    ))?;
    publication.publish(&applied.scope)?;
    Ok(applied.value)
}

#[tauri::command]
pub async fn fetch_lyrics_online(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
    intent: LyricsOnlineFetchIntent,
) -> CommandResult<LyricsPayload> {
    let state = state.inner().clone();
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        fetch_lyrics_online_on_thread(&state, &app_handle, &song_id, intent)
    })
    .await
    .map_err(|error| internal_error(format!("fetch_lyrics_online task failed: {error}")))?
}

fn fetch_lyrics_online_on_thread(
    state: &AppState,
    app_handle: &AppHandle,
    song_id: &str,
    intent: LyricsOnlineFetchIntent,
) -> CommandResult<LyricsPayload> {
    let library_root = state.library_root()?;
    let connection = cache::open_database(&library_root.database_path()).map_err(database_error)?;
    let acquisition = LyricsAcquisition::new(&state.lrclib_client, &state.lrcapi_client);
    let online_result = acquisition.fetch_online(&connection, song_id, intent)?;
    let should_publish = !matches!(
        online_result,
        crate::lyrics::fetch::OnlineLyricsResult::NotApplicable
    );
    let song_id_owned = song_id.to_owned();

    let publication = remote::PublishChanges::new(state, app_handle);
    let applied = publication.apply(remote::Change::new(
        remote::ChangeScope::Songs(vec![song_id_owned.clone()]),
        move |connection: &Connection, _library: &LibraryRoot| {
            LyricsAcquisition::persist_online_result(
                connection,
                &song_id_owned,
                online_result,
                intent,
            )
            .map_err(Into::into)
        },
        move |result: &LyricsPersistenceResult| {
            if should_publish && result.changed {
                remote::ChangeScope::Songs(vec![song_id.to_owned()])
            } else {
                remote::ChangeScope::None
            }
        },
    ))?;
    publication.publish(&applied.scope)?;
    match applied.value.fetched {
        Some(fetched) => Ok(payload_from_fetched(song_id.to_owned(), &fetched)?),
        None => Ok(empty_lyrics_payload(song_id.to_owned())),
    }
}

fn plain_text_to_lines(text: &str) -> Vec<LyricLine> {
    text.lines()
        .map(|l| LyricLine {
            time_ms: 0,
            text: l.to_string(),
            words: None,
            bg_words: None,
            section: None,
        })
        .collect()
}

fn empty_lyrics_payload(song_id: String) -> LyricsPayload {
    LyricsPayload {
        song_id,
        lines: Vec::new(),
        source: None,
        offset_ms: 0,
        raw_lrc: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        cache::apply_migrations(&conn).expect("migrations");
        conn
    }

    fn insert_song(conn: &Connection, hash: &str) {
        let song = crate::library::Song {
            hash: hash.to_owned(),
            file_path: Some(format!("media/{hash}.mp3")),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: Some("Title".to_owned()),
            artist: Some("Artist".to_owned()),
            album: None,
            duration_ms: 0,
            cover_art: None,
            has_cover_art: false,
            artwork_thumb_path: None,
            imported_at: 0,
            original_ext: None,
        };
        cache::upsert_song(conn, &song).expect("insert song");
    }

    fn seed_entry(conn: &Connection, hash: &str, source: LyricsSource, lrc: &str) {
        cache::lyrics::upsert_lyrics_cache_entry(
            conn,
            &LyricsCacheEntry {
                song_hash: hash.to_owned(),
                lrc: lrc.to_owned(),
                source,
                offset_ms: 0,
                fetched_at: 1_000_000,
            },
        )
        .expect("seed lyrics entry");
    }

    fn synced_online_result() -> Result<Option<LyricsFetchResult>, LyricsError> {
        Ok(Some(LyricsFetchResult {
            source: LyricsSource::LrcLib,
            raw_lrc: "[00:01.00]Online line one\n[00:02.00]Online line two\n".to_owned(),
        }))
    }

    fn apply_online_lyrics_result(
        connection: &Connection,
        song_hash: &str,
        online_result: Result<Option<LyricsFetchResult>, LyricsError>,
        intent: LyricsOnlineFetchIntent,
    ) -> Result<LyricsPersistenceResult, LyricsError> {
        let result = match online_result {
            Ok(Some(fetched)) => crate::lyrics::fetch::OnlineLyricsResult::Found(fetched),
            Ok(None) => crate::lyrics::fetch::OnlineLyricsResult::DefiniteMissing,
            Err(error) => return Err(error),
        };
        LyricsAcquisition::persist_online_result(connection, song_hash, result, intent)
    }

    // Issue #203: the silent auto-upgrade must never overwrite hand-entered
    // lyrics with an online match (which may be a wrong match for a mistagged
    // song).
    #[test]
    fn auto_upgrade_does_not_clobber_manual_entry() {
        let conn = test_db();
        insert_song(&conn, "song-manual");
        let manual_lrc = "Hand written line one\nHand written line two\n";
        seed_entry(&conn, "song-manual", LyricsSource::Manual, manual_lrc);

        let persisted = apply_online_lyrics_result(
            &conn,
            "song-manual",
            synced_online_result(),
            LyricsOnlineFetchIntent::AutomaticUpgrade,
        )
        .expect("apply should succeed");

        assert!(!persisted.changed);

        let stored = cache::lyrics::get_lyrics_cache_entry(&conn, "song-manual")
            .expect("get")
            .expect("entry exists");
        assert_eq!(stored.source, LyricsSource::Manual);
        assert_eq!(stored.lrc, manual_lrc);
    }

    #[test]
    fn auto_upgrade_preserves_manual_ttml_and_lys_and_sidecar() {
        for (hash, source) in [
            ("h-manual-ttml", LyricsSource::ManualTtml),
            ("h-manual-lys", LyricsSource::ManualLys),
            ("h-sidecar", LyricsSource::Sidecar),
            ("h-sidecar-ttml", LyricsSource::SidecarTtml),
            ("h-sidecar-lys", LyricsSource::SidecarLys),
        ] {
            let conn = test_db();
            insert_song(&conn, hash);
            seed_entry(&conn, hash, source.clone(), "user provided lyric\n");

            let persisted = apply_online_lyrics_result(
                &conn,
                hash,
                synced_online_result(),
                LyricsOnlineFetchIntent::AutomaticUpgrade,
            )
            .expect("apply should succeed");
            assert!(!persisted.changed, "source {source:?}");

            let stored = cache::lyrics::get_lyrics_cache_entry(&conn, hash)
                .expect("get")
                .expect("entry exists");
            assert_eq!(stored.source, source, "cache source {source:?}");
        }
    }

    #[test]
    fn user_replace_fetch_replaces_manual_entry() {
        let conn = test_db();
        insert_song(&conn, "song-manual");
        seed_entry(&conn, "song-manual", LyricsSource::Manual, "Hand written\n");

        let persisted = apply_online_lyrics_result(
            &conn,
            "song-manual",
            synced_online_result(),
            LyricsOnlineFetchIntent::UserReplace,
        )
        .expect("apply should succeed");

        assert!(persisted.changed);

        let stored = cache::lyrics::get_lyrics_cache_entry(&conn, "song-manual")
            .expect("get")
            .expect("entry exists");
        assert_eq!(stored.source, LyricsSource::LrcLib);
        assert!(stored.lrc.contains("Online line one"));
    }

    #[test]
    fn auto_upgrade_replaces_embedded_entry() {
        let conn = test_db();
        insert_song(&conn, "song-embedded");
        seed_entry(
            &conn,
            "song-embedded",
            LyricsSource::Embedded,
            "Embedded plain line\n",
        );

        let persisted = apply_online_lyrics_result(
            &conn,
            "song-embedded",
            synced_online_result(),
            LyricsOnlineFetchIntent::AutomaticUpgrade,
        )
        .expect("apply should succeed");

        assert!(persisted.changed);

        let stored = cache::lyrics::get_lyrics_cache_entry(&conn, "song-embedded")
            .expect("get")
            .expect("entry exists");
        assert_eq!(stored.source, LyricsSource::LrcLib);
    }
}
