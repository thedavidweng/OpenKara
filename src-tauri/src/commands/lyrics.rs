use crate::{
    cache,
    cache::lyrics::LyricsCacheEntry,
    commands::error::{database_error, internal_error, CommandResult},
    library::Song,
    library_root::LibraryRoot,
    lyrics::{
        self,
        error::LyricsError,
        fetch::{
            fetch_online_timed_lyrics_async, lookup_query_from_song, LyricsFetchResult,
            LyricsSource, TimedLyricsProvider, TimedLyricsProviderAsync,
        },
        lrcapi::LrcApiClient,
        lrclib::{LrcLibClient, LyricsLookupQuery},
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

#[tauri::command]
pub async fn fetch_lyrics(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
) -> CommandResult<LyricsPayload> {
    let background_state = state.inner().clone();

    let song_id_for_phase1 = song_id.clone();
    let phase1 = tauri::async_runtime::spawn_blocking(move || {
        fetch_lyrics_phase1(&background_state, &song_id_for_phase1)
    })
    .await
    .map_err(|error| internal_error(format!("fetch_lyrics task failed: {error}")))??;

    match phase1 {
        FetchLyricsPhase1::Ready(payload) => Ok(payload),
        FetchLyricsPhase1::LocalLyrics { fetched, song_hash } => {
            // Local lyrics must go through run_song_database_mutation so that
            // prepare → sync_db → publish_song runs around the cache write.
            let state_for_phase3 = state.inner().clone();
            let handle_for_phase3 = app_handle.clone();
            let song_id_for_phase3 = song_id.clone();
            tauri::async_runtime::spawn_blocking(move || {
                fetch_lyrics_phase3(
                    &state_for_phase3,
                    &handle_for_phase3,
                    &song_id_for_phase3,
                    &song_hash,
                    Ok(Some(fetched)),
                )
            })
            .await
            .map_err(|error| internal_error(format!("fetch_lyrics task failed: {error}")))?
        }
        FetchLyricsPhase1::NeedOnline { query, song_hash } => {
            // Reuse the shared HTTP clients from AppState so connection pools
            // persist across song switches instead of a fresh TLS handshake
            // per fetch.
            let lrclib_client = &state.inner().lrclib_client;
            let lrcapi_client = &state.inner().lrcapi_client;
            let providers = [
                TimedLyricsProviderAsync::LrcLib(lrclib_client),
                TimedLyricsProviderAsync::LrcApi(lrcapi_client),
            ];
            let online_result = fetch_online_timed_lyrics_async(&providers, &query)
                .await
                .map_err(|e| LyricsError::NetworkUnavailable(e.to_string()));

            let state_for_phase3 = state.inner().clone();
            let handle_for_phase3 = app_handle.clone();
            let song_id_for_phase3 = song_id.clone();
            tauri::async_runtime::spawn_blocking(move || {
                fetch_lyrics_phase3(
                    &state_for_phase3,
                    &handle_for_phase3,
                    &song_id_for_phase3,
                    &song_hash,
                    online_result,
                )
            })
            .await
            .map_err(|error| internal_error(format!("fetch_lyrics task failed: {error}")))?
        }
    }
}

enum FetchLyricsPhase1 {
    Ready(LyricsPayload),
    LocalLyrics {
        fetched: LyricsFetchResult,
        song_hash: String,
    },
    NeedOnline {
        query: LyricsLookupQuery,
        song_hash: String,
    },
}

// Read-only: does NOT write the cache or call prepare/sync/publish. Cache
// writes and remote sync happen in phase 3, wrapped in
// run_song_database_mutation.
fn fetch_lyrics_phase1(state: &AppState, song_id: &str) -> CommandResult<FetchLyricsPhase1> {
    let library_root = state.library_root()?;
    let connection = cache::open_database(&library_root.database_path()).map_err(database_error)?;

    let song = cache::get_song_by_hash(&connection, song_id)
        .map_err(|e| database_error(e.to_string()))?
        .ok_or(LyricsError::SongNotFound(song_id.to_string()))?;

    // Cache hit — return immediately (no DB write, no remote sync needed).
    // Negative cache (Absent) entries expire after NEGATIVE_CACHE_TTL_SECS so
    // lyrics added to LRCLIB/LrcAPI later can be discovered on re-fetch.
    if let Some(cached) = cache::lyrics::get_lyrics_cache_entry(&connection, song_id)
        .map_err(|e| database_error(e.to_string()))?
    {
        if cached.source == LyricsSource::Absent {
            if !is_negative_cache_expired(&cached) {
                return Ok(FetchLyricsPhase1::Ready(empty_lyrics_payload(song.hash)));
            }
        } else {
            let payload = payload_from_cached_entry(song.hash, cached)?;
            return Ok(FetchLyricsPhase1::Ready(payload));
        }
    }

    // The cache write is deferred to phase 3 so it goes through
    // run_song_database_mutation (prepare → sync_db → publish_song).
    if let Some(song_path) = song.file_path.as_deref() {
        let resolved_path = library_root.resolve(song_path);
        if let Some(fetched) = lyrics::fetch::fetch_lyrics_for_song_local(&song, &resolved_path)
            .map_err(|e| LyricsError::Internal(e.to_string()))?
        {
            return Ok(FetchLyricsPhase1::LocalLyrics {
                fetched,
                song_hash: song.hash,
            });
        }
    }

    match lookup_query_from_song(&song) {
        Some(query) => Ok(FetchLyricsPhase1::NeedOnline {
            query,
            song_hash: song.hash,
        }),
        None => Ok(FetchLyricsPhase1::Ready(empty_lyrics_payload(song.hash))),
    }
}

// Wrapped in run_song_database_mutation so prepare/sync/publish happen
// around the DB write.
fn fetch_lyrics_phase3(
    state: &AppState,
    app_handle: &AppHandle,
    song_id: &str,
    song_hash: &str,
    online_result: Result<Option<LyricsFetchResult>, LyricsError>,
) -> CommandResult<LyricsPayload> {
    let library_root = state.library_root()?;
    let connection = cache::open_database(&library_root.database_path()).map_err(database_error)?;

    remote::run_song_database_mutation(state, app_handle, song_id, || {
        let result: Result<LyricsPayload, LyricsError> = match online_result {
            Ok(Some(fetched)) => {
                let lines = lyrics::fetch::parse_lyrics_auto(&fetched.raw_lrc)
                    .map_err(|e| LyricsError::LyricsNotReady(e.to_string()))?;
                let raw_lrc = fetched.raw_lrc.clone();
                let source = fetched.source.clone();
                let offset_ms = lyrics::parser::parse_lrc_metadata(&raw_lrc)
                    .offset_ms
                    .unwrap_or(0);
                cache::lyrics::upsert_lyrics_cache_entry(
                    &connection,
                    &LyricsCacheEntry {
                        song_hash: song_hash.to_owned(),
                        lrc: fetched.raw_lrc,
                        source: source.clone(),
                        offset_ms,
                        fetched_at: current_unix_timestamp()
                            .map_err(|e| LyricsError::Internal(e.to_string()))?,
                    },
                )
                .map_err(|e| LyricsError::DatabaseUnavailable(e.to_string()))?;

                Ok(LyricsPayload {
                    song_id: song_hash.to_owned(),
                    lines,
                    source: Some(source),
                    offset_ms,
                    raw_lrc,
                })
            }
            Ok(None) => {
                cache_negative_lyrics_lookup(&connection, song_hash)?;
                Ok(empty_lyrics_payload(song_hash.to_owned()))
            }
            Err(_) => Ok(empty_lyrics_payload(song_hash.to_owned())),
        };
        result.map_err(Into::into)
    })
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
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    remote::run_song_database_mutation(state, app_handle, song_id, || {
        set_lyrics_offset_in_connection(&connection, song_id, ms).map_err(Into::into)
    })
}

pub fn fetch_lyrics_from_connection(
    connection: &Connection,
    library_root: &LibraryRoot,
    lrclib_client: &LrcLibClient,
    lrcapi_client: &LrcApiClient,
    song_id: &str,
) -> Result<LyricsPayload, LyricsError> {
    let song = cache::get_song_by_hash(connection, song_id)
        .map_err(|e| LyricsError::DatabaseUnavailable(e.to_string()))?
        .ok_or(LyricsError::SongNotFound(song_id.to_string()))?;

    // Lyrics are cached by the stable song hash so repeat fetches can skip both
    // network and filesystem fallbacks once a synced source has been resolved.
    // Negative cache (Absent) entries expire after NEGATIVE_CACHE_TTL_SECS.
    if let Some(cached) = cache::lyrics::get_lyrics_cache_entry(connection, song_id)
        .map_err(|e| LyricsError::DatabaseUnavailable(e.to_string()))?
    {
        if cached.source == LyricsSource::Absent && !is_negative_cache_expired(&cached) {
            return Ok(empty_lyrics_payload(song.hash));
        }
        if cached.source != LyricsSource::Absent {
            return payload_from_cached_entry(song.hash, cached);
        }
    }

    let Some(song_path) = song.file_path.as_deref() else {
        return Ok(LyricsPayload {
            song_id: song.hash,
            lines: Vec::new(),
            source: None,
            offset_ms: 0,
            raw_lrc: String::new(),
        });
    };
    let resolved_path = library_root.resolve(song_path);
    let providers = [
        TimedLyricsProvider::LrcLib(lrclib_client),
        TimedLyricsProvider::LrcApi(lrcapi_client),
    ];

    // Online requests are opportunistic: if they fail, we still want embedded
    // and sidecar sources to rescue the fetch instead of failing the whole song.
    let Some(fetched) = lyrics::fetch::fetch_lyrics_for_song(&providers, &song, &resolved_path)
        .map_err(|e| LyricsError::Internal(e.to_string()))?
    else {
        cache_negative_lyrics_lookup(connection, &song.hash)?;
        return Ok(empty_lyrics_payload(song.hash));
    };

    let lines = lyrics::fetch::parse_lyrics_auto(&fetched.raw_lrc)
        .map_err(|e| LyricsError::LyricsNotReady(e.to_string()))?;
    let source = fetched.source;
    let raw_lrc = fetched.raw_lrc.clone();
    let offset_ms = lyrics::parser::parse_lrc_metadata(&raw_lrc)
        .offset_ms
        .unwrap_or(0);
    cache::lyrics::upsert_lyrics_cache_entry(
        connection,
        &LyricsCacheEntry {
            song_hash: song.hash.clone(),
            lrc: fetched.raw_lrc,
            source: source.clone(),
            offset_ms,
            fetched_at: current_unix_timestamp()
                .map_err(|e| LyricsError::Internal(e.to_string()))?,
        },
    )
    .map_err(|e| LyricsError::DatabaseUnavailable(e.to_string()))?;

    Ok(LyricsPayload {
        song_id: song.hash,
        lines,
        source: Some(source),
        offset_ms,
        raw_lrc,
    })
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
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    let publish_song_id = song_id.to_owned();
    remote::run_song_database_mutation(state, app_handle, song_id, || {
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
            &connection,
            &LyricsCacheEntry {
                song_hash: publish_song_id.clone(),
                lrc: text,
                source: source.clone(),
                offset_ms,
                fetched_at,
            },
        )
        .map_err(|e| database_error(e.to_string()))?;

        Ok(LyricsPayload {
            song_id: publish_song_id,
            lines,
            source: Some(source),
            offset_ms,
            raw_lrc,
        })
    })
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
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    remote::run_songs_database_mutation(
        state,
        app_handle,
        || {
            let all_songs =
                cache::list_songs(&connection).map_err(|e| database_error(e.to_string()))?;

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

                    if let Err(e) = cache::lyrics::upsert_lyrics_cache_entry(&connection, &entry) {
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
        |result| {
            result
                .matched
                .iter()
                .map(|entry| entry.song_id.clone())
                .collect()
        },
    )
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
    let library_root = state.library_root()?;
    let connection = cache::open_database(&library_root.database_path()).map_err(database_error)?;

    let publish_song_id = song_id.to_owned();
    remote::run_song_database_mutation(state, app_handle, song_id, || {
        let song = cache::get_song_by_hash(&connection, &publish_song_id)
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
            &connection,
            &LyricsCacheEntry {
                song_hash: publish_song_id.clone(),
                lrc: embedded,
                source: LyricsSource::Embedded,
                offset_ms,
                fetched_at,
            },
        )
        .map_err(|e| database_error(e.to_string()))?;

        Ok(LyricsPayload {
            song_id: publish_song_id,
            lines,
            source: Some(LyricsSource::Embedded),
            offset_ms,
            raw_lrc,
        })
    })
}

#[tauri::command]
pub async fn fetch_lyrics_online(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
) -> CommandResult<LyricsPayload> {
    let background_state = state.inner().clone();
    let song_id_for_phase1 = song_id.clone();
    let phase1 = tauri::async_runtime::spawn_blocking(move || {
        fetch_lyrics_online_phase1(&background_state, &song_id_for_phase1)
    })
    .await
    .map_err(|error| internal_error(format!("fetch_lyrics_online task failed: {error}")))??;

    match phase1 {
        FetchOnlinePhase1::NoQuery(payload) => Ok(payload),
        FetchOnlinePhase1::Fetch { query, song_hash } => {
            // Reuse the shared HTTP clients from AppState for connection pool
            // reuse across calls.
            let lrclib_client = &state.inner().lrclib_client;
            let lrcapi_client = &state.inner().lrcapi_client;
            let providers = [
                TimedLyricsProviderAsync::LrcLib(lrclib_client),
                TimedLyricsProviderAsync::LrcApi(lrcapi_client),
            ];
            let online_result = fetch_online_timed_lyrics_async(&providers, &query)
                .await
                .map_err(|e| LyricsError::NetworkUnavailable(e.to_string()));

            let state_for_phase3 = state.inner().clone();
            let handle_for_phase3 = app_handle.clone();
            tauri::async_runtime::spawn_blocking(move || {
                fetch_lyrics_online_phase3(
                    &state_for_phase3,
                    &handle_for_phase3,
                    &song_hash,
                    online_result,
                )
            })
            .await
            .map_err(|error| internal_error(format!("fetch_lyrics_online task failed: {error}")))?
        }
    }
}

enum FetchOnlinePhase1 {
    NoQuery(LyricsPayload),
    Fetch {
        query: LyricsLookupQuery,
        song_hash: String,
    },
}

fn fetch_lyrics_online_phase1(state: &AppState, song_id: &str) -> CommandResult<FetchOnlinePhase1> {
    let library_root = state.library_root()?;
    let connection = cache::open_database(&library_root.database_path()).map_err(database_error)?;

    let song = cache::get_song_by_hash(&connection, song_id)
        .map_err(|e| database_error(e.to_string()))?
        .ok_or(LyricsError::SongNotFound(song_id.to_owned()))?;

    match lookup_query_from_song(&song) {
        Some(query) => Ok(FetchOnlinePhase1::Fetch {
            query,
            song_hash: song.hash,
        }),
        None => Ok(FetchOnlinePhase1::NoQuery(LyricsPayload {
            song_id: song.hash,
            lines: Vec::new(),
            source: None,
            offset_ms: 0,
            raw_lrc: String::new(),
        })),
    }
}

fn fetch_lyrics_online_phase3(
    state: &AppState,
    app_handle: &AppHandle,
    song_hash: &str,
    online_result: Result<Option<LyricsFetchResult>, LyricsError>,
) -> CommandResult<LyricsPayload> {
    let library_root = state.library_root()?;
    let connection = cache::open_database(&library_root.database_path()).map_err(database_error)?;

    remote::run_song_database_mutation_with_result(
        state,
        app_handle,
        || {
            let result: Result<LyricsPayload, LyricsError> = match online_result {
                Ok(Some(fetched)) => {
                    let lines = lyrics::fetch::parse_lyrics_auto(&fetched.raw_lrc)
                        .map_err(|e| LyricsError::LyricsNotReady(e.to_string()))?;
                    let raw_lrc = fetched.raw_lrc.clone();
                    let source = fetched.source.clone();
                    let offset_ms = lyrics::parser::parse_lrc_metadata(&raw_lrc)
                        .offset_ms
                        .unwrap_or(0);
                    let fetched_at = current_unix_timestamp()
                        .map_err(|e| LyricsError::Internal(e.to_string()))?;

                    cache::lyrics::upsert_lyrics_cache_entry(
                        &connection,
                        &LyricsCacheEntry {
                            song_hash: song_hash.to_owned(),
                            lrc: fetched.raw_lrc,
                            source: source.clone(),
                            offset_ms,
                            fetched_at,
                        },
                    )
                    .map_err(|e| LyricsError::DatabaseUnavailable(e.to_string()))?;

                    Ok(LyricsPayload {
                        song_id: song_hash.to_owned(),
                        lines,
                        source: Some(source),
                        offset_ms,
                        raw_lrc,
                    })
                }
                Ok(None) => Ok(LyricsPayload {
                    song_id: song_hash.to_owned(),
                    lines: Vec::new(),
                    source: None,
                    offset_ms: 0,
                    raw_lrc: String::new(),
                }),
                Err(e) => Err(e),
            };
            result.map_err(Into::into)
        },
        |payload| payload.source.as_ref().map(|_| payload.song_id.clone()),
    )
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

fn cache_negative_lyrics_lookup(
    connection: &rusqlite::Connection,
    song_hash: &str,
) -> Result<(), LyricsError> {
    let fetched_at = current_unix_timestamp().map_err(|e| LyricsError::Internal(e.to_string()))?;
    cache::lyrics::upsert_lyrics_cache_entry(
        connection,
        &LyricsCacheEntry {
            song_hash: song_hash.to_owned(),
            lrc: String::new(),
            source: LyricsSource::Absent,
            offset_ms: 0,
            fetched_at,
        },
    )
    .map_err(|e| LyricsError::DatabaseUnavailable(e.to_string()))?;
    Ok(())
}

use super::error::current_unix_timestamp;

/// Negative cache TTL: how long an Absent entry suppresses online re-lookup.
/// LRCLIB and LrcAPI are community-edited, so lyrics may appear later. A 7-day
/// TTL balances re-discovery against hammering the APIs on every song change.
const NEGATIVE_CACHE_TTL_SECS: i64 = 7 * 24 * 60 * 60;

fn is_negative_cache_expired(entry: &LyricsCacheEntry) -> bool {
    match current_unix_timestamp() {
        Ok(now) => now - entry.fetched_at > NEGATIVE_CACHE_TTL_SECS,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absent_entry(fetched_at: i64) -> LyricsCacheEntry {
        LyricsCacheEntry {
            song_hash: "test".to_owned(),
            lrc: String::new(),
            source: LyricsSource::Absent,
            offset_ms: 0,
            fetched_at,
        }
    }

    #[test]
    fn negative_cache_expired_after_ttl() {
        let now = current_unix_timestamp().unwrap();
        // 8 days ago — past the 7-day TTL.
        let entry = absent_entry(now - 8 * 24 * 60 * 60);
        assert!(is_negative_cache_expired(&entry));
    }

    #[test]
    fn negative_cache_not_expired_within_ttl() {
        let now = current_unix_timestamp().unwrap();
        // 1 day ago — within the 7-day TTL.
        let entry = absent_entry(now - 24 * 60 * 60);
        assert!(!is_negative_cache_expired(&entry));
    }

    #[test]
    fn negative_cache_not_expired_when_fresh() {
        let now = current_unix_timestamp().unwrap();
        let entry = absent_entry(now);
        assert!(!is_negative_cache_expired(&entry));
    }
}
