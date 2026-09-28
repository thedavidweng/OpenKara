# NetEase web sign-in and local import design

Date: 2026-09-27
Status: proposed

## User outcome

A user opens a NetEase page from OpenKara, signs in on that page, browses the
user's music, and imports a permitted full song into the local OpenKara
library. OpenKara does not ask for a password or SMS code. The user can use
the same saved session after an app restart. A refused song does not enter the
library and has a clear refusal reason.

## Scope and decisions

- Do not use the NetEase developer platform or its app credentials.
- Use a dedicated incognito Tauri window with the official `music.163.com`
  login page. The page is the top-level document. It is not an iframe in
  OpenKara. OpenKara shows a Check sign-in action beside Connect NetEase.
- The Rust host reads the `MUSIC_U` and `__csrf` cookies from that window after
  the user completes sign-in. Rust verifies the session before it stores the
  minimum credentials in the existing system credential store. The login
  window has no Tauri capability or access to application commands. A user
  selects Check sign-in after completing the official page.
- Remove OpenKara's password and SMS inputs, SMS send command, and custom
  NetEase login requests. Keep one action in OpenKara to open the official
  page. Do not log or persist the phone number, code, password, or full browser
  cookie jar.
- Use a pinned release of `@neteasecloudmusicapienhanced/api` for NetEase
  account, playlist, search, track detail, and song URL requests. Run it in
  one app-owned Node sidecar. Rust and the sidecar exchange line-delimited
  JSON over standard input and output. Do not expose a local HTTP server.
  Bundle the Node runtime and package files for macOS, Windows, and Linux.
- Pass the saved session cookie to the sidecar for authenticated requests.
  The sidecar owns NetEase endpoint paths, request encryption, and provider
  request headers. OpenKara owns source types, error mapping, and import
  policy. Remove OpenKara's custom NetEase request encryption and endpoint
  definitions after the replacement passes acceptance.
- Use the upstream song URL operation to get a permitted full audio URL.
  Rust downloads the bytes into the existing temporary import path. The
  shared Streaming Import path owns hashing, conflicts, library files, and
  playlist membership. Preserve the current refusal for a trial clip, empty
  URL, or unavailable song. Do not add alternative music sources.

YesPlayMusic uses `/song/url` to play a track. It does not save song files in
its mainline player. OpenKara's existing Streaming Import path already saves
the obtained audio. The design reuses that path instead of copying the player.

## Data flow

1. The user selects Connect NetEase. Rust opens the official login page in a
   dedicated incognito Tauri window.
2. The user completes any verification on that page, then selects Check
   sign-in in OpenKara. Rust reads only the NetEase session cookies and asks
   the sidecar for the account state.
3. On success, Rust stores `MUSIC_U`, `__csrf`, and the display name through
   the existing credential store. On failure, it stores no new credentials
   and leaves the login window available for the user to retry.
4. The existing Streaming Source port calls the sidecar for browse and
   resolve operations. The sidecar returns provider JSON. Rust maps it to
   OpenKara types and downloads only an accepted full audio URL.
5. The existing shared import path adds the local file to the library.

## Failure and security boundaries

- A NetEase rejection, including code 8860, remains a provider rejection.
  OpenKara does not retry with a different identity or hide the response.
- If the sidecar cannot start, the NetEase source reports that integration
  error. It does not revert to the old custom protocol client.
- The official page may change or reject an embedded webview. Acceptance
  requires a real sign-in on all three desktop platforms before the old login
  paths are removed from a release.
- A session expires when the upstream account request rejects its cookie.
  The user can open the official page again. Sign-out deletes the stored
  credentials. The incognito window closes after a successful sign-in, so its
  browser data does not become a second persistent session.
- Package versions, checksums, and licenses are recorded in the lockfile and
  release evidence. The sidecar never prints credentials to standard output
  or diagnostic logs.

## Acceptance evidence

1. A real test account signs in through the official page. The account name
   appears in OpenKara. No OpenKara password or SMS input is present.
2. Restart preserves the session. Sign-out clears it. An expired session
   requests a new sign-in.
3. The account's liked tracks and playlists load. Search returns tracks.
4. A permitted full song downloads and imports through the shared path. A
   trial clip and an unavailable song produce refusals and no library file.
5. A clean macOS, Windows, and Linux install launches the bundled sidecar.
   The package does not require a user-installed Node runtime.
6. Tests cover the sidecar protocol, cookie capture and storage, provider
   error mapping, and the existing import result. Manual checks cover login
   page behavior, keyboard navigation, and completion on each platform.

The supplied account is for manual acceptance only. Its identifier and
verification material must not appear in source, fixtures, logs, or reports.

## Sources

- [YesPlayMusic track URL use](https://github.com/qier222/YesPlayMusic/blob/master/src/api/track.js)
- [Enhanced NetEase API](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced)
- [Enhanced song URL operation](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/main/module/song_url_v1.js)
- [Tauri external webview URL](https://docs.rs/tauri/2.11.5/tauri/enum.WebviewUrl.html)
- [Tauri cookie access](https://docs.rs/tauri/latest/x86_64-linux-android/tauri/webview/struct.Webview.html)
- [Tauri remote page capabilities](https://v2.tauri.app/security/capabilities/)
