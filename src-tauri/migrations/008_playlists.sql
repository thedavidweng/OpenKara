-- Playlist management tables for saved playlists (F1).
-- Each playlist has a name and optionally sorted order for the sidebar.
-- playlist_songs joins songs to playlists with an optional singer assignment.

CREATE TABLE IF NOT EXISTS playlists (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS playlist_songs (
    playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    song_hash   TEXT NOT NULL REFERENCES songs(hash) ON DELETE CASCADE,
    added_at    INTEGER NOT NULL,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    singer      TEXT,
    PRIMARY KEY (playlist_id, song_hash)
);
