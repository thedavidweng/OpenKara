use super::{
    amll::AmllClient,
    error::LyricsError,
    fetch::{
        accepts_as_timed, fetch_lyrics_for_song_local, has_word_tokens, lookup_query_from_song,
        offset_ms_for_raw, LyricsFetchResult, LyricsSource, OnlineLyricsResult,
        TimedLyricsProvider,
    },
    lrcapi::LrcApiClient,
    lrclib::{LrcLibClient, LyricsLookupQuery},
    ttml_parser,
};
use crate::{
    cache::{self, lyrics::LyricsCacheEntry},
    commands::error::current_unix_timestamp,
    library_root::LibraryRoot,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

const NEGATIVE_CACHE_TTL_SECS: i64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricsOnlineFetchIntent {
    AutomaticUpgrade,
    UserReplace,
}

#[derive(Debug)]
pub enum LyricsAcquisitionResult {
    Cached(LyricsCacheEntry),
    Fetched(LyricsFetchResult),
    NegativeCacheHit,
    Absent { cache_negative: bool },
}

pub struct LyricsPersistenceResult {
    pub fetched: Option<LyricsFetchResult>,
    pub changed: bool,
}

/// Owns the ordered lyrics source chain and the rules that decide whether a
/// remote result may change the durable cache.
pub struct LyricsAcquisition<'a> {
    amll: &'a AmllClient,
    lrclib: &'a LrcLibClient,
    lrcapi: &'a LrcApiClient,
}

impl<'a> LyricsAcquisition<'a> {
    pub fn new(amll: &'a AmllClient, lrclib: &'a LrcLibClient, lrcapi: &'a LrcApiClient) -> Self {
        Self {
            amll,
            lrclib,
            lrcapi,
        }
    }

    /// Run cache → embedded → TTML → LYS → LRC → AMLL → LRCLIB → LrcApi.
    ///
    /// This method does not write SQLite. Callers must pass its result to a
    /// `PublishChanges::apply` mutation before the result becomes durable.
    pub fn acquire(
        &self,
        connection: &Connection,
        library_root: &LibraryRoot,
        song_id: &str,
    ) -> Result<LyricsAcquisitionResult, LyricsError> {
        let song = cache::get_song_by_hash(connection, song_id)
            .map_err(|error| LyricsError::DatabaseUnavailable(error.to_string()))?
            .ok_or_else(|| LyricsError::SongNotFound(song_id.to_owned()))?;

        if let Some(cached) = cache::lyrics::get_lyrics_cache_entry(connection, song_id)
            .map_err(|error| LyricsError::DatabaseUnavailable(error.to_string()))?
        {
            if cached.source == LyricsSource::Absent {
                if !is_negative_cache_expired(&cached) {
                    return Ok(LyricsAcquisitionResult::NegativeCacheHit);
                }
            } else {
                return Ok(LyricsAcquisitionResult::Cached(cached));
            }
        }

        if song.is_media_g_zip() {
            return Ok(LyricsAcquisitionResult::Absent {
                cache_negative: false,
            });
        }

        if let Some(song_path) = song.file_path.as_deref() {
            let resolved_path = library_root.resolve(song_path);
            if let Some(local) = fetch_lyrics_for_song_local(&song, &resolved_path)
                .map_err(|error| LyricsError::Internal(error.to_string()))?
            {
                return Ok(LyricsAcquisitionResult::Fetched(local));
            }
        }

        let Some(query) = lookup_query_from_song(&song) else {
            return Ok(LyricsAcquisitionResult::Absent {
                cache_negative: false,
            });
        };

        match self.fetch_online_query(&query) {
            OnlineLyricsResult::Found(fetched) => Ok(LyricsAcquisitionResult::Fetched(fetched)),
            OnlineLyricsResult::DefiniteMissing => Ok(LyricsAcquisitionResult::Absent {
                cache_negative: true,
            }),
            OnlineLyricsResult::WordTimedProbeMiss | OnlineLyricsResult::NotApplicable => {
                Ok(LyricsAcquisitionResult::Absent {
                    cache_negative: false,
                })
            }
            OnlineLyricsResult::Unavailable(error) => {
                Err(LyricsError::NetworkUnavailable(error.to_string()))
            }
        }
    }

