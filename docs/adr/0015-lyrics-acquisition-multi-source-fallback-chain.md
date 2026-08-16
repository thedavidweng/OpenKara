# ADR 0015 — Lyrics acquisition uses a multi-source fallback chain

Date: 2026-07-28
Status: superseded by 0026

## Context

A song can supply timed lyrics from more than one place. Embedded tags,
sidecar files, and online databases each hold a subset of the catalog. No
single source is complete. Online Lyrics Sources add latency and can fail when the
network is slow or absent. The player must show lyrics without blocking
playback on a network timeout. The source that supplied the lyrics also
matters to the user, because lyrics from an Online Lyrics Source can be
wrong and the user must know where to override them.

## Decision

Lyrics acquisition follows a fixed fallback chain. The chain reads the
SQLite lyrics cache first. On a cache miss it tries local sources before
Online Lyrics Sources. The local order is embedded tags, then sidecar files. The
sidecar order is TTML, then LYS, then LRC. The Online Lyrics Source order is
LRCLIB, then LrcApi. The first source that returns valid timed lyrics wins. A
miss on every source writes a negative-cache entry with a time-to-live so a later
addition to LRCLIB or LrcApi can be found on the next fetch. The winning
source is recorded in the cache and returned to the frontend through the
`LyricsPayload` source field.

## Consequences

- New lyrics sources must slot into the fixed chain. They cannot bypass the
  cache or the local-before-online-lyrics order.
- Local sources must stay free of network calls so playback never waits on a
  network timeout for lyrics that exist on disk.
- A new sidecar format must declare its priority relative to TTML, LYS, and
  LRC. The `read_sidecar_lyrics` priority function is the single place that
  sets this order.
- The negative-cache time-to-live is the only mechanism that lets a later
  online addition become visible. A source that removes this TTL creates
  stale empty-lyrics results.
- The `LyricsSource` enum is the contract for the winning source. A new
  source adds a variant here and in the IPC contract, not in ad hoc strings.
