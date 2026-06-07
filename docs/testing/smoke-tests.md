# OpenKara Manual E2E Smoke Test Checklist

> **Purpose**: Minimal end-to-end verification that critical user flows work on each
> supported platform. This is not a comprehensive test suite -- it covers the happy
> paths that must pass before any release or deploy.

## How to Use

1. Check off each step as you execute it.
2. Mark the test **PASS** or **FAIL** in the result column.
3. If a test fails, file an issue immediately and note the issue number in the comments.
4. Platform-specific tests are tagged: **[macOS]**, **[Windows]**, **[Linux]**.

---

## 1. First Launch and Onboarding

### SMOKE-001 -- First launch wizard completes

| Field             | Detail                                                                                                        |
| ----------------- | ------------------------------------------------------------------------------------------------------------- |
| **Feature**       | Onboarding                                                                                                    |
| **Preconditions** | Fresh install, no existing library or settings on disk.                                                       |
| **Steps**         | 1. Launch OpenKara for the first time.                                                                        |
|                   | 2. Verify the language selection screen appears.                                                              |
|                   | 3. Select **English** and confirm the UI switches to English.                                                 |
|                   | 4. On the library setup screen, select **Create new local library**.                                          |
|                   | 5. Choose a temporary directory (e.g. `/tmp/openkara-test`).                                                  |
|                   | 6. Wait for library creation to finish.                                                                       |
|                   | 7. On the stem mode screen, select **Two-stem** and click **Get Started**.                                    |
| **Expected**      | The main library view appears with an empty song list. Settings are persisted (relaunching skips onboarding). |

### SMOKE-002 -- Back navigation in onboarding

| Field             | Detail                                                                           |
| ----------------- | -------------------------------------------------------------------------------- |
| **Feature**       | Onboarding                                                                       |
| **Preconditions** | At the library setup step of the onboarding wizard.                              |
| **Steps**         | 1. Click the **Back** link.                                                      |
|                   | 2. Verify the language selection screen reappears.                               |
|                   | 3. Select a language again and proceed forward.                                  |
| **Expected**      | Navigation between onboarding steps works without crashing or duplicating state. |

---

## 2. Music File Import

### SMOKE-003 -- Import a single audio file via file picker

| Field             | Detail                                                                                          |
| ----------------- | ----------------------------------------------------------------------------------------------- |
| **Feature**       | Library / Import                                                                                |
| **Preconditions** | App is open with an empty library. A valid audio file (MP3 or FLAC) is available on disk.       |
| **Steps**         | 1. Click the **Import** button in the library toolbar.                                          |
|                   | 2. In the native file picker, select a single MP3 or FLAC file.                                 |
|                   | 3. Wait for import to complete.                                                                 |
| **Expected**      | The song appears in the library list with the correct title and artist extracted from metadata. |

### SMOKE-004 -- Import multiple files at once

| Field             | Detail                                                                                 |
| ----------------- | -------------------------------------------------------------------------------------- |
| **Feature**       | Library / Import                                                                       |
| **Preconditions** | App is open with an empty library. Three or more audio files are available.            |
| **Steps**         | 1. Click the **Import** button.                                                        |
|                   | 2. Select multiple files in the file picker (hold Cmd/Ctrl).                           |
|                   | 3. Confirm the selection.                                                              |
| **Expected**      | All selected songs appear in the library. No duplicates. Progress indicator completes. |

### SMOKE-005 -- Drag-and-drop import

| Field             | Detail                                                                         |
| ----------------- | ------------------------------------------------------------------------------ |
| **Feature**       | Library / Import                                                               |
| **Preconditions** | App is open. An audio file is accessible in Finder/File Explorer.              |
| **Steps**         | 1. Drag an audio file from the OS file manager onto the OpenKara library view. |
|                   | 2. Drop the file.                                                              |
| **Expected**      | The file is imported and appears in the library.                               |

---

## 3. Song Playback with Lyrics

### SMOKE-006 -- Play a song from the library

| Field             | Detail                                                                         |
| ----------------- | ------------------------------------------------------------------------------ |
| **Feature**       | Playback                                                                       |
| **Preconditions** | Library contains at least one imported song.                                   |
| **Steps**         | 1. Double-click a song in the library list.                                    |
|                   | 2. Verify playback starts (audio is audible).                                  |
|                   | 3. Verify the playback bar shows the song title, elapsed time, and a seek bar. |
|                   | 4. Click **Pause**. Verify audio stops.                                        |
|                   | 5. Click **Play**. Verify audio resumes.                                       |
| **Expected**      | Play/pause toggling works. Seek bar moves with playback.                       |

