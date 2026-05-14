# Active plan

> **Status:** Active · **Last updated:** 2026-05-13  
> **Supersedes:** [`../archive/plans/2026-05-13-v0.9-hardening-and-playlists-plan.md`](../archive/plans/2026-05-13-v0.9-hardening-and-playlists-plan.md) (previous cycle — H1–H8 + F1 completed).

## Release target

**Next ship milestone:** To be determined. Candidate areas below are drawn from the backlog and hardening priorities discovered during the v0.9 cycle.

## Scope (candidates for next slice)

This is a **skeleton** — expand with agreed streams before execution begins.

### Potential capability: Mic Input & Vocal Effects

- Microphone capture via audio input device
- Real-time reverb/echo vocal effects
- Mix mic with accompaniment for output

### Potential capability: Pitch & Key Shift

- Real-time pitch shifting of the accompaniment track
- Per-song or global key setting

### Potential capability: Session Recording

- Record vocal + mixdown to audio file
- Export recorded sessions

### Remaining hardening / debt

- `cargo deny` setup (scheduled weekly check)
- CycloneDX SBOM generation
- H2 WebDAV smoke on Windows/Linux (needs maintainer access)
- Windows DirectML validation in CI (needs GPU runner)

---

## How executors should use this plan

1. Pick one stream from the agreed scope.
2. Add its **Acceptance criteria**, **Work items**, and **Verification** section before writing code.
3. Keep contracts and code aligned in the same change.
4. When the stream is complete, mark it here and update [`../implementation-status.md`](../implementation-status.md).

---

## Descope & abort policy

Same as previous cycle: document gaps, narrow acceptance, defer, or abort per [`./README.md`](./README.md).

---

## Completion

When all agreed streams for this slice are done, archive this file under `docs/archive/plans/` with a dated filename and write the next skeleton.
