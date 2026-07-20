# ADR 0009 — Demucs stem order is vocals, drums, bass, other

Date: 2026-07-19
Status: accepted

## Context

The separator produces 4 stems from a Demucs model. The Demucs reference
implementation emits stems in a fixed channel order: vocals, drums,
bass, other. OpenKara's 2-stem mode (vocals + accompaniment) is built by
summing the three non-vocal stems, and the 4-stem mode exposes each stem
individually with a UI label. If the stem order were permuted, the
"vocals" slider would control the wrong stem and the karaoke experience
would break silently.

## Decision

The separator's stem output order is fixed as `[vocals, drums, bass,
other]`, matching the Demucs reference. The 2-stem accompaniment is
`drums + bass + other`. The 4-stem UI labels map positionally onto this
order. The stem order is a contract between the separator output and the
playback/UI consumers; it is not configurable.

## Consequences

- Swapping the order in the separator output without updating the
  2-stem summation and the 4-stem UI labels is a silent regression.
- A new model with a different stem order must either be remapped to
  this order at the separator boundary, or trigger a new ADR that
  supersedes this one and updates all consumers in the same change.
- Stem volume commands (`set_stem_volume`) index stems positionally;
  the index-to-name mapping lives in one place and is the single source
  of truth for the UI labels.
