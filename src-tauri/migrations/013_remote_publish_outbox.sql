-- Durable publish change-set stored inside the library SQLite database.
-- MUST be written in the same library SQLite transaction as the song
-- mutation. Crash recovery rebuilds remote-state.db from unprojected rows.
-- Machine-local: cleared from generation candidates at freeze time and
-- deleted after successful control-DB projection.
CREATE TABLE IF NOT EXISTS remote_publish_outbox (
    operation_id TEXT PRIMARY KEY NOT NULL,
    song_ids_json TEXT NOT NULL,
    expected_generation INTEGER,
    source_db_digest TEXT,
    created_at_ms INTEGER NOT NULL,
    projected_at_ms INTEGER
);
