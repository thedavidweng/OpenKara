# Database Schema

This document is **auto-generated** from `src-tauri/migrations/*.sql` by `scripts/generate-db-schema.mjs`.
Do **not** edit it by hand. Regenerate after any migration change:

```bash
node scripts/generate-db-schema.mjs
```

Source migrations: `001_init.sql`, `002_stems.sql`, `003_lyrics.sql`, `004_portable_paths.sql`, `005_audio_source_kind.sql`, `005_individual_stem_paths.sql`, `006_stem_model_variant.sql`, `007_song_instrumental.sql`, `008_playlists.sql`, `009_singer_rotation.sql`, `010_fts5_songs.sql`, `011_waveforms.sql`, `012_artwork_derivatives.sql`, `013_remote_publish_outbox.sql`, `014_lyrics_word_timed_probe.sql`, `015_streaming_identity.sql`.

## `songs`

Created by `001_init.sql`.

| Column                 | Type      | Notes       |
| ---------------------- | --------- | ----------- |
| `hash`                 | `TEXT`    | Primary key |
| `file_path`            | `TEXT`    |             |
| `title`                | `TEXT`    |             |
| `artist`               | `TEXT`    |             |
| `album`                | `TEXT`    |             |
| `duration_ms`          | `INTEGER` |             |
| `cover_art`            | `BLOB`    |             |
| `imported_at`          | `INTEGER` | NOT NULL    |
| `audio_source_kind`    | `TEXT`    |             |
| `instrumental`         | `INTEGER` |             |
| `artwork_thumb_path`   | `TEXT`    |             |
| `artwork_preview_path` | `TEXT`    |             |

## `stems`

Created by `002_stems.sql`.

| Column          | Type      | Notes               |
| --------------- | --------- | ------------------- |
| `song_hash`     | `TEXT`    | FK → songs(hash)    |
| `vocals_path`   | `TEXT`    | NOT NULL            |
| `accomp_path`   | `TEXT`    | NOT NULL            |
| `separated_at`  | `INTEGER` | NOT NULL            |
| `drums_path`    | `TEXT`    |                     |
| `bass_path`     | `TEXT`    |                     |
| `other_path`    | `TEXT`    |                     |
| `model_variant` | `TEXT`    | default 'htdemucs'; |

## `lyrics`

Created by `003_lyrics.sql`.

| Column                  | Type      | Notes               |
| ----------------------- | --------- | ------------------- |
| `song_hash`             | `TEXT`    | FK → songs(hash)    |
| `lrc`                   | `TEXT`    | NOT NULL            |
| `source`                | `TEXT`    | NOT NULL            |
| `offset_ms`             | `INTEGER` | NOT NULL, default 0 |
| `fetched_at`            | `INTEGER` | NOT NULL            |
| `word_timed_checked_at` | `INTEGER` |                     |

## `library_meta`

Created by `004_portable_paths.sql`.

| Column  | Type   | Notes       |
| ------- | ------ | ----------- |
| `key`   | `TEXT` | Primary key |
| `value` | `TEXT` | NOT NULL    |

## `playlists`

Created by `008_playlists.sql`.

| Column       | Type      | Notes               |
| ------------ | --------- | ------------------- |
| `id`         | `TEXT`    | Primary key         |
| `name`       | `TEXT`    | NOT NULL            |
| `created_at` | `INTEGER` | NOT NULL            |
| `updated_at` | `INTEGER` | NOT NULL            |
| `sort_order` | `INTEGER` | NOT NULL, default 0 |

## `playlist_songs`

Created by `008_playlists.sql`.

| Column        | Type      | Notes               |
| ------------- | --------- | ------------------- |
| `playlist_id` | `TEXT`    | FK → playlists(id)  |
| `song_hash`   | `TEXT`    | FK → songs(hash)    |
| `added_at`    | `INTEGER` | NOT NULL            |
| `sort_order`  | `INTEGER` | NOT NULL, default 0 |
| `singer`      | `TEXT`    |                     |

## `rotation_state`

Created by `009_singer_rotation.sql`.

