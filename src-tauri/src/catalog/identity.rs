use super::types::CatalogError;
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingTrackIdentity {
    pub source: String,
    pub remote_track_id: String,
}

pub fn lookup_song_hash(
    connection: &Connection,
    identity: &StreamingTrackIdentity,
) -> Result<Option<String>, CatalogError> {
    connection
        .query_row(
            "SELECT song_hash FROM streaming_track_identities \
             WHERE source = ?1 AND remote_track_id = ?2",
            params![identity.source, identity.remote_track_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| CatalogError::Internal(error.to_string()))
}

/// Every Streaming Track Identity that points at the library song.
pub fn identities_for_song(
    connection: &Connection,
    song_hash: &str,
) -> Result<Vec<StreamingTrackIdentity>, CatalogError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT source, remote_track_id FROM streaming_track_identities \
             WHERE song_hash = ?1 ORDER BY source, remote_track_id",
        )
        .map_err(|error| CatalogError::Internal(error.to_string()))?;
    let rows = statement
        .query_map([song_hash], |row| {
            Ok(StreamingTrackIdentity {
                source: row.get(0)?,
                remote_track_id: row.get(1)?,
            })
        })
        .map_err(|error| CatalogError::Internal(error.to_string()))?;
    rows.collect::<Result<_, _>>()
        .map_err(|error| CatalogError::Internal(error.to_string()))
}

/// Fills `library_song_id` from the persisted Streaming Track Identity, or
/// from the identity of the track's remembered Playable Match.
pub fn annotate_library_songs(
    connection: &Connection,
    source_id: &str,
    tracks: &mut [super::types::StreamingTrack],
) -> Result<(), CatalogError> {
    let mut direct = connection
        .prepare_cached(
            "SELECT song_hash FROM streaming_track_identities \
             WHERE source = ?1 AND remote_track_id = ?2",
        )
        .map_err(|error| CatalogError::Internal(error.to_string()))?;
    let mut matched = connection
        .prepare_cached(
            "SELECT i.song_hash FROM playable_matches m \
             JOIN streaming_track_identities i \
               ON i.source = m.match_source AND i.remote_track_id = m.match_remote_track_id \
             WHERE m.source = ?1 AND m.remote_track_id = ?2",
        )
        .map_err(|error| CatalogError::Internal(error.to_string()))?;
    for track in tracks {
        let args = params![source_id, track.remote_track_id];
        track.library_song_id = direct
            .query_row(args, |row| row.get(0))
            .optional()
            .map_err(|error| CatalogError::Internal(error.to_string()))?;
        if track.library_song_id.is_none() {
            track.library_song_id = matched
                .query_row(args, |row| row.get(0))
                .optional()
                .map_err(|error| CatalogError::Internal(error.to_string()))?;
        }
    }
    Ok(())
}

pub fn remember_playable_match(
    connection: &Connection,
    original: &StreamingTrackIdentity,
    matched: &StreamingTrackIdentity,
    matched_at_ms: i64,
) -> Result<(), CatalogError> {
    connection
        .execute(
            "INSERT INTO playable_matches \
             (source, remote_track_id, match_source, match_remote_track_id, matched_at_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(source, remote_track_id) DO UPDATE SET \
               match_source = excluded.match_source, \
               match_remote_track_id = excluded.match_remote_track_id, \
               matched_at_ms = excluded.matched_at_ms",
            params![
                original.source,
                original.remote_track_id,
                matched.source,
                matched.remote_track_id,
                matched_at_ms
            ],
        )
        .map_err(|error| CatalogError::Internal(error.to_string()))?;
    Ok(())
}

pub fn lookup_playable_match(
    connection: &Connection,
    original: &StreamingTrackIdentity,
) -> Result<Option<StreamingTrackIdentity>, CatalogError> {
    connection
        .query_row(
            "SELECT match_source, match_remote_track_id FROM playable_matches \
             WHERE source = ?1 AND remote_track_id = ?2",
            params![original.source, original.remote_track_id],
            |row| {
                Ok(StreamingTrackIdentity {
                    source: row.get(0)?,
                    remote_track_id: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(|error| CatalogError::Internal(error.to_string()))
}

pub fn stamp_identity(
    connection: &Connection,
    identity: &StreamingTrackIdentity,
    song_hash: &str,
) -> Result<(), CatalogError> {
    connection
        .execute(
            "INSERT INTO streaming_track_identities (source, remote_track_id, song_hash) \
             VALUES (?1, ?2, ?3) \
             ON CONFLICT(source, remote_track_id) DO UPDATE SET song_hash = excluded.song_hash",
            params![identity.source, identity.remote_track_id, song_hash],
        )
        .map_err(|error| CatalogError::Internal(error.to_string()))?;
    Ok(())
}

pub fn lookup_playlist_id(
    connection: &Connection,
    source: &str,
    remote_playlist_id: &str,
) -> Result<Option<String>, CatalogError> {
    connection
        .query_row(
            "SELECT playlist_id FROM playlist_origin_stamps \
             WHERE source = ?1 AND remote_playlist_id = ?2",
            params![source, remote_playlist_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| CatalogError::Internal(error.to_string()))
}

pub fn stamp_playlist_origin(
    connection: &Connection,
    source: &str,
    remote_playlist_id: &str,
    playlist_id: &str,
) -> Result<(), CatalogError> {
    connection
        .execute(
            "INSERT INTO playlist_origin_stamps (source, remote_playlist_id, playlist_id) \
             VALUES (?1, ?2, ?3) \
             ON CONFLICT(source, remote_playlist_id) DO UPDATE SET playlist_id = excluded.playlist_id",
            params![source, remote_playlist_id, playlist_id],
        )
        .map_err(|error| CatalogError::Internal(error.to_string()))?;
    Ok(())
}

pub fn find_song_hash_by_file_hash(
    connection: &Connection,
    file_hash: &str,
) -> Result<Option<String>, CatalogError> {
    connection
        .query_row(
            "SELECT hash FROM songs WHERE hash = ?1",
            params![file_hash],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| CatalogError::Internal(error.to_string()))
}
