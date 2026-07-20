# ADR 0002 — PlaybackCoordinator serializes the control plane

Date: 2026-07-19
Status: accepted

## Context

`PlaybackController` is mutated from many threads: Tauri command handlers,
background decode/fetch workers, the realtime audio callback, and the
preload scheduler. Letting each of those mutate the controller directly
produced ordering races (latest-request-wins vs. FIFO), AirPlay epoch
drift, CDG seek-reset gaps, and output-thread startup races. The realtime
audio callback runs at CPAL priority and cannot take a mutex.

## Decision

All control-plane mutations of `PlaybackController` (pause / resume / seek /
set_volume / set_stem_volume / set_eq_enabled / set_eq_gains / load_stems /
install_track / fail_load / prepare_next / cancel_prepared_next /
attach_stems) go through a single `PlaybackCoordinator` thread. Background
decode/fetch threads produce immutable `ReadyTrack` payloads and send
`PlaybackCommand` messages to the coordinator instead of mutating the
controller. The coordinator guarantees FIFO ordering, latest-request-wins
guards, AirPlay epoch/generation bumps, CDG seek-reset, and output-thread
startup — all on its own thread. The realtime audio callback never takes
the coordinator lock; it only reads controller state under the controller's
own lock.

Public Tauri command names, arguments, response shapes, and event names are
unchanged — the coordinator is an internal architectural seam, not an IPC
contract change.

## Consequences

- No code path may mutate `PlaybackController` outside the coordinator
  thread except the realtime audio callback's read-only render path.
- New control-plane operations must be added as `PlaybackCommand` variants,
  not as direct controller mutations.
- The realtime audio callback must remain free of coordinator sends inside
  the render hot path; crossfade promotion and gapless swap are handled by
  the callback reading controller state, not by sending commands mid-render.
- The IPC contract in `docs/references/contracts/playback.md` is the
  external face; this ADR records the internal threading invariant.
