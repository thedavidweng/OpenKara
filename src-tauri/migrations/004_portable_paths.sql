-- Library-level key-value metadata (e.g. schema version, migration markers).
CREATE TABLE IF NOT EXISTS library_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
