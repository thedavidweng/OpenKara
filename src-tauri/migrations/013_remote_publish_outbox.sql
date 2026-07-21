-- Durable publish change-set stored inside the library SQLite database.
-- Written in the same transaction as the local song mutation so a crash
-- between the library commit and control-DB projection can rebuild the
-- remote-state.db operation from this outbox.
CREATE TABLE IF NOT EXISTS remote_publish_outbox (
    operation_id TEXT PRIMARY KEY NOT NULL,
    song_ids_json TEXT NOT NULL,
    expected_generation INTEGER,
    source_db_digest TEXT,
    created_at_ms INTEGER NOT NULL,
    projected_at_ms INTEGER
);
