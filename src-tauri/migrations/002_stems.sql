CREATE TABLE IF NOT EXISTS stems (
    song_hash    TEXT PRIMARY KEY REFERENCES songs(hash),
    vocals_path  TEXT NOT NULL,
    accomp_path  TEXT NOT NULL,
    separated_at INTEGER NOT NULL
);
