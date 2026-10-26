# ADR 0040 — Rebase song-scoped outbox changes on publish

Date: 2026-10-26
Status: accepted

## Context

ADR 0011 tracks the Remote Revision so Publish Changes never overwrites another device silently. Earlier `CONTEXT.md` wording stopped publication with a whole-repository Pre-Publish Conflict whenever the Remote Revision moved after a local edit. When two devices edit different songs, a whole-repository conflict forces the user to discard one device's edits. Each outbox row already records the changed song ids, a whole-repository flag, the expected generation, and the base database digest. This decision records the rebase rule from [#590](https://github.com/thedavidweng/OpenKara/issues/590).

## Decision

Keep ADR 0011 and ADR 0018. Supersede the `CONTEXT.md` wording that raised a whole-repository Pre-Publish Conflict on every post-edit Remote Revision change. The Local Working Copy keeps the base database snapshot until Publish Changes succeeds. When the conditional manifest replace fails because the Remote Revision moved onto a newer remote generation, Publish Changes downloads the newer remote database snapshot. For each song-scoped outbox change whose remote rows still match the base snapshot rows, Publish Changes applies the local song rows onto the newer remote snapshot and retries the conditional manifest replace. A Pre-Publish Conflict remains only for songs that both devices changed and for whole-repository changes, and it names the conflicting songs. Pre-Mutation Refresh does not change.

## Consequences

- Edits to different songs on two devices publish without a Pre-Publish Conflict and without silent overwrites.
- The writer must retain the base database snapshot for the active generation until Publish Changes commits.
- Local mutations must record accurate song ids or set the whole-repository flag in the outbox.
- A Pre-Publish Conflict must name the conflicting songs whenever song-scoped changes collide.
- Tests script failures through the in-memory storage port instead of production fault-injection code, which changes how tests exercise ADR 0018 restart recovery without changing recovery behavior.
