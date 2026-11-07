# ADR 0049 — Let a Streaming Source supply lyrics for its own tracks

Date: 2026-11-06
Status: proposed

## Context

Music services hold Word-timed Lyrics for most of their catalog. QQ Music
has QRC, Kugou has KRC, and NetEase has YRC. These lyrics are keyed by the
service's own track id. Lyrics Acquisition can only reach them through
AMLL, LRCLIB, and LrcApi, which search by title and artist. A search can
miss or pick the wrong version, and those catalogs are smaller. Songs that
come from a Streaming Import or an Online Preview already carry a
Streaming Track Identity, so the service can answer with an exact match.
Other music players with source plugins, such as MyGo Music, let a plugin
return lyrics for its own tracks.

ADR 0047 keeps music-service code out of OpenKara. The host must not parse
QRC, KRC, YRC, or any other service format.

## Decision

Add the `lyrics` capability in Streaming Source protocol 2.2. Its method is
`lyrics.get {track_id}`. It returns `{lyrics: null}` when the service has
none, or `{lyrics: {format, text}}`. `format` is `ttml` for Word-timed
Lyrics or `lrc` for Line-timed Lyrics. The source converts the service
format to one of these two. Translation and romanization travel inside the
TTML, which the host already parses. A source that does not declare
`lyrics` keeps working, because the host checks only the protocol major.

The source's lookup is an Online Lyrics Source. It applies only to a song
that has a Streaming Track Identity from that source, and only while the
source is installed, enabled, and ready. The host asks only that source.
It does not ask other Streaming Sources by title.

The Online Lyrics Source order becomes the identity's own Streaming Source,
then AMLL, then LRCLIB, then LrcApi. The cache and local sources stay ahead
of every network call, as ADR 0026 requires. Valid Word-timed Lyrics from
the source win. Line-timed Lyrics from the source also win, but they stay
eligible for a Word-timed Upgrade like any other Online Lyrics Source
result. A miss, an error, or an unavailable source falls through to AMLL.
When the source failed or was unavailable, a miss on every later source
writes no negative-cache entry, so the next acquisition asks the source
again.

Lyrics Acquisition runs as it does today, when the song needs lyrics.
Streaming Import does not fetch lyrics. An Online Preview uses the same
lookup with the preview's source and remote id.

## Consequences

- Imported songs and previews from a service get exact Word-timed Lyrics
  without a title and artist search.
- The host gains no parser. Malformed TTML or LRC from a source is a miss,
  handled the same way as malformed AMLL or LRCLIB lyrics.
- `LyricsSource` gains a `streaming` variant that records the source id. The
  IPC contract and the source label in the lyrics UI update in the same
  change.
- A song whose source was later removed falls back to the other Online
  Lyrics Sources. Lyrics already in the cache stay.
- The protocol reference, the host API page, and the fixture package
  document and exercise `lyrics.get` in the same change.
- ADR 0026 is amended for the Online Lyrics Source order only.
