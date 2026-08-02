# OpenKara Context

This glossary captures domain language for OpenKara. Product docs, contracts, and implementation discussions must use the same terms.

## Language

**Remote Repository (远程资料库)**:
A registered OpenKara library. A remote provider stores its database and media artifacts. The user opens it through a local working copy.
_Avoid_: Remote library, remote song library, cloud folder

**Remote Provider**:
A storage service. It hosts a **Remote Repository**. Examples are Google Drive, Dropbox, and WebDAV.
_Avoid_: Cloud account, backend

**Remote Repository Location**:
The provider-specific folder, path, or URL. A remote provider stores a **Remote Repository** there.
_Avoid_: Locator, root, cloud folder

**Local Working Copy**:
The local cached copy of a **Remote Repository**. OpenKara opens and edits it.
_Avoid_: Local mirror, cache

**Repository Credentials**:
The OAuth tokens or WebDAV username and password. They let OpenKara access a **Remote Repository**.
_Avoid_: Login, account

**Reauthorize Repository (重新授权)**:
The recovery action. It renews OpenKara's permission to access an existing **Remote Repository**. It does not change the repository location.
_Avoid_: Reconnect provider, update credentials, login again

**Relocate Repository**:
The confirmed recovery action. It replaces a **Remote Repository Location**. The user moved the same repository in the remote provider.
_Avoid_: Reauthorize, overwrite old repository, connect new repository

**Remote Revision**:
The provider revision marker. OpenKara uses it to detect whether the remote database changed outside the current local working copy.
_Avoid_: Version, timestamp

**Refresh Repository**:
The action. It updates a **Local Working Copy** from the current **Remote Repository** state.
_Avoid_: Sync, force resync

**Publish Changes**:
The action. It writes local database or media changes from a **Local Working Copy** to a **Remote Repository**.
_Avoid_: Sync, upload database

**Publish Song**:
The action. It puts one song and its required karaoke artifacts in a **Remote Repository**.
_Avoid_: Sync song

**Mirror Local Library**:
The one-time action. It initializes a **Remote Repository** from an existing local library.
_Avoid_: Sync local library

**Disconnect Repository**:
The action. It removes a repository from OpenKara on the current device. It does not delete the repository contents.
_Avoid_: Delete library, remove data

**Delete Repository**:
The destructive action. It deletes repository contents from their storage location. For a **Remote Repository**, it deletes the provider-hosted repository contents.
_Avoid_: Disconnect, remove registration

**Pre-Mutation Refresh**:
The automatic refresh. OpenKara performs it before a local edit. The remote revision is newer than the local working copy.
_Avoid_: Conflict merge, background sync

**Pre-Publish Conflict**:
A safety stop. It occurs when the remote revision changes after the local edit. This happens before OpenKara publishes the result.
_Avoid_: Sync failure, upload error

**Release Candidate**:
The exact signed artifact set that OpenKara proposes to publish. A Release
Candidate has one commit, version, target set, and byte identity.

**Release Evidence**:
The recorded proof that a Release Candidate passed its required installed-app
scenarios. Release Evidence names the candidate subject and its artifact
digests.

**Lyrics Acquisition**:
The action that finds, parses, and stores the best available lyrics for a
song. It follows the fixed source order and records the winning source.

## Relationships

- A **Remote Repository** belongs to exactly one **Remote Provider** account and one **Remote Repository Location**.
- A **Remote Repository** has one **Local Working Copy** on each device. The device opens it.
- A **Local Working Copy** records the last known **Remote Revision**. This prevents conflicts.
- **Repository Credentials** grant access to a **Remote Repository**. They are not the repository itself.
- **Reauthorize Repository** updates **Repository Credentials** for the same **Remote Repository**. It must not change the **Remote Repository Location**.
- **Relocate Repository** updates the registered **Remote Repository Location** after explicit confirmation. It does not delete or overwrite contents at the old location.
- After **Relocate Repository**, OpenKara keeps the existing **Local Working Copy** directory. It immediately performs **Refresh Repository** from the new location. It records the new **Remote Revision**.
- **Relocate Repository** only accepts a location. The location must already contain a valid OpenKara repository. An empty location belongs to new repository creation or mirroring. It does not belong to relocation.
- **Refresh Repository** reads from a **Remote Repository** into a **Local Working Copy**.
- **Publish Changes** writes from a **Local Working Copy** into a **Remote Repository**.
- **Mirror Local Library** creates initial **Remote Repository** contents from a local library.
- **Disconnect Repository** removes OpenKara's local registration and credentials. It leaves repository contents in place.
- **Delete Repository** removes repository contents and then disconnects the repository from OpenKara.
- A **Pre-Mutation Refresh** can proceed automatically. The system did not apply the user edit yet.
- A **Pre-Publish Conflict** stops publication. The remote database is newer than the finished local edit. If OpenKara publishes the edit, it could overwrite another device.

## Example dialogue

> **Dev:** "If sync fails, do we reconnect the remote repository?"
> **Domain expert:** "Only if access expired. If the remote revision changed, refresh the local working copy first. If credentials expired, reauthorize the repository."

## Flagged ambiguities

- People used "Remote library" to mean both the user's karaoke library and the provider-hosted database/media container. Resolved: use **Remote Repository (远程资料库)**. The database also lives remotely.
- People used "Sync" for both remote-to-local and local-to-remote. Resolved: use **Refresh Repository** for remote-to-local. Use **Publish Changes** for local-to-remote.
- People treated "Remove" and "delete" as similar Settings actions. Resolved: **Disconnect Repository** preserves repository contents. **Delete Repository** deletes them from storage.
- People treated "Reconnect provider" and "update credentials" as separate user recovery actions. Resolved: use **Reauthorize Repository (重新授权)** for both OAuth renewal and WebDAV credential renewal. The remote repository location must not change.
- "Overwrite the old one" during reauthorization means OpenKara replaces its registered remote location. It does not delete or write over data at the old remote location. Resolved: call this **Relocate Repository**. It requires explicit confirmation with a cancel path.
- People used remote revision conflicts as one broad failure class. Resolved: **Pre-Mutation Refresh** is automatic. **Pre-Publish Conflict** is a user-visible safety stop.
