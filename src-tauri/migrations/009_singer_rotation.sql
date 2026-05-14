-- Singer rotation state for turn-based queue workflows (F1).
-- Singers are stored as a JSON array of names. current_index tracks the
-- round-robin pointer. mode is 'round_robin' or 'single'. active indicates
-- whether rotation is enabled.

CREATE TABLE IF NOT EXISTS rotation_state (
    id              INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    singer_names    TEXT NOT NULL DEFAULT '[]',
    current_index   INTEGER NOT NULL DEFAULT 0,
    mode            TEXT NOT NULL DEFAULT 'round_robin',
    active          INTEGER NOT NULL DEFAULT 0
);