| Column          | Type      | Notes                           |
| --------------- | --------- | ------------------------------- |
| `id`            | `INTEGER` | Primary key                     |
| `singer_names`  | `TEXT`    | NOT NULL, default '[]'          |
| `current_index` | `INTEGER` | NOT NULL, default 0             |
| `mode`          | `TEXT`    | NOT NULL, default 'round_robin' |
| `active`        | `INTEGER` | NOT NULL, default 0             |

## `songs_fts`

Created by `010_fts5_songs.sql`.

| Column      | Type   | Notes |
| ----------- | ------ | ----- |
| `title`     | `TEXT` | FTS5  |
| `artist`    | `TEXT` | FTS5  |
| `album`     | `TEXT` | FTS5  |
| `file_path` | `TEXT` | FTS5  |

## `waveforms`

Created by `011_waveforms.sql`.

| Column      | Type      | Notes    |
| ----------- | --------- | -------- |
| `song_hash` | `TEXT`    | NOT NULL |
| `buckets`   | `INTEGER` | NOT NULL |
| `peaks`     | `BLOB`    | NOT NULL |

## `remote_publish_outbox`

Created by `013_remote_publish_outbox.sql`.

| Column                | Type      | Notes               |
| --------------------- | --------- | ------------------- |
| `operation_id`        | `TEXT`    | Primary key         |
| `song_ids_json`       | `TEXT`    | NOT NULL            |
| `whole_repository`    | `INTEGER` | NOT NULL, default 0 |
| `expected_generation` | `INTEGER` |                     |
| `source_db_digest`    | `TEXT`    |                     |
| `created_at_ms`       | `INTEGER` | NOT NULL            |
| `projected_at_ms`     | `INTEGER` |                     |

## `streaming_track_identities`

Created by `015_streaming_identity.sql`.

| Column            | Type   | Notes            |
| ----------------- | ------ | ---------------- |
| `source`          | `TEXT` | NOT NULL         |
| `remote_track_id` | `TEXT` | NOT NULL         |
| `song_hash`       | `TEXT` | FK → songs(hash) |

## `playlist_origin_stamps`

Created by `015_streaming_identity.sql`.

| Column               | Type   | Notes              |
| -------------------- | ------ | ------------------ |
| `source`             | `TEXT` | NOT NULL           |
| `remote_playlist_id` | `TEXT` | NOT NULL           |
| `playlist_id`        | `TEXT` | FK → playlists(id) |

## Migration History

1. `001_init.sql` — CREATE TABLE IF NOT EXISTS songs (
2. `002_stems.sql` — CREATE TABLE IF NOT EXISTS stems (
3. `003_lyrics.sql` — CREATE TABLE IF NOT EXISTS lyrics (
4. `004_portable_paths.sql` — Library-level key-value metadata (e.g. schema version, migration markers).
5. `005_audio_source_kind.sql` — ALTER TABLE songs ADD COLUMN audio_source_kind TEXT NOT NULL DEFAULT 'original';
6. `005_individual_stem_paths.sql` — ALTER TABLE stems ADD COLUMN drums_path TEXT;
7. `006_stem_model_variant.sql` — ALTER TABLE stems ADD COLUMN model_variant TEXT DEFAULT 'htdemucs';
8. `007_song_instrumental.sql` — ALTER TABLE songs ADD COLUMN instrumental INTEGER NOT NULL DEFAULT 0;
9. `008_playlists.sql` — Playlist management tables for saved playlists (F1).
10. `009_singer_rotation.sql` — Singer rotation state for turn-based queue workflows (F1).
11. `010_fts5_songs.sql` — FTS5 virtual table for fast full-text search on song metadata.
12. `011_waveforms.sql` — CREATE TABLE IF NOT EXISTS waveforms (
13. `012_artwork_derivatives.sql` — Artwork derivative paths (thumbnail and preview WebP files).
14. `013_remote_publish_outbox.sql` — Durable publish change-set stored inside the library SQLite database.
15. `014_lyrics_word_timed_probe.sql` — ALTER TABLE lyrics ADD COLUMN word_timed_checked_at INTEGER;
16. `015_streaming_identity.sql` — CREATE TABLE IF NOT EXISTS streaming_track_identities (
