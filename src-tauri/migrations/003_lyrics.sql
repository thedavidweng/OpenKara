CREATE TABLE IF NOT EXISTS lyrics (
    song_hash   TEXT PRIMARY KEY REFERENCES songs(hash),
    lrc         TEXT NOT NULL,
    source      TEXT NOT NULL,
    offset_ms   INTEGER NOT NULL DEFAULT 0,
    fetched_at  INTEGER NOT NULL
);