### SMOKE-007 -- Seek within a song

| Field             | Detail                                                                        |
| ----------------- | ----------------------------------------------------------------------------- |
| **Feature**       | Playback                                                                      |
| **Preconditions** | A song is currently playing.                                                  |
| **Steps**         | 1. Click on the seek bar at roughly the midpoint.                             |
|                   | 2. Verify playback jumps to the selected position.                            |
|                   | 3. Drag the seek bar to near the end.                                         |
| **Expected**      | Playback position updates immediately. Audio continues from the new position. |

### SMOKE-008 -- Lyrics display and sync

| Field             | Detail                                                                 |
| ----------------- | ---------------------------------------------------------------------- |
| **Feature**       | Lyrics                                                                 |
| **Preconditions** | A song with synced lyrics (from LRCLIB or embedded) is in the library. |
| **Steps**         | 1. Play the song.                                                      |
|                   | 2. Verify the lyrics panel shows the lyrics text.                      |
|                   | 3. Verify the current line is highlighted as playback progresses.      |
|                   | 4. Verify the lyrics auto-scroll to keep the current line visible.     |
| **Expected**      | Lyrics are displayed and highlighted in sync with audio.               |

### SMOKE-009 -- Lyrics offset adjustment

| Field             | Detail                                                                         |
| ----------------- | ------------------------------------------------------------------------------ |
| **Feature**       | Lyrics                                                                         |
| **Preconditions** | A song with synced lyrics is playing.                                          |
| **Steps**         | 1. Open the lyrics offset control (if visible in the lyrics panel).            |
|                   | 2. Adjust the offset by +500ms.                                                |
|                   | 3. Verify the highlight timing shifts accordingly.                             |
|                   | 4. Reset the offset to 0.                                                      |
| **Expected**      | Offset changes are applied in real time. Resetting returns to original timing. |

---

## 4. Vocal Separation (AI Processing)

### SMOKE-010 -- Trigger vocal separation on a song

| Field             | Detail                                                                                                                                                                                                  |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Feature**       | AI Vocal Separation (Demucs v4 / ONNX)                                                                                                                                                                  |
| **Preconditions** | A song is imported that has not yet been separated. The ONNX model is available (first run downloads it).                                                                                               |
| **Steps**         | 1. Select the song in the library.                                                                                                                                                                      |
|                   | 2. Trigger vocal separation from the context menu or song detail view.                                                                                                                                  |
|                   | 3. Wait for processing to complete.                                                                                                                                                                     |
| **Expected**      | A progress indicator is shown during processing. After completion, the song has separated stems available (vocals + accompaniment, or 4-stem depending on settings). Playback with vocal removal works. |

### SMOKE-011 -- Stem mode setting

| Field             | Detail                                                                       |
| ----------------- | ---------------------------------------------------------------------------- |
| **Feature**       | Settings / Stem Mode                                                         |
| **Preconditions** | App is open.                                                                 |
| **Steps**         | 1. Open **Settings**.                                                        |
|                   | 2. Navigate to the stem mode section.                                        |
|                   | 3. Switch between **Two-stem** and **Four-stem**.                            |
|                   | 4. Verify the setting is saved.                                              |
| **Expected**      | The selected stem mode is persisted. Restarting the app retains the setting. |

---

## 5. Playlist Management

### SMOKE-012 -- Create a playlist

| Field             | Detail                                                               |
| ----------------- | -------------------------------------------------------------------- |
| **Feature**       | Playlists                                                            |
| **Preconditions** | Library contains at least two songs.                                 |
| **Steps**         | 1. Create a new playlist (via sidebar or menu).                      |
|                   | 2. Enter a name (e.g. "Friday Night").                               |
|                   | 3. Confirm creation.                                                 |
| **Expected**      | The new playlist appears in the playlist list with the entered name. |

### SMOKE-013 -- Add songs to a playlist

| Field             | Detail                                               |
| ----------------- | ---------------------------------------------------- |
| **Feature**       | Playlists                                            |
| **Preconditions** | A playlist exists. Library has songs.                |
| **Steps**         | 1. Right-click a song in the library.                |
|                   | 2. Select **Add to playlist** from the context menu. |
|                   | 3. Choose the target playlist.                       |
|                   | 4. Open the playlist view.                           |
| **Expected**      | The song appears in the playlist.                    |

### SMOKE-014 -- Remove a song from a playlist

