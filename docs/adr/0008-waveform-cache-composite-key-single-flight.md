# ADR 0008 — Waveform is cached per (song_id, bucket_count), single-flight

Date: 2026-07-19
Status: accepted

## Context

Waveform peaks are needed by the seek bar and the library list. Computing
them requires decoding the full audio file, which is expensive. The same
song can be requested at different bucket counts (seek bar vs. list
thumbnail) and from multiple components simultaneously (open the seek bar
while the library list is rendering). Naive caching keyed only on
`song_id` returns the wrong resolution; naive dispatch launches N
duplicate full decodes for N concurrent callers.

## Decision

The waveform cache key is the composite `(song_id, bucket_count)`. The
service layer uses a single-flight map: the first caller for a given
composite key starts the decode, and concurrent callers for the same key
share the same `Receiver`. A `CompletionGuard` clears the entry when the
decode finishes or all receivers drop, so a cancelled computation does
not strand the key. A poisoned mutex on the entry is recovered (the
entry is cleared and the error propagates with a sanitized message),
never propagated to callers.

Waveform computation is refused for remote (streaming) songs without
decoding — the contract is local-only. Media-G (zip) local songs are
supported.

## Consequences

- Adding a new waveform consumer must reuse the existing service; do not
  decode audio for peaks in a new code path.
- A new bucket count produces a new cache entry; the cache is not
  downsampled or upsampled from an existing entry. This is intentional —
  resampling cached peaks introduces artifacts.
- Remote songs must not get a silent fallback that decodes the full
  stream; the service returns an error so the UI can show an empty
  waveform instead of stalling on a network decode.
- The single-flight map is in `state::playback`; the cache key type is
  the load-bearing seam, not the storage backend.
