CREATE TABLE IF NOT EXISTS waveforms (
    song_hash TEXT NOT NULL,
    buckets   INTEGER NOT NULL CHECK (buckets BETWEEN 24 AND 1000),
    peaks     BLOB NOT NULL,
    PRIMARY KEY (song_hash, buckets),
    FOREIGN KEY (song_hash) REFERENCES songs(hash) ON DELETE CASCADE
);
