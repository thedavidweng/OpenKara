# ADR 0004 — transport_generation is the monotonic transport identity

Date: 2026-07-19
Status: accepted

## Context

The frontend receives `playback-position` events at up to ~30 Hz and must
decide which event is authoritative. Position events are async, can be
delayed, dropped, or reordered by the IPC channel, and can come from
different transport states (playing, buffering, post-seek). Earlier
iterations used `songId` + position as the identity, which broke when a
delayed pre-seek event yanked the clock back after a seek/resume/pause, or
when a same-generation event for the previous song arrived after the
new-song snapshot during a gapless swap.

## Decision

`transport_generation: u64` is a monotonic counter that bumps on every
transport transition: new song load, resume, pause, seek, and gapless
swap. The frontend's position clock, stale-event filter, and authoritative
snapshot replacement all key off `transport_generation`. An event whose
generation is older than the clock's current generation is rejected before
its position is applied.

The `PlaybackStateSnapshot` returned by every transport command carries the
post-command generation, so the frontend can immediately anchor its clock
without waiting for the next `playback-position` event.

## Consequences

- Any new transport transition on the backend must bump
  `transport_generation` in the same coordinator step that mutates the
  controller, or the frontend will treat post-transition events as stale.
- The gapless swap in the realtime audio callback must bump the generation
  so the frontend's stale-event filter rejects delayed `playback-position`
  events from the previous song.
- Frontend code must never accept a position event whose generation is less
  than the clock's current generation; the monotonic `>` comparison is
  load-bearing, not a style choice.
- `transport_generation` is part of the IPC contract
  (`docs/references/contracts/playback.md`); this ADR records why it is
  monotonic and why the comparison is strict.
