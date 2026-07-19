# Architecture Decision Records

ADRs capture durable decisions and non-obvious constraints that the code
cannot explain on its own. Each record is a short, dated markdown file
numbered in creation order (`NNNN-kebab-title.md`).

## When to add an ADR

Add a record when a decision is:

- **load-bearing** — changing it would break correctness, portability, or a
  frozen contract, and
- **non-obvious** — a reader of the code alone would not reconstruct the
  reasoning (e.g. why a specific `ort` feature flag is pinned, why a cursor
  is monotonic, why two resampler caches must not be shared).

Do not add an ADR for anything the code already says clearly, for release
history (that lives in `CHANGELOG.md`), or for frozen IPC interfaces (those
live in `docs/references/contracts/`).

## Format

```markdown
# ADR NNNN — <imperative title>

Date: YYYY-MM-DD
Status: accepted | superseded by NNNN | deprecated

## Context

<one paragraph: the problem and the forces that make it non-obvious>

## Decision

<one paragraph: what we chose>

## Consequences

<bullet list: what this forces on future code, and what it rules out>
```

Supersede a record by writing a new one and updating the old `Status` line —
do not delete or rewrite accepted records.

## Index

- [0001 — ADR format and scope](./0001-adr-format-and-scope.md)
- [0002 — PlaybackCoordinator serializes the control plane](./0002-playback-coordinator-serializes-control-plane.md)
- [0003 — IPC types are snake_case end-to-end](./0003-ipc-types-are-snake-case-end-to-end.md)
- [0004 — transport_generation is the monotonic transport identity](./0004-transport-generation-is-monotonic.md)
- [0005 — Realtime audio callback is lock-free](./0005-realtime-audio-callback-is-lock-free.md)
- [0006 — Multi-stem streaming uses all-or-nothing buffering](./0006-multi-stem-streaming-all-or-nothing-buffering.md)
- [0007 — Opaque generation newtypes prevent cross-space comparison](./0007-opaque-generation-newtypes.md)
- [0008 — Waveform cache is per (song_id, bucket_count), single-flight](./0008-waveform-cache-composite-key-single-flight.md)
- [0009 — Demucs stem order is vocals, drums, bass, other](./0009-demucs-stem-order.md)
- [0010 — Library sort is mixed-script, locale-aware, case-insensitive](./0010-library-sort-mixed-script.md)
- [0011 — Remote library mirror is database-first, files-lazy](./0011-remote-library-mirror-database-first.md)
- [0012 — CDG seek resets both timelines, transport_generation preserves renderer](./0012-cdg-seek-resets-both-timelines.md)
