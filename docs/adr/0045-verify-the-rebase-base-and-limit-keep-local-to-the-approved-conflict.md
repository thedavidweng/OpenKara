# ADR 0045 — Verify the rebase base and limit Keep Local to the approved conflict

Date: 2026-10-31
Status: accepted

## Context

[ADR 0040](./0040-rebase-song-scoped-changes-on-publish.md) rebases song-scoped changes when the Remote Revision moved. A rebase compares three databases: the base snapshot, the newer remote snapshot and the local candidate. A wrong base hides local changes or invents remote changes, and a rebase from a wrong base can silently overwrite one device's edits. Release 0.19.x kept one base file per Local Working Copy. That file did not record its generation, and it could contain unpublished edits. The digest that an outbox row records is the digest of the working database before the mutation, so it is not proof of the common ancestor. Outbox song ids can also name a song that did not change. A Keep Local choice must survive a failed upload and an app restart. But the remote can change again before the next attempt, and the user did not see those changes.

## Decision

Amend ADR 0040. The Local Working Copy keeps one base snapshot for each remote generation. The control database records the SHA-256 digest of the base it trusts. A rebase uses a base only when its bytes have that digest. When the base is missing, has no recorded digest or has different bytes, the publication stops with a whole-repository Pre-Publish Conflict, and the local changes stay. The app records a base digest only from a pulled snapshot, a committed candidate or a verified Use Remote candidate. It never uses the digest that an outbox row records. A rebase finds the songs that this device changed by comparing the base rows with the local rows, not by reading outbox song ids. Older bases are deleted only after the control database records the new commit. A Keep Local choice stores the Pre-Publish Conflict that the user approved. On a later attempt, local rows win only for the songs in that conflict. When another song conflicts, the attempt stops with a new conflict, clears the stored approval and asks again. The local database replaces the whole remote library only when the approved conflict was a whole-repository conflict. A 0.19.x base file is used once, and only when its bytes have the local database digest that the control database recorded for the base generation. It is deleted after the next recorded commit.

## Consequences

- A base that cannot be proven never rebases. This can cause more whole-repository conflicts after an upgrade or after data loss in the working copy, but no edit is lost silently.
- The control database stores the base digest (`local_base_sha256`). Every path that changes the base generation must record the digest of that generation's snapshot.
- A crash at any step of a commit leaves a base that the retry can verify.
- Outbox song ids decide which assets upload. They do not decide which rows a rebase applies.
- A stored Keep Local choice never covers a remote change that the user did not see. The user can be asked more than once about one publication.
- A whole-repository approval replaces the whole remote library. The user interface must say so before the user approves it.
- The rule in ADR 0040 that the base snapshot stays until Publish Changes succeeds now applies to each generation's base.