| Field             | Detail                                                            |
| ----------------- | ----------------------------------------------------------------- |
| **Feature**       | Playlists                                                         |
| **Preconditions** | A playlist contains at least one song.                            |
| **Steps**         | 1. Open the playlist.                                             |
|                   | 2. Right-click a song in the playlist.                            |
|                   | 3. Select **Remove from playlist**.                               |
| **Expected**      | The song is removed from the playlist. It remains in the library. |

### SMOKE-015 -- Delete a playlist

| Field             | Detail                                                          |
| ----------------- | --------------------------------------------------------------- |
| **Feature**       | Playlists                                                       |
| **Preconditions** | A playlist exists.                                              |
| **Steps**         | 1. Right-click the playlist in the sidebar.                     |
|                   | 2. Select **Delete playlist**.                                  |
|                   | 3. Confirm the deletion.                                        |
| **Expected**      | The playlist is removed. Songs in the library are not affected. |

### SMOKE-016 -- Rename a playlist

| Field             | Detail                                             |
| ----------------- | -------------------------------------------------- |
| **Feature**       | Playlists                                          |
| **Preconditions** | A playlist exists.                                 |
| **Steps**         | 1. Right-click the playlist and select **Rename**. |
|                   | 2. Enter a new name.                               |
|                   | 3. Confirm.                                        |
| **Expected**      | The playlist name updates throughout the UI.       |

---

## 6. Queue and Singer Rotation

### SMOKE-017 -- Add songs to the playback queue

| Field             | Detail                                                       |
| ----------------- | ------------------------------------------------------------ |
| **Feature**       | Queue                                                        |
| **Preconditions** | Library has at least two songs.                              |
| **Steps**         | 1. Right-click a song and select **Add to queue**.           |
|                   | 2. Open the queue panel.                                     |
|                   | 3. Verify the song appears in the queue.                     |
|                   | 4. Add a second song to the queue.                           |
| **Expected**      | Both songs appear in the queue in the order they were added. |

### SMOKE-018 -- Remove a song from the queue

| Field             | Detail                                                        |
| ----------------- | ------------------------------------------------------------- |
| **Feature**       | Queue                                                         |
| **Preconditions** | Queue has at least one song.                                  |
| **Steps**         | 1. Open the queue panel.                                      |
|                   | 2. Remove a song from the queue (via button or context menu). |
| **Expected**      | The song is removed. Remaining songs stay in order.           |

### SMOKE-019 -- Reorder songs in the queue

| Field             | Detail                                                                  |
| ----------------- | ----------------------------------------------------------------------- |
| **Feature**       | Queue                                                                   |
| **Preconditions** | Queue has at least two songs.                                           |
| **Steps**         | 1. Open the queue panel.                                                |
|                   | 2. Drag a song to a different position.                                 |
| **Expected**      | The song moves to the new position. Playback order reflects the change. |

### SMOKE-020 -- Singer rotation: add singers and toggle

| Field             | Detail                                                                                           |
| ----------------- | ------------------------------------------------------------------------------------------------ |
| **Feature**       | Singer Rotation                                                                                  |
| **Preconditions** | Queue has at least two songs.                                                                    |
| **Steps**         | 1. Open the rotation controls (via playback bar or queue panel).                                 |
|                   | 2. Toggle rotation **on**.                                                                       |
|                   | 3. Add two singers (e.g. "Alice" and "Bob").                                                     |
|                   | 4. Verify both singers appear in the singer list.                                                |
|                   | 5. Play through the first song.                                                                  |
|                   | 6. When the song ends, verify the next singer is automatically assigned to the next queue entry. |
| **Expected**      | Rotation is active. Singers alternate in round-robin order across queue entries.                 |

### SMOKE-021 -- Singer rotation: assign singer to a queue entry

| Field             | Detail                                                                                                          |
| ----------------- | --------------------------------------------------------------------------------------------------------------- |
| **Feature**       | Singer Rotation                                                                                                 |
| **Preconditions** | Rotation is active with multiple singers. Queue has entries.                                                    |
| **Steps**         | 1. Open the queue panel.                                                                                        |
|                   | 2. Assign a specific singer to a queue entry (via singer picker or drag).                                       |
|                   | 3. Verify the assignment is shown on the queue entry.                                                           |
| **Expected**      | The singer assignment is reflected in the UI. The assigned singer sings that song regardless of rotation order. |

### SMOKE-022 -- Singer rotation: disable rotation

| Field             | Detail                                                                 |
| ----------------- | ---------------------------------------------------------------------- |
| **Feature**       | Singer Rotation                                                        |
| **Preconditions** | Rotation is active.                                                    |
| **Steps**         | 1. Toggle rotation **off**.                                            |
|                   | 2. Play a song.                                                        |
|                   | 3. When it ends, verify the next song plays without singer assignment. |
| **Expected**      | Rotation is disabled. Queue plays in plain FIFO order.                 |

