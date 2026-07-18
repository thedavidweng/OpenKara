-- Artwork derivative paths (thumbnail and preview WebP files).
-- Applied via column_exists checks in cache::apply_migrations because
-- SQLite ALTER TABLE lacks IF NOT EXISTS and migrate_legacy_song_schema
-- recreates the songs table.
ALTER TABLE songs ADD COLUMN artwork_thumb_path TEXT;
ALTER TABLE songs ADD COLUMN artwork_preview_path TEXT;