    /// Fetch lyrics from Online Lyrics Sources with an explicit overwrite intent.
    ///
    /// Word-timed Upgrade of Line-timed Lyrics from an Online Lyrics Source calls AMLL only.
    /// `user_replace` and unsynced embedded / absent use the full chain.
    pub fn fetch_online(
        &self,
        connection: &Connection,
        song_id: &str,
        intent: LyricsOnlineFetchIntent,
    ) -> Result<OnlineLyricsResult, LyricsError> {
        let song = cache::get_song_by_hash(connection, song_id)
            .map_err(|error| LyricsError::DatabaseUnavailable(error.to_string()))?
            .ok_or_else(|| LyricsError::SongNotFound(song_id.to_owned()))?;
        let Some(query) = lookup_query_from_song(&song) else {
            return Ok(OnlineLyricsResult::NotApplicable);
        };
        let current = cache::lyrics::get_lyrics_cache_entry(connection, song_id)
            .map_err(|error| LyricsError::DatabaseUnavailable(error.to_string()))?;

        if intent == LyricsOnlineFetchIntent::AutomaticUpgrade {
            if let Some(entry) = current.as_ref() {
                if is_online_line_timed_source(&entry.source) {
                    if is_word_timed_probe_fresh(entry) {
                        return Ok(OnlineLyricsResult::NotApplicable);
                    }
                    return Ok(self.fetch_amll_query(&query));
                }
                if !matches!(entry.source, LyricsSource::Embedded | LyricsSource::Absent) {
                    return Ok(OnlineLyricsResult::NotApplicable);
                }
            }
        }

        Ok(self.fetch_online_query(&query))
    }

    pub(crate) fn fetch_amll_query(&self, query: &LyricsLookupQuery) -> OnlineLyricsResult {
        match TimedLyricsProvider::Amll(self.amll).fetch_timed_lrc(query) {
            Ok(Some(raw)) if accepts_as_timed(&LyricsSource::Amll, &raw) => {
                OnlineLyricsResult::Found(LyricsFetchResult {
                    source: LyricsSource::Amll,
                    raw_lrc: raw,
                    word_timed_checked_at: None,
                })
            }
            Ok(Some(_)) | Ok(None) => OnlineLyricsResult::WordTimedProbeMiss,
            Err(error) => OnlineLyricsResult::Unavailable(error),
        }
    }

    fn fetch_online_query(&self, query: &LyricsLookupQuery) -> OnlineLyricsResult {
        let providers = [
            TimedLyricsProvider::Amll(self.amll),
            TimedLyricsProvider::LrcLib(self.lrclib),
            TimedLyricsProvider::LrcApi(self.lrcapi),
        ];
        super::fetch::fetch_online_timed_lyrics(&providers, query)
    }

    pub fn persist_acquisition(
        connection: &Connection,
        song_id: &str,
        result: &LyricsAcquisitionResult,
    ) -> Result<LyricsPersistenceResult, LyricsError> {
        let current = cache::lyrics::get_lyrics_cache_entry(connection, song_id)
            .map_err(|error| LyricsError::DatabaseUnavailable(error.to_string()))?;

        match result {
            LyricsAcquisitionResult::Cached(entry) => Ok(LyricsPersistenceResult {
                fetched: fetched_from_cache_entry(entry),
                changed: false,
            }),
            LyricsAcquisitionResult::Fetched(fetched) => {
                if let Some(current) = current
                    .as_ref()
                    .filter(|entry| entry.source != LyricsSource::Absent)
                {
                    return Ok(unchanged(current));
                }
                persist_fetched(connection, song_id, fetched)?;
                Ok(LyricsPersistenceResult {
                    fetched: Some(fetched.clone()),
                    changed: true,
                })
            }
            LyricsAcquisitionResult::NegativeCacheHit => Ok(LyricsPersistenceResult {
                fetched: None,
                changed: false,
            }),
            LyricsAcquisitionResult::Absent { cache_negative } => {
                if let Some(current) = current
                    .as_ref()
                    .filter(|entry| entry.source != LyricsSource::Absent)
                {
                    return Ok(unchanged(current));
                }
                if *cache_negative {
                    cache_negative_lookup(connection, song_id)?;
                }
                Ok(LyricsPersistenceResult {
                    fetched: None,
                    changed: *cache_negative,
                })
            }
        }
    }

