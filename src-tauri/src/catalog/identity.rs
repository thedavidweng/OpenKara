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
