# ADR 0001 — ADR format and scope

Date: 2026-07-19
Status: accepted

## Context

OpenKara keeps contracts in `docs/references/contracts/` and release history
in `CHANGELOG.md`, but a third class of knowledge — durable, non-obvious
_why_ decisions that the code cannot self-explain — had no home. Comments
attached to the code get deleted when the surrounding code is refactored,
and `AGENTS.md` already warns against losing rationale comments that way.

## Decision

Introduce `docs/adr/` for Architecture Decision Records in the lightweight
MADR-derived format documented in `docs/adr/README.md`. Records are numbered,
dated, immutable once accepted, and superseded rather than rewritten.

ADRs are reserved for load-bearing, non-obvious decisions. Frozen IPC
interfaces stay in `docs/references/contracts/`; release-level changes stay
in `CHANGELOG.md`.

## Consequences

- Future non-obvious constraints surfaced during code review or comment
  cleanup must be recorded here instead of being re-inlined as comments.
- Accepted ADRs are not edited in place; supersession requires a new record.
- The README index is the entry point and must list every accepted record.
