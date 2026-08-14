# ADR 0030 — Focus stage owns line slots, word roman, and long-note emphasis

Date: 2026-08-14
Status: accepted

## Context

ADR 0029 put the karaoke wipe and neighbor scale on a flex list. Singers still
saw a scrolling document. Harmony sat in the same row as pronunciation.
Long notes lost their emphasis when the wipe was rewritten. The payload
stored only a line-level roman string, so aligned word readings could not
reach the player.

## Decision

Centered timed lyrics use a focus stage. The runtime measures each line,
reserves a slot, and places the line with `top`. The existing scroll
viewport still moves the active slot to the optical center. Left-aligned
lyrics stay a document list.

`WordToken` carries optional aligned `roman`. The TTML parser writes that
field from a nested `x-roman` span or from sidecar parts that match the
word count. The line shows a word stack when those readings exist. It
shows a line sub-row only when they do not.

Background vocals stay on the same payload as `bg_words`. The view treats
them as an attached row. The row collapses when the line is not active.
A long note on the active line uses an in-house emphasize motion. The
motion is not a copy of the AMLL player.

## Consequences

- `docs/references/contracts/lyrics.md` documents `WordToken.roman`.
- A missing `roman` field deserializes as absent.
- Focus layout stays off when measured heights are zero so jsdom tests
  keep document flow.
- A later change that copies AMLL player source is still out of license
  scope.
