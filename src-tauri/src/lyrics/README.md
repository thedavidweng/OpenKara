# lyrics

Lyrics backend: fixed acquisition chain (cache -> embedded tags -> TTML/LYS/LRC
sidecars -> AMLL -> LRCLIB -> LrcApi), TTML/LYS/LRC parsers, SQLite cache, and per-song
offset persistence. `fetch_lyrics_online` accepts `automatic_upgrade` or
`user_replace` intent. Cached Line-timed Lyrics from an Online Lyrics Source may later receive a
Word-timed Upgrade (AMLL only) gated by `word_timed_checked_at`.

React lyrics panel and sync loop live in the frontend (`src/`).
