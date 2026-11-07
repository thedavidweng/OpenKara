CREATE TABLE IF NOT EXISTS playable_matches (
    source TEXT NOT NULL,
    remote_track_id TEXT NOT NULL,
    match_source TEXT NOT NULL,
    match_remote_track_id TEXT NOT NULL,
    matched_at_ms INTEGER NOT NULL,
    PRIMARY KEY (source, remote_track_id)
);
