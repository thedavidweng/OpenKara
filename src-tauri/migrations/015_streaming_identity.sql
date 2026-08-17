CREATE TABLE IF NOT EXISTS streaming_track_identities (
    source TEXT NOT NULL,
    remote_track_id TEXT NOT NULL,
    song_hash TEXT NOT NULL REFERENCES songs(hash) ON DELETE CASCADE,
    PRIMARY KEY (source, remote_track_id)
);

CREATE TABLE IF NOT EXISTS playlist_origin_stamps (
    source TEXT NOT NULL,
    remote_playlist_id TEXT NOT NULL,
    playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    PRIMARY KEY (source, remote_playlist_id)
);
