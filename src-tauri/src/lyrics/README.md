# lyrics

Lyrics backend: fixed acquisition chain (cache -> embedded tags -> TTML/LYS/LRC
sidecars -> LRCLIB -> LrcApi), TTML/LYS/LRC parsers, SQLite cache, and per-song
offset persistence. `fetch_lyrics_online` accepts `automatic_upgrade` or
`user_replace` intent.

React lyrics panel and sync loop live in the frontend (`src/`).
