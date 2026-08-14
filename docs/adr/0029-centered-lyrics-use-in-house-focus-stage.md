# ADR 0029 — Centered lyrics use an in-house focus stage

Date: 2026-08-14
Status: accepted

## Context

OpenKara now plays Word-timed Lyrics from AMLL. ADR 0028 forbids AGPL
AMLL player packages and forbids a copy of that player. The first
player change kept the list stage. After singers used the result, the
centered mode still looked like a flat list with a hard wipe. That
does not match the AMLL karaoke cue: the current line is the focus,
neighbors recede, and sung text lights from left to right over text
that stays visible.

## Decision

Keep the ban on AGPL AMLL packages. Do not copy AMLL player source.
Read the published visual model and rewrite it in OpenKara types.
The word wipe uses a two-alpha mask and a pixel mask-position from
`-(width + fade)` to `0`. Inactive lines use equal alphas `0.2`.
The active line uses bright `1` and dark `0.4`. Inactive playing
lines scale to `0.97` and use distance blur. Centered alignment
keeps the current line in the optical center. The line container
owns the viewport type size. Roman sits under the main line at
half that size. Background vocals sit under roman at seven-tenths
and only while the line is active. Do not hide the base text. Do
not wipe from right to left. Do not size roman from the button
user-agent font.

## Consequences

- License review still rejects `@applemusic-like-lyrics` packages.
- Focus-stage motion lives in `LyricsLineRuntime`. The wipe lives in
  `KaraokeFillController` and `LyricLine`.
- A later change that copies AMLL player source is still out of
  license scope.