    pub(crate) fn persist_online_result(
        connection: &Connection,
        song_id: &str,
        result: OnlineLyricsResult,
        intent: LyricsOnlineFetchIntent,
    ) -> Result<LyricsPersistenceResult, LyricsError> {
        // Re-read at persist time. The user may have saved manual lyrics
        // while the probe was in flight.
        let current = cache::lyrics::get_lyrics_cache_entry(connection, song_id)
            .map_err(|error| LyricsError::DatabaseUnavailable(error.to_string()))?;

        match result {
            OnlineLyricsResult::Found(fetched) => {
                if intent == LyricsOnlineFetchIntent::AutomaticUpgrade
                    && !may_automatic_upgrade_replace(
                        current.as_ref().map(|entry| &entry.source),
                        &fetched,
                    )
                {
                    return Ok(unchanged_option(current.as_ref()));
                }
                persist_fetched(connection, song_id, &fetched)?;
                Ok(LyricsPersistenceResult {
                    fetched: Some(fetched),
                    changed: true,
                })
            }
            OnlineLyricsResult::WordTimedProbeMiss => {
                if current
                    .as_ref()
                    .is_some_and(|entry| is_online_line_timed_source(&entry.source))
                {
                    return stamp_probe_keep_row(connection, song_id, current.as_ref());
                }
                Ok(unchanged_option(current.as_ref()))
            }
            OnlineLyricsResult::DefiniteMissing => {
                if intent == LyricsOnlineFetchIntent::AutomaticUpgrade
                    && current
                        .as_ref()
                        .is_some_and(|entry| is_online_line_timed_source(&entry.source))
                {
                    // Belt and suspenders: a mis-wired fetch_amll_query that
                    // still returns DefiniteMissing must not wipe lrc_lib.
                    return stamp_probe_keep_row(connection, song_id, current.as_ref());
                }
                if intent == LyricsOnlineFetchIntent::AutomaticUpgrade
                    && current.as_ref().is_some_and(|entry| {
                        !matches!(entry.source, LyricsSource::Embedded | LyricsSource::Absent)
                    })
                {
                    return Ok(unchanged_option(current.as_ref()));
                }
                cache_negative_lookup(connection, song_id)?;
                Ok(LyricsPersistenceResult {
                    fetched: None,
                    changed: true,
                })
            }
            OnlineLyricsResult::NotApplicable => Ok(unchanged_option(current.as_ref())),
            OnlineLyricsResult::Unavailable(error) => {
                Err(LyricsError::NetworkUnavailable(error.to_string()))
            }
        }
    }
}

fn persist_fetched(
    connection: &Connection,
    song_id: &str,
    fetched: &LyricsFetchResult,
) -> Result<(), LyricsError> {
    let offset_ms = offset_ms_for_raw(&fetched.raw_lrc);
    let fetched_at =
        current_unix_timestamp().map_err(|error| LyricsError::Internal(error.to_string()))?;
    cache::lyrics::upsert_lyrics_cache_entry(
        connection,
        &LyricsCacheEntry {
            song_hash: song_id.to_owned(),
            lrc: fetched.raw_lrc.clone(),
            source: fetched.source.clone(),
            offset_ms,
            fetched_at,
            word_timed_checked_at: fetched.word_timed_checked_at,
        },
    )
    .map_err(|error| LyricsError::DatabaseUnavailable(error.to_string()))
}

fn cache_negative_lookup(connection: &Connection, song_id: &str) -> Result<(), LyricsError> {
    let fetched_at =
        current_unix_timestamp().map_err(|error| LyricsError::Internal(error.to_string()))?;
    cache::lyrics::upsert_lyrics_cache_entry(
        connection,
        &LyricsCacheEntry {
            song_hash: song_id.to_owned(),
            lrc: String::new(),
            source: LyricsSource::Absent,
            offset_ms: 0,
            fetched_at,
            word_timed_checked_at: None,
        },
    )
    .map_err(|error| LyricsError::DatabaseUnavailable(error.to_string()))
}

