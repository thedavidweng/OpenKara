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

**Online Source (在线来源)**:
A switchable music or video origin inside OpenKara. A **Streaming Source** or a **Video Source**.
_Avoid_: Provider, catalog, integration, remote provider, online lyrics source as a synonym

**Streaming Source (流媒体源)**:
An account-backed online music service. The user can browse it and import audio into the local library. NetEase Cloud Music is one. Later ones include Kugou and QQ Music.
_Avoid_: Catalog, provider, catalog import source, scrobbler, video source, UNM

**China Client Address (国内客户端地址)**:
The client address the NetEase **Streaming Source** sends as `X-Real-IP`. NetEase then treats the request as mainland traffic. It is not a second source and not UNM.
_Avoid_: UNM, unblock, real IP as a user identity, VPN, proxy server

**Streaming Credentials (流媒体凭据)**:
The session cookies that let OpenKara call a **Streaming Source** as the signed-in user. For NetEase they are `MUSIC_U` and `__csrf`. They are not the password and not **Repository Credentials**.
_Avoid_: Login, account, password, repository credentials, MUSIC_U as the user

**Streaming Playlist (流媒体歌单)**:
A track list a **Streaming Source** shows. OpenKara can display it. It is not a **Playlist**.
_Avoid_: Playlist, feed, remote playlist, catalog playlist, synced playlist

**Streaming Import (流媒体导入)**:
The action that turns selected tracks from a **Streaming Source** into library songs. OpenKara then owns those songs. The shared import path writes them. The **Streaming Source** does not.
_Avoid_: Sync, feed, publish, mirror

**Import Refusal (拒绝导入)**:
A track on a **Streaming Playlist** that OpenKara will not import. The user can still see title and artist. Typical causes are no play rights, a trial clip, or an empty stream URL.
_Avoid_: Grey song as the generic name, skip, unblock, UNM

**Playlist Origin Stamp (歌单来源印记)**:
The **Streaming Source** and that source's playlist id stored on a **Playlist**. It only matches a later **Streaming Import** of the same **Streaming Playlist**. It is not a live link.
_Avoid_: Binding, sync id, remote playlist id as the playlist itself

**Streaming Track Identity (流媒体曲目身份)**:
The **Streaming Source** plus that source's stable track id. It identifies one listing on that service. It is not the title, the artist, or the file hash.
_Avoid_: 原信息, metadata, title-artist match, hash as the streaming identity

**Library Decision (曲库抉择)**:
A blocking choice the user must make before a library action continues. The UI shows title, artist, album, format, bit rate, duration, and size. It does not show the file hash.
_Avoid_: Library manager, catalog manager, hash picker, CDG dialog as the general name

**Import Conflict (导入冲突)**:
A **Library Decision**. A **Streaming Import** matches an existing library song by **Streaming Track Identity**, but the files differ.
_Avoid_: Duplicate import, overwrite, sync conflict

**Keep Library Song**:
The **Import Conflict** choice. OpenKara leaves the existing song. It does not write the new file.
_Avoid_: Skip, ignore, cancel import

**Replace Library Song**:
The **Import Conflict** choice. OpenKara replaces the stored audio. Playlist membership and lyrics stay on that song. Existing stems become invalid.
_Avoid_: Upgrade, overwrite, upsert

**Apply to Remaining (将其余同样处理)**:
The **Library Decision** option. The user's Keep or Replace choice applies to every later **Import Conflict** in the same **Streaming Import**.
_Avoid_: Default replace, silent overwrite, apply to all libraries

**Reveal Song File (显示歌曲文件)**:
The action that shows the library song's audio file in the system file manager.
_Avoid_: Open original, 原曲, show hash path

**Reveal Stems (显示分轨)**:
The action that shows that song's stem folder in the system file manager.
_Avoid_: Open stems as files, open original, reveal song file

**Playlist**:
An OpenKara-managed ordered list of library songs.
_Avoid_: Streaming playlist, queue, feed

**Video Source (视频源)**:
An origin that resolves a link into videos the user can queue. It does not import audio. YouTube is one. This version uses the public watch page only. It does not sign in to Google.
_Avoid_: Streaming source, catalog, download source, YouTube provider, player URL

**Scrobbler**:
A listen-history service. It records what the user played. Last.fm and ListenBrainz are scrobblers. A scrobbler is not an **Online Source**.
_Avoid_: Streaming source, catalog, integration, Last.fm as the generic name

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

**Online Lyrics Source (在线歌词源)**:
A network lyrics lookup in **Lyrics Acquisition**. AMLL, LRCLIB, and LrcApi
are online lyrics sources. It is not an **Online Source**.
_Avoid_: Online Source, online source, catalog, scrobbler, provider

**Word-timed Lyrics (逐字歌词)**:
Lyrics whose timestamps are finer than a line. The timestamps may mark
words or syllables.
_Avoid_: Karaoke lyrics, synced lyrics, AMLL lyrics

**Line-timed Lyrics (逐行歌词)**:
Lyrics that timestamp only whole lines.
_Avoid_: LRC lyrics, simple lyrics, unsynced lyrics

**Word-timed Upgrade**:
The automatic replacement of **Line-timed Lyrics** from an **Online Lyrics
Source** with **Word-timed Lyrics**. It does not replace lyrics the user or
catalog owner put there.
_Avoid_: Auto fetch, refresh lyrics, re-download

