# ADR 0048 — Match a playable version on another source with user confirmation

Date: 2026-11-06
Status: accepted

## Context

Some services list tracks but give no full-length audio. Their Streaming
Source can browse, but every `media.resolve` call is an Import Refusal with
`no_media`. ADR 0035 says that a later feature to search a title on another
signed-in Streaming Source is a new product surface. A source package must not
do this itself. A package that searches other services hides the choice from
the user and stores the wrong Streaming Track Identity. The protocol also says
that the host does not fall back to another source.

## Decision

The host offers a Playable Match. It is an explicit user action on one track
whose source refuses with `no_media` (`empty_url` in the host). It is a host
feature. It is not a package fallback, and it does not change automatic
resolution. A source returns only its own media.

The host searches every other ready source that can search. It uses the query
`<title> <first artist>`, reads the first page, and ignores sources that fail.
One host module ranks the results. It drops refused tracks, tracks from the
original source, and tracks with a known duration more than 3000 ms from the
original. The exception is a track with the same ISRC. The tiers are: same
ISRC, exact title and artist, then similar title with overlapping artists.
A title with a version mark that the original lacks, or the reverse, ranks
below all other tiers. Version marks are live, remix, instrumental, karaoke,
and acoustic. The list holds at most 10 candidates.

The user sees the original and each candidate, with title, artist, album,
duration, and source, and confirms one. A same-ISRC candidate is preselected
but stays visible. The host then runs the normal Streaming Import on the
candidate source. The library song gets the candidate's Streaming Track
Identity. The original track is not an identity. A matched import gets no
Playlist Origin Stamp.

The host remembers a confirmed match for the original track. The next request
for the same original track imports the remembered candidate without a
dialog, if its source is ready and allowed to import. Otherwise the host
searches again. A listing of the original source shows the library song of the
remembered match as "In library".

## Consequences

- Packages stay honest. They never search other services and never return
  media from another service.
- Every match has a user choice. No code replaces a track silently.
- The `playable_matches` table stores the original identity, the matched
  identity, and the time of the choice. It does not store titles.
- The rules in the module `catalog/matching.rs` are the contract for ranking.
  A change to the tolerance or the version marks needs a new decision.
- This change covers one track. Matching a whole Streaming Playlist, with a
  list of unmatched tracks, is a follow-up.
- Video Sources such as YouTube are not candidates (ADR 0036).
