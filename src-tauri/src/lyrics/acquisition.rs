use super::{
    error::LyricsError,
    fetch::{
        fetch_lyrics_for_song_local, lookup_query_from_song, LyricsFetchResult, LyricsSource,
        OnlineLyricsResult, TimedLyricsProvider,
    },
    lrcapi::LrcApiClient,
    lrclib::{LrcLibClient, LyricsLookupQuery},
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
    lrclib: &'a LrcLibClient,
    lrcapi: &'a LrcApiClient,
}

impl<'a> LyricsAcquisition<'a> {
    pub fn new(lrclib: &'a LrcLibClient, lrcapi: &'a LrcApiClient) -> Self {
        Self { lrclib, lrcapi }
    }

    /// Run cache → embedded → TTML → LYS → LRC → LRCLIB → LrcApi.
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

        match self.fetch_online_query(&query)? {
            OnlineLyricsResult::Found(fetched) => Ok(LyricsAcquisitionResult::Fetched(fetched)),
            OnlineLyricsResult::DefiniteMissing => Ok(LyricsAcquisitionResult::Absent {
                cache_negative: true,
            }),
            OnlineLyricsResult::NotApplicable => Ok(LyricsAcquisitionResult::Absent {
                cache_negative: false,
            }),
            OnlineLyricsResult::Unavailable(error) => {
                Err(LyricsError::NetworkUnavailable(error.to_string()))
            }
        }
    }

    /// Fetch online lyrics for a song with an explicit overwrite intent.
    ///
    /// The intent controls persistence. Provider lookup has one fixed order.
    pub fn fetch_online(
        &self,
        connection: &Connection,
        song_id: &str,
        _intent: LyricsOnlineFetchIntent,
    ) -> Result<OnlineLyricsResult, LyricsError> {
        let song = cache::get_song_by_hash(connection, song_id)
            .map_err(|error| LyricsError::DatabaseUnavailable(error.to_string()))?
            .ok_or_else(|| LyricsError::SongNotFound(song_id.to_owned()))?;
        let Some(query) = lookup_query_from_song(&song) else {
            return Ok(OnlineLyricsResult::NotApplicable);
        };
        self.fetch_online_query(&query)
    }

    fn fetch_online_query(
        &self,
        query: &LyricsLookupQuery,
    ) -> Result<OnlineLyricsResult, LyricsError> {
        let providers = [
            TimedLyricsProvider::LrcLib(self.lrclib),
            TimedLyricsProvider::LrcApi(self.lrcapi),
        ];
        Ok(super::fetch::fetch_online_timed_lyrics(&providers, query))
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
        let current = cache::lyrics::get_lyrics_cache_entry(connection, song_id)
            .map_err(|error| LyricsError::DatabaseUnavailable(error.to_string()))?;
        if intent == LyricsOnlineFetchIntent::AutomaticUpgrade
            && current
                .as_ref()
                .is_some_and(|entry| !is_auto_upgradable_source(&entry.source))
        {
            return Ok(unchanged_option(current.as_ref()));
        }

        match result {
            OnlineLyricsResult::Found(fetched) => {
                persist_fetched(connection, song_id, &fetched)?;
                Ok(LyricsPersistenceResult {
                    fetched: Some(fetched),
                    changed: true,
                })
            }
            OnlineLyricsResult::DefiniteMissing => {
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
    let offset_ms = super::parser::parse_lrc_metadata(&fetched.raw_lrc)
        .offset_ms
        .unwrap_or(0);
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

fn is_auto_upgradable_source(source: &LyricsSource) -> bool {
    matches!(source, LyricsSource::Embedded | LyricsSource::Absent)
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
            },
        )
        .expect("manual lyrics should persist");

        let result = LyricsAcquisition::persist_acquisition(
            &connection,
            "song",
            &LyricsAcquisitionResult::Fetched(LyricsFetchResult {
                source: LyricsSource::LrcLib,
                raw_lrc: "[00:02.00] stale".to_owned(),
            }),
        )
        .expect("late result should resolve");

        assert!(!result.changed);
        assert_eq!(
            result.fetched.expect("current lyrics").source,
            LyricsSource::Manual
        );
    }
}