**Supplied Romanization**:
Romanization that arrives with the lyrics. When it is present, it is the
romanization the player shows.
_Avoid_: Official romanization, TTML roman, x-roman

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
- **Lyrics Acquisition** may perform a **Word-timed Upgrade** when the
  cached winner is **Line-timed Lyrics** from an **Online Lyrics Source**. A
  Word-timed Upgrade does not replace manual, sidecar, or embedded lyrics.
  Unsynced **embedded** lyrics can still receive the older automatic timed
  upgrade.
- A **Word-timed Upgrade** only proceeds when the match is confident. An
  ambiguous match leaves the current **Line-timed Lyrics** in place.
- When lyrics include **Supplied Romanization**, the player shows that
  romanization. It does not generate a local romanization for those lines.
- An **Online Source** is either a **Streaming Source** or a **Video Source**.
- A **Streaming Source** is not a **Remote Provider**. It does not host a **Remote Repository**.
- A **China Client Address** belongs only to the NetEase **Streaming Source** adapter. It does not add engines, search other platforms, or change **Import Refusal**.
- **Streaming Credentials** live in the keychain under a streaming service name. They never share storage with **Repository Credentials**.
- NetEase sign-in offers QR, phone and password, or email and password, as YesPlayMusic does. OpenKara sends the password once. It stores only **Streaming Credentials**. It does not store the password or its hash.
- Turning a **Streaming Source** off hides it. It does not clear **Streaming Credentials**. Sign-out clears them.
- A **Video Source** does not import audio. It only supplies videos for the queue.
- YouTube playback loads the public watch page in a WebView. It does not call `/player` stream URLs. It does not store Google cookies.
- Age-restricted, private, or unlisted YouTube items fail visibly. They do not prompt for Google sign-in in this version.
- Adding another streaming brand adds another **Streaming Source**. It does not add a new import pipeline.
- A **Streaming Source** does not write library songs, **Playlists**, or a **Remote Repository**.
- A **Streaming Playlist** stays on the **Streaming Source** until the user performs a **Streaming Import**.
- After a **Streaming Import**, the new songs and any new **Playlist** belong to OpenKara. They do not stay bound to the **Streaming Playlist**.
- A **Playlist Origin Stamp** only finds the same **Playlist** on a later **Streaming Import**. It does not refresh that **Playlist** when the **Streaming Playlist** changes on its own.
- A **Streaming Track Identity** matches one listing. A live cut and a studio cut are different listings when the source gives them different ids.
- The file hash identifies one stored file. Two bitrates of the same **Streaming Track Identity** are the same library song, not two songs.
- Title and artist are display fields. They do not decide whether two imports are the same song.
- A **Library Decision** pauses the action until the user chooses. CDG pairing and **Import Conflict** are two cases of the same surface.
- Same file hash is not an **Import Conflict**. The import is already the same file.
- **Keep Library Song** and **Replace Library Song** are the only **Import Conflict** outcomes. OpenKara does not pick one by default.
- **Apply to Remaining** only covers later **Import Conflicts** in the current **Streaming Import**. It does not change songs the user already decided. It does not set a future default.
- An **Import Refusal** stays visible on the **Streaming Playlist**. It does not download. It does not become an **Import Conflict**. It goes on the failure list if the user selected it.
- **Reveal Song File** and **Reveal Stems** belong on a library song's context menu. They are disabled when the file or the stem folder is missing. They are hidden for a **Video Source** queue item.
- A **Scrobbler** records plays. It is not an **Online Source**, a **Streaming Source**, or a **Video Source**.
- An **Online Lyrics Source** belongs to **Lyrics Acquisition**. It is not an **Online Source**.

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
- People used "provider" for NetEase, Kugou, or YouTube. Resolved: **Remote Provider** stays storage-only. Streaming brands are **Streaming Sources**. YouTube is a **Video Source**.
- People used "catalog" for an importable music service. That reads as Last.fm or ListenBrainz. Resolved: use **Streaming Source**.
- People used "online source" for AMLL, LRCLIB, and LrcApi. That collides with **Online Source**. Resolved: those lookups are **Online Lyrics Sources**. Last.fm and ListenBrainz are **Scrobblers**.
- People used "feed" for showing a NetEase playlist in OpenKara. Resolved: the user browses a **Streaming Playlist**. A **Streaming Import** may create a **Playlist**. OpenKara owns the **Playlist**.
- People used file hash alone or title and artist alone to detect a repeat **Streaming Import**. Hash splits one listing across bitrates. Title and artist merge distinct versions. Resolved: match **Streaming Track Identity** and the file hash together. Same identity, different quality, is one song.
- People asked for a keep-or-replace window on repeat import. OpenKara has no such window today. The CDG picker is the existing blocking choice. Resolved: enlarge that surface into a **Library Decision**. An **Import Conflict** is one case. The user must **Keep Library Song** or **Replace Library Song**.
- People called the imported audio 原曲. That collides with accompaniment. Resolved: **Reveal Song File** and **Reveal Stems**. Disable the item when the target is missing. Do not hide it.
- A **Streaming Import** can hit many **Import Conflicts**. Resolved: ask for the current song, and offer **Apply to Remaining**, like a file-manager copy conflict.
- Grey songs and trial clips are **Import Refusals**. The user can see them. OpenKara does not import them and does not fetch a replacement from another service.
- Overseas NetEase access is not UNM. Resolved: the NetEase adapter always sends a **China Client Address**, as YesPlayMusic does. This version has no UNM engines and no Real-IP setting.
- People asked whether a password login must persist the password. Resolved: match YesPlayMusic. Offer QR, phone, and email. Persist only **Streaming Credentials**.
- YouTube guest playback must not use `/player` stream URLs. Kaset verified those as UNPLAYABLE when signed out. Resolved: play the watch page in a WebView. No Google login in this version.
