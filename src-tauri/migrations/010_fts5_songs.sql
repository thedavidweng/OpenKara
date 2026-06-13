-- FTS5 virtual table for fast full-text search on song metadata.
CREATE VIRTUAL TABLE IF NOT EXISTS songs_fts USING fts5(
    title,
    artist,
    album,
    file_path,
    content='songs',
    content_rowid='rowid'
);

-- Populate FTS index from existing data.
INSERT INTO songs_fts(rowid, title, artist, album, file_path)
SELECT rowid, title, artist, album, file_path FROM songs;

-- Triggers to keep FTS index in sync with the songs table.
CREATE TRIGGER IF NOT EXISTS songs_ai AFTER INSERT ON songs BEGIN
    INSERT INTO songs_fts(rowid, title, artist, album, file_path)
    VALUES (new.rowid, new.title, new.artist, new.album, new.file_path);
END;

CREATE TRIGGER IF NOT EXISTS songs_ad AFTER DELETE ON songs BEGIN
    INSERT INTO songs_fts(songs_fts, rowid, title, artist, album, file_path)
    VALUES ('delete', old.rowid, old.title, old.artist, old.album, old.file_path);
END;

CREATE TRIGGER IF NOT EXISTS songs_au AFTER UPDATE ON songs BEGIN
    INSERT INTO songs_fts(songs_fts, rowid, title, artist, album, file_path)
    VALUES ('delete', old.rowid, old.title, old.artist, old.album, old.file_path);
    INSERT INTO songs_fts(rowid, title, artist, album, file_path)
    VALUES (new.rowid, new.title, new.artist, new.album, new.file_path);
END;
