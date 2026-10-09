# ADR 0026 — Put AMLL first among Online Lyrics Sources

Date: 2026-08-13
Status: accepted

## Context

ADR 0015 fixed the lyrics source chain as cache, then local files, then
LRCLIB, then LrcApi. AMLL now supplies Word-timed Lyrics as TTML. The
AMLL catalog is smaller than LRCLIB. A miss must still fall through.
Local files and the SQLite cache must stay ahead of every network call.
A first paint from LRCLIB would flash Line-timed Lyrics on songs that
AMLL already has.

## Decision

Keep the ADR 0015 local order. Sidecar order stays TTML, then LYS, then
LRC. The first source that returns valid timed lyrics still wins. Insert
AMLL as the first Online Lyrics Source. The Online Lyrics Source order
is AMLL, then LRCLIB, then LrcApi. Use only the AMLL native search and
get endpoints. Do not use the AMLL LrcLib endpoints. AMLL wins only when
the match is confident and the parsed TTML has at least one word token.
The negative cache TTL stays 7 days. Unchanged from ADR 0015.

## Consequences

- A new Online Lyrics Source cannot skip the cache or the
  local-before-online-lyrics order.
- A new sidecar format must still declare its priority in
  read_sidecar_lyrics.
- Line-timed TTML from AMLL is a miss. The chain continues.
- The LyricsSource enum gains an amll variant. The IPC contract updates
  in the same change.
- ADR 0015 is superseded for chain order. Its local-before-online-lyrics rule,
  sidecar order, and negative-cache TTL still stand through this record.