---

## 7. CD+G Rendering

### SMOKE-023 -- Play a CD+G file

| Field             | Detail                                                                                                         |
| ----------------- | -------------------------------------------------------------------------------------------------------------- |
| **Feature**       | CD+G                                                                                                           |
| **Preconditions** | A CD+G file (.cdg with matching audio) is imported.                                                            |
| **Steps**         | 1. Play the CD+G song from the library.                                                                        |
|                   | 2. Verify the CD+G canvas renders the karaoke graphics.                                                        |
|                   | 3. Verify graphics update in sync with the audio.                                                              |
| **Expected**      | CD+G graphics are rendered in the playback stage. No visual glitches or dropped frames during normal playback. |

---

## 8. AirPlay Streaming

### SMOKE-024 -- AirPlay route button visibility

| Field             | Detail                                                                               |
| ----------------- | ------------------------------------------------------------------------------------ |
| **Feature**       | AirPlay                                                                              |
| **Platform**      | **[macOS]** only                                                                     |
| **Preconditions** | Running on macOS.                                                                    |
| **Steps**         | 1. Open the playback bar.                                                            |
|                   | 2. Verify the AirPlay route button is visible in the player toolbar.                 |
|                   | 3. Click the AirPlay button.                                                         |
|                   | 4. Verify the native macOS route picker appears.                                     |
| **Expected**      | The AirPlay button is visible and clickable on macOS. The native route picker opens. |

### SMOKE-025 -- AirPlay not shown on other platforms

| Field             | Detail                                                 |
| ----------------- | ------------------------------------------------------ |
| **Feature**       | AirPlay                                                |
| **Platform**      | **[Windows]**, **[Linux]**                             |
| **Preconditions** | Running on Windows or Linux.                           |
| **Steps**         | 1. Open the playback bar.                              |
|                   | 2. Verify the AirPlay route button is **not** visible. |
| **Expected**      | No AirPlay button is rendered on non-macOS platforms.  |

---

## 9. Settings

### SMOKE-026 -- Open and navigate settings

| Field             | Detail                                                                                                           |
| ----------------- | ---------------------------------------------------------------------------------------------------------------- |
| **Feature**       | Settings                                                                                                         |
| **Preconditions** | App is open.                                                                                                     |
| **Steps**         | 1. Open **Settings** (via menu or keyboard shortcut).                                                            |
|                   | 2. Verify the settings overlay opens.                                                                            |
|                   | 3. Navigate through each settings section (General, Library, Model, Stem Mode, Execution Provider, Danger Zone). |
|                   | 4. Close settings.                                                                                               |
| **Expected**      | Settings overlay opens and closes cleanly. All sections render without errors.                                   |

### SMOKE-027 -- Change language in settings

| Field             | Detail                                                           |
| ----------------- | ---------------------------------------------------------------- |
| **Feature**       | Settings / General                                               |
| **Preconditions** | App is open in English.                                          |
| **Steps**         | 1. Open **Settings > General**.                                  |
|                   | 2. Change language to Chinese (or the other available language). |
|                   | 3. Verify the UI switches language.                              |
|                   | 4. Close and reopen the app.                                     |
|                   | 5. Verify the language setting persisted.                        |
| **Expected**      | Language changes apply immediately and survive app restart.      |

### SMOKE-028 -- Model variant selection

| Field             | Detail                                                                                         |
| ----------------- | ---------------------------------------------------------------------------------------------- |
| **Feature**       | Settings / Model                                                                               |
| **Preconditions** | App is open.                                                                                   |
| **Steps**         | 1. Open **Settings > Model**.                                                                  |
|                   | 2. Verify available model variants are listed.                                                 |
|                   | 3. Select a different variant.                                                                 |
|                   | 4. Verify the selection is saved.                                                              |
| **Expected**      | Model variant changes are persisted. The selected model is used for the next vocal separation. |

---

## 10. Library Management

### SMOKE-029 -- Search the library

| Field             | Detail                                                                |
| ----------------- | --------------------------------------------------------------------- |
| **Feature**       | Library / Search                                                      |
| **Preconditions** | Library has at least three songs with distinct titles.                |
| **Steps**         | 1. Type a partial song title into the search box.                     |
|                   | 2. Verify the library list filters to matching songs.                 |
|                   | 3. Clear the search box.                                              |
|                   | 4. Verify all songs reappear.                                         |
| **Expected**      | Search filters results in real time. Clearing restores the full list. |

### SMOKE-030 -- Edit song metadata

