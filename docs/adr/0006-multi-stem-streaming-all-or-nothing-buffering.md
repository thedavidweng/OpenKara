# ADR 0006 — Multi-stem streaming uses all-or-nothing buffering

Date: 2026-07-19
Status: accepted

## Context

When streaming 2- or 4-stem separated audio, each stem is decoded by an
independent producer thread into its own ring buffer. The realtime
output callback consumes all stems in lockstep to mix them. If one stem
falls behind (slow decode, network stall on a remote source), continuing
to advance the other stems produces frame drift between stems — the
vocals and accompaniment go out of sync audibly.

## Decision

The multi-stem streaming clock advances by the **minimum** number of
frames available across all stems. If any stem is below its low-water
mark, the entire stream enters buffering: the callback emits silence for
all stems until every stem has caught up past its low-water mark. No stem
advances while any sibling is starved.

## Consequences

- A single slow stem stalls all stems. This is intentional: brief
  silence is preferable to inter-stem drift, which is uncorrectable
  after the fact.
- The low-water mark must be tuned so that the buffering window is short
  enough to feel like a network hiccup, not a pause. The current default
  is in `streaming.rs` and is a tuning parameter, not a fixed constant.
- Adding a new stem count (e.g., 3-stem) must reuse the same
  min-across-stems clock; a per-stem clock is a regression.
- Remote (proxy) sources that downsample to reduce bandwidth still
  participate in the all-or-nothing contract; the proxy bitrate is a
  throughput optimization, not a relaxation of the sync invariant.
