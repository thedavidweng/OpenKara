# Architecture Decision Records

ADRs record durable decisions. The code cannot explain these decisions alone. Each record is a short markdown file. The file has a date. The file has a number in creation order (`NNNN-kebab-title.md`).

## When to add an ADR

Add a record when a decision is:

- **load-bearing** — changing it breaks correctness, portability, or a frozen contract.
- **non-obvious** — a reader cannot reconstruct the reasoning from the code alone. For example: why a specific `ort` feature flag is pinned, why a cursor is monotonic, why two resampler caches must not be shared.

Do not add an ADR for clear code. Do not add an ADR for release history. Release history lives in `CHANGELOG.md`. Do not add an ADR for frozen IPC interfaces. Frozen IPC interfaces live in `docs/references/contracts/`.

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

## Writing rules

Write new ADRs in ASD-STE100 Simplified English. See `AGENTS.md` for the rules. Use short sentences. Use active voice. Use one topic per sentence. Use one word for one meaning. Do not edit accepted ADRs in place. Supersede a record. Write a new record. Update the old `Status` line. Do not delete accepted records. Do not rewrite accepted records.

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
