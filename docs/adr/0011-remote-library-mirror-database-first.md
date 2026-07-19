# ADR 0011 — Remote library mirror is database-first, files-lazy

Date: 2026-07-19
Status: accepted

## Context

A remote library (Dropbox, Google Drive, WebDAV) can contain thousands
of songs. Downloading every audio file on sync would saturate bandwidth
and disk. The library database, however, is small and is the index that
drives the UI: song list, search, sort, artwork. The UI needs to render
the full list immediately after sync, but audio only needs to be present
when the user plays a specific song.

## Decision

Remote sync is **database-first**: the SQLite library database is
mirrored to the remote provider on every mutation (import, edit, delete,
separation status), and pulled from the remote on open. Audio and
artwork files are **lazy**: they are fetched on demand when a song is
played or its artwork is rendered, and cached locally with an LRU
eviction policy under a budget.

The mirror is revision-tracked: each sync carries a revision token, and
a stale local revision against the remote produces a conflict error that
points the user to the settings recovery actions rather than silently
overwriting.

## Consequences

- The library list can render fully without any audio file being
  local. Code that assumes a song in the list has a local file is a
  regression — it must go through the cache/resolver path.
- The remote mutation pipeline is `prepare → mutate → sync → publish`.
  A sync failure returns without publishing, so the local state and the
  remote state never silently diverge.
- Adding a new file-bearing entity (e.g., lyrics sidecar) must decide
  whether it is mirrored eagerly (small, index-relevant) or fetched
  lazily (large, playback-relevant). The default for new text/index
  payloads is eager; the default for new media payloads is lazy.
- The conflict error message is part of the user-facing contract; it
  must point to the settings recovery actions, not a raw provider
  error.
