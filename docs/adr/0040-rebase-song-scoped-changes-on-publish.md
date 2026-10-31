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
- The Local Working Copy keeps one base database snapshot per remote generation and records the SHA-256 digest of the base it trusts. A rebase uses a base only when its bytes match that recorded digest; a missing, unrecorded, or altered base turns the publication into a whole-repository Pre-Publish Conflict and keeps the local changes. Older bases are pruned only after the new commit is recorded durably, so a crash at any point of the commit leaves a base the retry can verify.
- The rebase decides a song's scope by comparing the base and local snapshots, not by trusting outbox song ids, so an outbox entry for a song whose rows did not change never overrides the remote rows.
- A Keep Local resolution is stored on the pending operation, so a retry after a failed upload or an app restart keeps the keep-local policy.
- The singleton base snapshot written by 0.19.x is adopted once if its bytes match the recorded local database digest, then removed; an unverified legacy base is never used for a rebase.
- Local mutations must record accurate song ids or set the whole-repository flag in the outbox.
- A Pre-Publish Conflict must name the conflicting songs whenever song-scoped changes collide.
- Tests script failures through the in-memory storage port instead of production fault-injection code, which changes how tests exercise ADR 0018 restart recovery without changing recovery behavior.
