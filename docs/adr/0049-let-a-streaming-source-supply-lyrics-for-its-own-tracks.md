# ADR 0049 — Let a Streaming Source supply lyrics for its own tracks

Date: 2026-11-06
Status: accepted; amends 0026 and 0027

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
format to one of these two. A source that does not declare `lyrics` keeps
working, because the host checks only the protocol major.

The source's lookup is an Online Lyrics Source. It applies only to a song
that has a Streaming Track Identity from that source, and to an Online
Preview of that source. The host asks only that source. It does not ask
other Streaming Sources by title.

The Online Lyrics Source order becomes the identity's own Streaming Source,
then AMLL, then LRCLIB, then LrcApi. The cache and local sources stay ahead
of every network call, as ADR 0026 requires. Valid lyrics from the source
win. Line-timed Lyrics from the source stay eligible for a Word-timed
Upgrade like any other Online Lyrics Source result. A miss or an error
falls through to AMLL.

The source is unavailable when it is installed and declares `lyrics`, but
it is disabled or has no release that the host can run. An unavailable
source counts as a failed lookup. When the source failed, a miss on every
later source writes no negative-cache entry, so the next acquisition asks
the source again. A source that is not installed, or that does not declare
`lyrics`, is not part of the chain.

A Word-timed Upgrade asks the song's Streaming Source first, then AMLL. It
does not ask the source again when the source supplied the current lyrics.
Only Word-timed Lyrics from either one replace the cached lyrics. A failed
source lookup followed by an AMLL miss is an error, so the probe stamp
stays unset.

Lyrics Acquisition runs as it does today, when the song needs lyrics.
Streaming Import does not fetch lyrics.

## Consequences

- Imported songs and previews from a service get exact Word-timed Lyrics
  without a title and artist search.
- Songs that already hold Line-timed Lyrics from LRCLIB or LrcApi get the
  source's Word-timed Lyrics through the next Word-timed Upgrade.
- The host gains no parser. Malformed TTML or LRC from a source is a miss,
  handled the same way as malformed AMLL or LRCLIB lyrics. A `format` that
  does not match the text is a source error.
- `LyricsSource` gains a variant that records the source id. IPC and the
  SQLite cache serialize it as `streaming:<source_id>`. An older OpenKara
  that reads a Remote Library with this value does not know it.
- A song whose source was later removed falls back to the other Online
  Lyrics Sources. Lyrics already in the cache stay.
- The protocol reference and the fixture package document and exercise
  `lyrics.get`.