fn fetched_from_cache_entry(entry: &LyricsCacheEntry) -> Option<LyricsFetchResult> {
    if entry.source == LyricsSource::Absent {
        None
    } else {
        Some(LyricsFetchResult {
            source: entry.source.clone(),
            raw_lrc: entry.lrc.clone(),
            word_timed_checked_at: entry.word_timed_checked_at,
        })
    }
}

fn unchanged(entry: &LyricsCacheEntry) -> LyricsPersistenceResult {
    LyricsPersistenceResult {
        fetched: fetched_from_cache_entry(entry),
        changed: false,
    }
}

fn unchanged_option(entry: Option<&LyricsCacheEntry>) -> LyricsPersistenceResult {
    LyricsPersistenceResult {
        fetched: entry.and_then(fetched_from_cache_entry),
        changed: false,
    }
}

fn is_online_line_timed_source(source: &LyricsSource) -> bool {
    matches!(
        source,
        LyricsSource::LrcLib | LyricsSource::LrcApi | LyricsSource::LrcApiTtml
    )
}

fn may_automatic_upgrade_replace(
    current: Option<&LyricsSource>,
    incoming: &LyricsFetchResult,
) -> bool {
    match current {
        None | Some(LyricsSource::Absent) | Some(LyricsSource::Embedded) => true,
        Some(src) if is_online_line_timed_source(src) => {
            incoming.source == LyricsSource::Amll
                && ttml_parser::parse_ttml(&incoming.raw_lrc)
                    .map(|lines| has_word_tokens(&lines))
                    .unwrap_or(false)
        }
        Some(_) => false,
    }
}

fn stamp_probe_keep_row(
    connection: &Connection,
    song_id: &str,
    current: Option<&LyricsCacheEntry>,
) -> Result<LyricsPersistenceResult, LyricsError> {
    let now = current_unix_timestamp().map_err(|e| LyricsError::Internal(e.to_string()))?;
    cache::lyrics::set_word_timed_checked_at(connection, song_id, now)
        .map_err(|e| LyricsError::DatabaseUnavailable(e.to_string()))?;
    tracing::debug!(song_id, word_timed_checked_at = now, "amll probe stamp");
    // changed = true so Publish Changes copies the stamp
    // (sync_song_lyrics_to_remote upserts the whole LyricsCacheEntry).
    Ok(LyricsPersistenceResult {
        fetched: current.and_then(fetched_from_cache_entry),
        changed: true,
    })
}

fn is_word_timed_probe_fresh(entry: &LyricsCacheEntry) -> bool {
    is_word_timed_probe_fresh_at(entry.word_timed_checked_at, current_unix_timestamp())
}

fn is_word_timed_probe_fresh_at(checked_at: Option<i64>, now: Result<i64, anyhow::Error>) -> bool {
    let Some(ts) = checked_at else {
        return false;
    };
    match now {
        Ok(now) => now.saturating_sub(ts) <= NEGATIVE_CACHE_TTL_SECS,
        // Fail closed, same posture as is_negative_cache_expired:
        // clock failure → skip network.
        Err(_) => true,
    }
}

