# ADR 0027 — Upgrade Line-timed Lyrics from an Online Lyrics Source only on a confident AMLL match

Date: 2026-08-13
Status: accepted

## Context

Cache-first acquisition makes a Line-timed LRCLIB or LrcApi hit sticky.
Users would never see later Word-timed Lyrics. Replacing manual or
sidecar lyrics would discard intent. Replacing embedded lyrics as a
Word-timed Upgrade would also discard catalog-owner intent. A fuzzy
AMLL search can return several songs. A picker during playback is not
acceptable. Searching AMLL on every play would add latency and load.
Unsynced embedded lyrics already use a separate full-chain
automatic_upgrade. That path stays.

## Decision

After first paint, Lyrics Acquisition may perform a Word-timed Upgrade
when the cached winner is Line-timed Lyrics from lrc_lib, lrc_api, or
lrc_api_ttml. The upgrade path calls AMLL only. It replaces the cache
only when the match is confident and the TTML has word tokens. An
ambiguous or empty result leaves the current lyrics in place. A durable
word_timed_checked_at stamp with a 7 day TTL prevents a repeat search
after a successful miss. Network errors do not stamp the probe.
Word-timed Upgrade does not replace manual, sidecar, or embedded
lyrics. Unsynced embedded and absent keep the existing full-chain
automatic_upgrade.

## Consequences

- shouldAutoUpgrade must no longer skip lrc_lib.
- automatic_upgrade must not write an absent row over Line-timed Lyrics
  from an Online Lyrics Source.
- Offset resets when the winning source changes.
- Future Line-timed Online Lyrics Sources must declare whether they
  allow a Word-timed Upgrade.
- Unsynced lrc_lib uses the AMLL-only upgrade path. It does not use
  the embedded full-chain path.
