# ADR 0028 — Do not depend on AGPL AMLL player packages

Date: 2026-08-13
Status: accepted

## Context

AMLL publishes Word-timed Lyrics over HTTPS as TTML. It also publishes
an Apple Music style player under AGPL-3.0 only
(@applemusic-like-lyrics/core, react, react-full, vue, lyric, ttml).
OpenKara is Apache-2.0. Bundling that player would force the combined
work to AGPL. The npm README says the project is personal and must not
be used directly in production. The player owns its own scroll engine
and would replace OpenKara's list stage, lyrics-engine, audience
paging, and AirPlay bridge. Copying the player source has the same
license effect as a package depend.

## Decision

Do not add @applemusic-like-lyrics packages. Do not relicense OpenKara
to AGPL. Do not copy AMLL player or parser source. Consume the native
HTTP API and the published TTML format only. Keep the OpenKara list
lyrics stage. Implement karaoke-relevant behavior in KaraokeFillController
and LyricLine.roman. Do not switch to Apple Music center-active layout
in this change.

## Consequences

- A later Apple Music layout is a new product surface. It needs its
  own ADR and the interaction profile.
- Word fade and Supplied Romanization land in the existing panel.
- License review must reject any PR that adds an AGPL AMLL npm
  dependency.
