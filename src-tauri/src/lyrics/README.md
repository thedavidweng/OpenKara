# lyrics

Lyrics backend: LRCLIB/LrcApi clients, TTML/LYS/LRC parsers, fetch priority
chain (LRCLIB -> LrcApi -> embedded tags -> sidecar), SQLite cache, per-song
offset persistence. Command payloads for `fetch_lyrics`, `fetch_lyrics_online`,
and `set_lyrics_offset`.

React lyrics panel and sync loop live in the frontend (`src/`).
