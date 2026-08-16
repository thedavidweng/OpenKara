# ADR 0032 — End Streaming Import at the shared import path

Date: 2026-08-16
Status: accepted

## Context

A Streaming Source can show liked tracks, user playlists, and search.
The user may import a whole Streaming Playlist or one track. The
library, Playlists, stems, lyrics, and Remote Repository already assume
a local song file and a content hash. If each brand writes SQLite, each
brand forks those rules. File hash alone splits one listing across bit
rates. Title and artist alone merge a live cut with a studio cut.

## Decision

A Streaming Source adapter signs in, browses, and returns either an
importable audio file plus metadata, or an Import Refusal. The shared
Streaming Import path calls the existing song import. OpenKara then
owns the song and any new Playlist. The adapter does not write library
rows, Playlists, or a Remote Repository. A Playlist Origin Stamp
remembers the source and the remote playlist id only so a later import
of the same Streaming Playlist can update that Playlist. A Streaming
Track Identity is the source plus the source's stable track id. Same
identity and different quality is one library song. Same identity and a
different file is an Import Conflict. The user must Keep Library Song
or Replace Library Song. The user may Apply to Remaining in that
import. An Import Refusal stays visible and does not download.

## Consequences

- NetEase, Kugou, and QQ share one Streaming Source port.
- A later import of the same Streaming Playlist adds missing tracks. It
  does not delete local songs when the remote list shrinks.
- Replace Library Song keeps playlist membership and lyrics. It
  invalidates stems.
- Same file hash is not an Import Conflict.
- Title and artist are display fields. They are not identity.