fn is_negative_cache_expired(entry: &LyricsCacheEntry) -> bool {
    match current_unix_timestamp() {
        Ok(now) => now - entry.fetched_at > NEGATIVE_CACHE_TTL_SECS,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Song;

    fn connection() -> Connection {
        let connection = Connection::open_in_memory().expect("SQLite should open");
        cache::apply_migrations(&connection).expect("migrations should apply");
        cache::upsert_song(
            &connection,
            &Song {
                hash: "song".to_owned(),
                file_path: None,
                cdg_path: None,
                media_g_container: None,
                instrumental: false,
                language: None,
                audio_source_kind: "original".to_owned(),
                title: Some("Song".to_owned()),
                artist: Some("Artist".to_owned()),
                album: None,
                duration_ms: 0,
                cover_art: None,
                has_cover_art: false,
                artwork_thumb_path: None,
                imported_at: 0,
                original_ext: None,
            },
        )
        .expect("song should persist");
        connection
    }

    #[test]
    fn negative_cache_expires_after_seven_days() {
        let now = current_unix_timestamp().expect("clock");
        let entry = LyricsCacheEntry {
            song_hash: "song".to_owned(),
            lrc: String::new(),
            source: LyricsSource::Absent,
            offset_ms: 0,
            fetched_at: now - NEGATIVE_CACHE_TTL_SECS - 1,
            word_timed_checked_at: None,
        };
        assert!(is_negative_cache_expired(&entry));
    }

    #[test]
    fn network_failure_does_not_create_negative_cache() {
        let connection = connection();
        let result = LyricsAcquisition::persist_online_result(
            &connection,
            "song",
            OnlineLyricsResult::Unavailable(anyhow::anyhow!("offline")),
            LyricsOnlineFetchIntent::UserReplace,
        );

        assert!(matches!(result, Err(LyricsError::NetworkUnavailable(_))));
        assert!(cache::lyrics::get_lyrics_cache_entry(&connection, "song")
            .expect("cache lookup should succeed")
            .is_none());
    }

    #[test]
    fn only_definite_missing_creates_negative_cache() {
        let connection = connection();
        let result = LyricsAcquisition::persist_online_result(
            &connection,
            "song",
            OnlineLyricsResult::DefiniteMissing,
            LyricsOnlineFetchIntent::UserReplace,
        )
        .expect("missing result should persist");

        assert!(result.changed);
        assert!(matches!(
            cache::lyrics::get_lyrics_cache_entry(&connection, "song")
                .expect("cache lookup should succeed")
                .map(|entry| entry.source),
            Some(LyricsSource::Absent)
        ));
    }

    #[test]
    fn late_acquisition_does_not_replace_newer_manual_lyrics() {
        let connection = connection();
        cache::lyrics::upsert_lyrics_cache_entry(
            &connection,
            &LyricsCacheEntry {
                song_hash: "song".to_owned(),
                lrc: "[00:01.00] manual".to_owned(),
                source: LyricsSource::Manual,
                offset_ms: 0,
                fetched_at: current_unix_timestamp().expect("clock"),
                word_timed_checked_at: None,
            },
        )
        .expect("manual lyrics should persist");

        let result = LyricsAcquisition::persist_acquisition(
            &connection,
            "song",
            &LyricsAcquisitionResult::Fetched(LyricsFetchResult {
                source: LyricsSource::LrcLib,
                raw_lrc: "[00:02.00] stale".to_owned(),
                word_timed_checked_at: None,
            }),
        )
        .expect("late result should resolve");

        assert!(!result.changed);
        assert_eq!(
            result.fetched.expect("current lyrics").source,
            LyricsSource::Manual
        );
    }

    #[test]
    fn persist_fetched_copies_acquire_probe_stamp() {
        let connection = connection();
        let result = LyricsAcquisition::persist_acquisition(
            &connection,
            "song",
            &LyricsAcquisitionResult::Fetched(LyricsFetchResult {
                source: LyricsSource::LrcLib,
                raw_lrc: "[00:02.00] after amll miss\n".to_owned(),
                word_timed_checked_at: Some(1_700_000_000),
            }),
        )
        .expect("lrclib after amll miss should persist");

        assert!(result.changed);
        let cached = cache::lyrics::get_lyrics_cache_entry(&connection, "song")
            .expect("cache lookup")
            .expect("entry");
        assert_eq!(cached.source, LyricsSource::LrcLib);
        assert_eq!(cached.word_timed_checked_at, Some(1_700_000_000));
    }

    #[test]
    fn persist_fetched_stores_declared_ttml_offset() {
        let connection = connection();
        let raw = r#"<tt itunes:timingOffset="150" xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:02.000"><span begin="00:01.000" end="00:02.000">Hello</span></p></div></body></tt>"#;
        let result = LyricsAcquisition::persist_acquisition(
            &connection,
            "song",
            &LyricsAcquisitionResult::Fetched(LyricsFetchResult {
                source: LyricsSource::Amll,
                raw_lrc: raw.to_owned(),
                word_timed_checked_at: None,
            }),
        )
        .expect("ttml should persist");

        assert!(result.changed);
        let cached = cache::lyrics::get_lyrics_cache_entry(&connection, "song")
            .expect("cache lookup")
            .expect("entry");
        assert_eq!(cached.source, LyricsSource::Amll);
        assert_eq!(cached.offset_ms, 150);
    }

    const SEEDED_LRCLIB: &str = "[00:10.00] cached line\n";

    fn word_timed_ttml() -> &'static str {
        r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:02.000"><span begin="00:01.000" end="00:02.000">Hello</span></p></div></body></tt>"#
    }

    fn seed_lrclib(connection: &Connection, offset_ms: i64) {
        cache::lyrics::upsert_lyrics_cache_entry(
            connection,
            &LyricsCacheEntry {
                song_hash: "song".to_owned(),
                lrc: SEEDED_LRCLIB.to_owned(),
                source: LyricsSource::LrcLib,
                offset_ms,
                fetched_at: 1_000_000,
                word_timed_checked_at: None,
            },
        )
        .expect("seed lrc_lib");
    }

    fn assert_lrclib_not_wiped(connection: &Connection, expected_offset: i64) {
        let cached = cache::lyrics::get_lyrics_cache_entry(connection, "song")
            .expect("cache lookup")
            .expect("entry");
        assert_eq!(cached.source, LyricsSource::LrcLib);
        assert_ne!(cached.source, LyricsSource::Absent);
        assert_eq!(cached.lrc, SEEDED_LRCLIB);
        assert_eq!(cached.offset_ms, expected_offset);
        assert!(cached.word_timed_checked_at.is_some());
    }

    fn empty_search_body() -> &'static str {
        r#"{
            "status": 200,
            "data": {
                "items": [],
                "pagination": {
                    "page": 1,
                    "pageSize": 5,
                    "total": 0,
                    "totalPages": 0,
                    "hasMore": false
                }
            }
        }"#
    }

    fn ambiguous_search_body() -> &'static str {
        r#"{
            "status": 200,
            "data": {
                "items": [
                    {
                        "id": 1,
                        "filename": "yellow-a.ttml",
                        "musicNames": ["Song"],
                        "artistNames": ["Artist"],
                        "albumNames": []
                    },
                    {
                        "id": 2,
                        "filename": "yellow-b.ttml",
                        "musicNames": ["Song"],
                        "artistNames": ["Artist"],
                        "albumNames": []
                    }
                ],
                "pagination": {
                    "page": 1,
                    "pageSize": 5,
                    "total": 2,
                    "totalPages": 1,
                    "hasMore": false
                }
            }
        }"#
    }

    fn persist_amll_query(connection: &Connection, amll: &AmllClient) -> LyricsPersistenceResult {
        let song = cache::get_song_by_hash(connection, "song")
            .expect("song lookup")
            .expect("song");
        let query = lookup_query_from_song(&song).expect("query");
        let lrclib = LrcLibClient::new("http://127.0.0.1:9");
        let lrcapi = LrcApiClient::new("http://127.0.0.1:9");
        let acquisition = LyricsAcquisition::new(amll, &lrclib, &lrcapi);
        let fetched = acquisition.fetch_amll_query(&query);
        assert!(
            matches!(fetched, OnlineLyricsResult::WordTimedProbeMiss),
            "fetch_amll_query must return WordTimedProbeMiss, not DefiniteMissing: {fetched:?}"
        );
        LyricsAcquisition::persist_online_result(
            connection,
            "song",
            fetched,
            LyricsOnlineFetchIntent::AutomaticUpgrade,
        )
        .expect("persist should keep the row")
    }

    #[test]
    fn fetch_amll_query_404_then_persist_does_not_wipe_lrclib() {
        let connection = connection();
        seed_lrclib(&connection, 250);

        let mut server = mockito::Server::new();
        let search = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::Any)
            .with_status(404)
            .create();

        let persisted = persist_amll_query(&connection, &AmllClient::new(server.url()));
        assert!(persisted.changed);
        assert_eq!(
            persisted.fetched.as_ref().map(|fetched| &fetched.source),
            Some(&LyricsSource::LrcLib)
        );
        assert_lrclib_not_wiped(&connection, 250);
        search.assert();
    }

    #[test]
    fn fetch_amll_query_empty_search_then_persist_does_not_wipe_lrclib() {
        let connection = connection();
        seed_lrclib(&connection, 250);

        let mut server = mockito::Server::new();
        let search = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(empty_search_body())
            .create();

        let persisted = persist_amll_query(&connection, &AmllClient::new(server.url()));
        assert!(persisted.changed);
        assert_lrclib_not_wiped(&connection, 250);
        search.assert();
    }

    #[test]
    fn fetch_amll_query_ambiguous_search_then_persist_does_not_wipe_lrclib() {
        let connection = connection();
        seed_lrclib(&connection, 250);

        let mut server = mockito::Server::new();
        let search = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(ambiguous_search_body())
            .create();

        let persisted = persist_amll_query(&connection, &AmllClient::new(server.url()));
        assert!(persisted.changed);
        assert_lrclib_not_wiped(&connection, 250);
        search.assert();
    }

    #[test]
    fn word_timed_upgrade_429_does_not_stamp() {
        let connection = connection();
        seed_lrclib(&connection, 250);

        let mut server = mockito::Server::new();
        let search = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::Any)
            .with_status(429)
            .create();

        let lrclib = LrcLibClient::new("http://127.0.0.1:9");
        let lrcapi = LrcApiClient::new("http://127.0.0.1:9");
        let amll = AmllClient::new(server.url());
        let acquisition = LyricsAcquisition::new(&amll, &lrclib, &lrcapi);
        let fetched = acquisition
            .fetch_online(
                &connection,
                "song",
                LyricsOnlineFetchIntent::AutomaticUpgrade,
            )
            .expect("429 is Unavailable, not a miss");
        assert!(matches!(fetched, OnlineLyricsResult::Unavailable(_)));
        let persist = LyricsAcquisition::persist_online_result(
            &connection,
            "song",
            fetched,
            LyricsOnlineFetchIntent::AutomaticUpgrade,
        );
        assert!(matches!(persist, Err(LyricsError::NetworkUnavailable(_))));

        let cached = cache::lyrics::get_lyrics_cache_entry(&connection, "song")
            .expect("cache lookup")
            .expect("entry");
        assert_eq!(cached.source, LyricsSource::LrcLib);
        assert_ne!(cached.source, LyricsSource::Absent);
        assert_eq!(cached.lrc, SEEDED_LRCLIB);
        assert_eq!(cached.offset_ms, 250);
        assert_eq!(cached.word_timed_checked_at, None);
        search.assert();
    }

    #[test]
    fn fresh_word_timed_probe_makes_zero_http() {
        let connection = connection();
        let now = current_unix_timestamp().expect("clock");
        cache::lyrics::upsert_lyrics_cache_entry(
            &connection,
            &LyricsCacheEntry {
                song_hash: "song".to_owned(),
                lrc: SEEDED_LRCLIB.to_owned(),
                source: LyricsSource::LrcLib,
                offset_ms: 250,
                fetched_at: 1_000_000,
                word_timed_checked_at: Some(now),
            },
        )
        .expect("seed fresh probe");

        let mut server = mockito::Server::new();
        let search = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::Any)
            .expect(0)
            .create();

        let lrclib = LrcLibClient::new("http://127.0.0.1:9");
        let lrcapi = LrcApiClient::new("http://127.0.0.1:9");
        let amll = AmllClient::new(server.url());
        let acquisition = LyricsAcquisition::new(&amll, &lrclib, &lrcapi);
        let fetched = acquisition
            .fetch_online(
                &connection,
                "song",
                LyricsOnlineFetchIntent::AutomaticUpgrade,
            )
            .expect("fresh probe is NotApplicable");
        assert!(matches!(fetched, OnlineLyricsResult::NotApplicable));
        search.assert();
    }

    #[test]
    fn clock_failure_treats_word_timed_probe_as_fresh() {
        assert!(is_word_timed_probe_fresh_at(
            Some(1),
            Err(anyhow::anyhow!("clock"))
        ));
        assert!(!is_word_timed_probe_fresh_at(
            None,
            Err(anyhow::anyhow!("clock"))
        ));
        assert!(is_word_timed_probe_fresh_at(Some(1), Ok(1)));
        assert!(!is_word_timed_probe_fresh_at(
            Some(1),
            Ok(1 + NEGATIVE_CACHE_TTL_SECS + 1)
        ));
    }

    #[test]
    fn automatic_upgrade_replaces_lrclib_with_word_timed_amll() {
        let connection = connection();
        seed_lrclib(&connection, 250);
        let result = LyricsAcquisition::persist_online_result(
            &connection,
            "song",
            OnlineLyricsResult::Found(LyricsFetchResult {
                source: LyricsSource::Amll,
                raw_lrc: word_timed_ttml().to_owned(),
                word_timed_checked_at: None,
            }),
            LyricsOnlineFetchIntent::AutomaticUpgrade,
        )
        .expect("amll win should persist");

        assert!(result.changed);
        let cached = cache::lyrics::get_lyrics_cache_entry(&connection, "song")
            .expect("cache lookup")
            .expect("entry");
        assert_eq!(cached.source, LyricsSource::Amll);
        assert_eq!(cached.offset_ms, 0);
        assert_eq!(cached.word_timed_checked_at, None);
    }

    #[test]
    fn automatic_upgrade_does_not_replace_lrclib_with_another_lrclib() {
        let connection = connection();
        seed_lrclib(&connection, 250);
        let result = LyricsAcquisition::persist_online_result(
            &connection,
            "song",
            OnlineLyricsResult::Found(LyricsFetchResult {
                source: LyricsSource::LrcLib,
                raw_lrc: "[00:02.00] other\n".to_owned(),
                word_timed_checked_at: None,
            }),
            LyricsOnlineFetchIntent::AutomaticUpgrade,
        )
        .expect("non-amll should be ignored");

        assert!(!result.changed);
        let cached = cache::lyrics::get_lyrics_cache_entry(&connection, "song")
            .expect("cache lookup")
            .expect("entry");
        assert_eq!(cached.source, LyricsSource::LrcLib);
        assert_eq!(cached.lrc, SEEDED_LRCLIB);
        assert_eq!(cached.offset_ms, 250);
    }

    #[test]
    fn definite_missing_on_lrclib_stamps_probe_instead_of_absent() {
        let connection = connection();
        seed_lrclib(&connection, 250);
        let result = LyricsAcquisition::persist_online_result(
            &connection,
            "song",
            OnlineLyricsResult::DefiniteMissing,
            LyricsOnlineFetchIntent::AutomaticUpgrade,
        )
        .expect("mis-wired DefiniteMissing must not wipe");

        assert!(result.changed);
        assert_lrclib_not_wiped(&connection, 250);
    }

    #[test]
    fn word_timed_probe_miss_keeps_user_tuned_offset() {
        let connection = connection();
        seed_lrclib(&connection, 500);
        let result = LyricsAcquisition::persist_online_result(
            &connection,
            "song",
            OnlineLyricsResult::WordTimedProbeMiss,
            LyricsOnlineFetchIntent::AutomaticUpgrade,
        )
        .expect("stamp-only");

        assert!(result.changed);
        assert_eq!(
            result
                .fetched
                .as_ref()
                .map(|fetched| fetched.source.clone()),
            Some(LyricsSource::LrcLib)
        );
        assert_lrclib_not_wiped(&connection, 500);
    }

    #[test]
    fn fetch_online_skips_network_for_protected_sources() {
        let connection = connection();
        cache::lyrics::upsert_lyrics_cache_entry(
            &connection,
            &LyricsCacheEntry {
                song_hash: "song".to_owned(),
                lrc: "manual".to_owned(),
                source: LyricsSource::Manual,
                offset_ms: 0,
                fetched_at: 1,
                word_timed_checked_at: None,
            },
        )
        .expect("seed manual");

        let mut server = mockito::Server::new();
        let search = server
            .mock("GET", "/v1/lyrics/search")
            .match_query(mockito::Matcher::Any)
            .expect(0)
            .create();
        let lrclib = LrcLibClient::new("http://127.0.0.1:9");
        let lrcapi = LrcApiClient::new("http://127.0.0.1:9");
        let amll = AmllClient::new(server.url());
        let acquisition = LyricsAcquisition::new(&amll, &lrclib, &lrcapi);
        let fetched = acquisition
            .fetch_online(
                &connection,
                "song",
                LyricsOnlineFetchIntent::AutomaticUpgrade,
            )
            .expect("protected skip");
        assert!(matches!(fetched, OnlineLyricsResult::NotApplicable));
        search.assert();
    }
}
