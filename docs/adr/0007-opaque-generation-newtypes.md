# ADR 0007 — Opaque generation newtypes prevent cross-space comparison

Date: 2026-07-19
Status: accepted

## Context

The playback system has multiple monotonic generation counters: a
`transport_generation` on the public snapshot (ADR 0004), a
`PreloadRequestGeneration` used internally to invalidate stale preload
results, an AirPlay epoch, and a refresh token for AirPlay control
debouncing. All are `u64`. Comparing one against another compiles
silently and produces stale-track bugs that are extremely hard to
diagnose — e.g., a stale `install_ready` whose `PreloadRequestGeneration`
happened to numerically match the current `transport_generation` would
install a track from a cancelled preload.

## Decision

Each generation counter is wrapped in a distinct newtype
(`PreloadRequestGeneration(u64)`, etc.) with no `PartialEq`/`PartialOrd`
across types. Comparisons only compile within the same newtype. The
public `transport_generation` on `PlaybackStateSnapshot` is the only one
exposed to the frontend; the internal newtypes stay internal.

## Consequences

- New generation counters must be wrapped in their own newtype, not
  reused as a bare `u64` or borrowed from another counter's type.
- Adding `From`/`Into` between generation newtypes is a regression; the
  type barrier is the point.
- Tests that need to construct a generation value must use the newtype
  constructor, not a bare integer — this is intentional friction.