| Field             | Detail                                                           |
| ----------------- | ---------------------------------------------------------------- |
| **Feature**       | Library / Song Edit                                              |
| **Preconditions** | Library has at least one song.                                   |
| **Steps**         | 1. Right-click a song and select **Edit** (or open properties).  |
|                   | 2. Change the title or artist field.                             |
|                   | 3. Save the changes.                                             |
|                   | 4. Verify the updated metadata is displayed in the library list. |
| **Expected**      | Metadata edits are saved and reflected in the UI.                |

### SMOKE-031 -- Delete a song from the library

| Field             | Detail                                                                               |
| ----------------- | ------------------------------------------------------------------------------------ |
| **Feature**       | Library                                                                              |
| **Preconditions** | Library has at least one song that is not currently playing.                         |
| **Steps**         | 1. Right-click the song.                                                             |
|                   | 2. Select **Delete** (or the equivalent destructive action).                         |
|                   | 3. Confirm the deletion.                                                             |
| **Expected**      | The song is removed from the library. It no longer appears in any playlist or queue. |

---

## 11. Remote Library (Remote Repository)

### SMOKE-032 -- Open remote repository wizard from settings

| Field             | Detail                                                                               |
| ----------------- | ------------------------------------------------------------------------------------ |
| **Feature**       | Remote Library                                                                       |
| **Preconditions** | App is open with a local library already configured.                                 |
| **Steps**         | 1. Open **Settings > Library**.                                                      |
|                   | 2. Look for the option to connect a remote repository.                               |
|                   | 3. Verify the remote provider list is shown (Google Drive, Dropbox, WebDAV).         |
| **Expected**      | Remote library options are accessible from settings. All three providers are listed. |

---

## Pre-Release Smoke Test

> **These tests MUST pass before any release is published.** They cover the minimum
> viable user experience: launch, import, play, and basic navigation.

| Test ID   | Description                                | Platforms      |
| --------- | ------------------------------------------ | -------------- |
| SMOKE-001 | First launch wizard completes              | All            |
| SMOKE-003 | Import a single audio file via file picker | All            |
| SMOKE-006 | Play a song from the library               | All            |
| SMOKE-007 | Seek within a song                         | All            |
| SMOKE-008 | Lyrics display and sync                    | All            |
| SMOKE-010 | Trigger vocal separation                   | All            |
| SMOKE-012 | Create a playlist                          | All            |
| SMOKE-017 | Add songs to the playback queue            | All            |
| SMOKE-020 | Singer rotation basic flow                 | All            |
| SMOKE-023 | Play a CD+G file                           | All            |
| SMOKE-024 | AirPlay route button visibility            | macOS only     |
| SMOKE-025 | AirPlay not shown on other platforms       | Windows, Linux |
| SMOKE-026 | Open and navigate settings                 | All            |
| SMOKE-029 | Search the library                         | All            |

**Estimated time**: ~25 minutes per platform.

---

## Post-Deploy Verification

> **Run these after every CI build that produces installable artifacts.** They verify
> the build is not broken and the app launches correctly.

| Step | Action                                                      | Expected                                             |
| ---- | ----------------------------------------------------------- | ---------------------------------------------------- |
| 1    | Download the installer artifact for each target platform.   | Files download without corruption.                   |
| 2    | Install the app on a clean machine (or clean user profile). | Installation completes without errors.               |
| 3    | Launch the app.                                             | The app window appears. No crash on startup.         |
| 4    | Complete the onboarding wizard (SMOKE-001).                 | Wizard finishes. Main view loads.                    |
| 5    | Import one song (SMOKE-003).                                | Song appears in library.                             |
| 6    | Play the song (SMOKE-006).                                  | Audio plays. No runtime errors in console.           |
| 7    | Open and close Settings (SMOKE-026).                        | Settings overlay works.                              |
| 8    | Close the app and relaunch.                                 | App starts without errors. Library data is retained. |

**Estimated time**: ~10 minutes per platform.

---

## Test Environment Notes

- **macOS**: Test on both Intel and Apple Silicon if both are supported.
- **Windows**: Test on Windows 10 and Windows 11.
- **Linux**: Test on at least one Ubuntu-based distro. Verify Flatpak build separately if applicable.
- **Network**: Some tests (lyrics fetch, remote library) require internet access. Vocal separation requires the ONNX model to be downloaded.
- **Audio**: Ensure the test machine has audio output available. Headphones are sufficient.

## Revision History

| Date       | Author | Change                     |
| ---------- | ------ | -------------------------- |
| 2026-06-07 | --     | Initial checklist created. |
