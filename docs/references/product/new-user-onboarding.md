# New User Onboarding

## Goal

A new user must install OpenKara, point it at a music library, import songs, and start playback. The user must do this without internal engineering docs.

## First-Run Flow

1. Launch the app.
2. Choose a language.
3. Pick one of the first-run library paths:
   - create a new local library
   - open an existing local library
   - connect a remote repository
4. If the user chooses a remote repository, guide the user through provider-specific setup.
5. See the active library open. Songs and metadata come from the selected source.
6. Start playback from the library.
7. Fetch or edit lyrics (optional).
8. Download a separation model when the user needs karaoke stems (optional).

## Expectations

- The first-run flow must not require a hosted account.
- Library setup must explain the directory or remote path in use.
- Import must work with common local audio formats.
- The app must stay usable before a model download finishes.
- Lyrics must degrade gracefully: cached, online, embedded, sidecar, or manual.
- Remote setup must distinguish Google Drive, Dropbox, and WebDAV provider needs.

## Remote Repository Expectations

- A user who connects by Google Drive must go through browser-based OAuth. OpenKara must bring the user back into the app. The user must not understand the Drive API model.
- A user who connects by WebDAV must enter the server URL, credentials, and target repository path. The user must do this without engineering docs.
- If remote setup fails, the UI must show the problem type: authentication, server reachability, or remote repository initialization.
- Google Drive, Dropbox, and WebDAV must all work as provider flows. Each must give provider-specific setup and recovery guidance.
- To refresh a remote repository, update the local working copy from the remote database. Do not publish local edits.
- To reauthorize a remote repository, renew provider access. If the repository location changed, OpenKara must ask before it replaces the saved location. OpenKara must reject empty locations that are not already OpenKara repositories.
- To disconnect a remote repository, remove only the local OpenKara registration and credentials. To delete one, remove the provider-hosted repository contents and the local working copy.
