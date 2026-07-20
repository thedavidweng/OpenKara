# ADR 0003 — IPC types are snake_case end-to-end

Date: 2026-07-19
Status: accepted

## Context

The Rust backend serializes IPC payloads via `serde` with default field
names (snake_case). The TypeScript frontend has to consume those payloads
at high frequency — the `playback-position` event fires up to ~30 Hz with a
full `PlaybackStateSnapshot`. An earlier review (C03) proposed splitting
the wire format (snake_case) from a public frontend contract (camelCase)
via generated mappers.

The C03 split was rejected because:

- The contract types are imported from ~94 sites across the frontend.
- A 30 Hz position event would run through the mapper on every fire.
- Mappers drift from the Rust structs; the drift is a runtime bug, not a
  compile error.

## Decision

The TypeScript IPC contract types in `src/types/ipc.ts` use snake_case
field names end-to-end, matching the Rust struct serialization. No wire-
format adapter is inserted between Rust and TypeScript. The contract types
are the single source of truth for payload shape on both sides.

## Consequences

- Frontend code reads IPC payloads with non-idiomatic snake_case field
  names. This is an accepted trade-off.
- Renaming a Rust struct field must be paired with a TypeScript contract
  type update in the same change; the AGENTS.md rule "Never change public
  IPC commands/payloads/events without updating the contract docs" covers
  this.
- Internal-only frontend state (Zustand stores, hooks) is free to use
  camelCase; the snake_case constraint applies only to the IPC boundary
  and the contract types in `src/types/ipc.ts`.
- New IPC commands must add snake_case fields to the contract types, not
  invent a parallel camelCase shape.
