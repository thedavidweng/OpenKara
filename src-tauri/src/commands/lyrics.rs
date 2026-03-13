use crate::{
    cache,
    cache::lyrics::LyricsCacheEntry,
    commands::error::{database_error, lyrics_error, CommandResult},
    lyrics::{self, fetch::LyricsSource, lrclib::LrcLibClient, parser::LyricLine},
    AppState,
};
use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LyricsPayload {
    pub song_id: String,
    pub lines: Vec<LyricLine>,
    pub source: Option<LyricsSource>,
    pub offset_ms: i64,
}

#[tauri::command]
pub fn fetch_lyrics(state: State<'_, AppState>, song_id: String) -> CommandResult<LyricsPayload> {
    let connection = cache::open_database(&state.database_path).map_err(database_error)?;

    fetch_lyrics_from_connection(&connection, &LrcLibClient::new_default(), &song_id).map_err(
        |error| {
            // Lower-level lyrics modules still return anyhow errors. Classify them
            // here so UI-facing commands expose stable error codes and fallback hints
            // before the internal modules are fully migrated to typed domain errors.
            lyrics_error(error.to_string())
        },
    )
}

#[tauri::command]
pub fn set_lyrics_offset(
    state: State<'_, AppState>,
    song_id: String,
    ms: i64,
) -> CommandResult<()> {
    let connection = cache::open_database(&state.database_path).map_err(database_error)?;

    set_lyrics_offset_in_connection(&connection, &song_id, ms)
        .map_err(|error| lyrics_error(error.to_string()))
}

pub fn fetch_lyrics_from_connection(
    connection: &Connection,
    client: &LrcLibClient,
    song_id: &str,
) -> Result<LyricsPayload> {
    let song = cache::get_song_by_hash(connection, song_id)
        .context("failed to load song from library")?
        .with_context(|| format!("song with hash {song_id} was not found in the library"))?;

    // Lyrics are cached by the stable song hash so repeat fetches can skip both
    // network and filesystem fallbacks once a synced source has been resolved.
    if let Some(cached) = cache::lyrics::get_lyrics_cache_entry(connection, song_id)? {
        return payload_from_cached_entry(song.hash, cached);
    }

    let Some(fetched) = lyrics::fetch::fetch_lyrics_for_song(client, &song)? else {
        return Ok(LyricsPayload {
            song_id: song.hash,
            lines: Vec::new(),
            source: None,
            offset_ms: 0,
        });
    };

    let lines = lyrics::parser::parse_lrc(&fetched.raw_lrc)
        .with_context(|| format!("failed to parse synced lyrics for song {song_id}"))?;
    let source = fetched.source;
    cache::lyrics::upsert_lyrics_cache_entry(
        connection,
        &LyricsCacheEntry {
            song_hash: song.hash.clone(),
            lrc: fetched.raw_lrc,
            source: source.clone(),
            offset_ms: 0,
            fetched_at: current_unix_timestamp()?,
        },
    )
    .context("failed to cache fetched lyrics")?;

    Ok(LyricsPayload {
        song_id: song.hash,
        lines,
        source: Some(source),
        offset_ms: 0,
    })
}

pub fn set_lyrics_offset_in_connection(
    connection: &Connection,
    song_id: &str,
    ms: i64,
) -> Result<()> {
    let song_exists = cache::get_song_by_hash(connection, song_id)
        .context("failed to load song from library")?
        .is_some();
    if !song_exists {
        bail!("song with hash {song_id} was not found in the library");
    }

    if cache::lyrics::get_lyrics_cache_entry(connection, song_id)?.is_none() {
        bail!("song with hash {song_id} does not have cached lyrics");
    }

    cache::lyrics::set_lyrics_offset(connection, song_id, ms)
        .context("failed to persist lyrics offset")?;

    Ok(())
}

fn payload_from_cached_entry(song_id: String, cached: LyricsCacheEntry) -> Result<LyricsPayload> {
    let lines = lyrics::parser::parse_lrc(&cached.lrc)
        .with_context(|| format!("failed to parse cached synced lyrics for song {song_id}"))?;

    Ok(LyricsPayload {
        song_id,
        lines,
        source: Some(cached.source),
        offset_ms: cached.offset_ms,
    })
}

fn current_unix_timestamp() -> Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is set before Unix epoch")?;

    Ok(duration.as_secs() as i64)
}
