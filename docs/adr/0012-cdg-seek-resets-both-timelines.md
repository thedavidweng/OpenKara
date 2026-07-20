# ADR 0012 — CDG seek resets both timelines, transport_generation preserves renderer

Date: 2026-07-19
Status: accepted

## Context

A CDG file has two timelines: the packet timeline (raw CDG commands at
300 packets/sec) and the rendered frame timeline (the 300x216 RGBA
frames produced from those packets). On seek, the renderer must restart
decoding from the packet nearest the new position, but the existing
renderer slot must not be torn down and rebuilt — rebuilding it on every
seek causes a visible flicker and loses the sub-pixel scroll state.

Separately, a `transport_generation` bump (ADR 0004) must not destroy the
CDG renderer, because the CDG renderer is tied to the song, not to the
transport state. A pause/resume bumps the generation but must not reset
the CDG frame.

## Decision

On seek, the CDG service marks **both** timelines (packet and frame) for
repositioning in one step: the packet cursor jumps to the nearest packet
and the frame timeline is flagged to re-derive from the new packet
cursor. The renderer slot itself is preserved across the seek.

On `transport_generation` bump (pause/resume/volume), the CDG renderer is
preserved unchanged — only a song change or an explicit invalidate
clears the renderer slot.

## Consequences

- A new seek path must reset both timelines atomically; resetting only
  one produces a frame/packet desync that surfaces as a flicker or a
  stale frame.
- Code that bumps `transport_generation` must not assume the CDG
  renderer is invalidated; only `invalidate_songs` and an explicit
  song change clear the slot.
- The CDG load path (loading, ready, error statuses) is separate from
  the transport state machine; a CDG error does not fail the audio
  playback and vice versa.
