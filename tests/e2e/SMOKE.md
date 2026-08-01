# OpenKara Smoke Tests

This document is for local debugging and exploratory testing. It is not a
release acceptance gate. Product release acceptance comes only from required
GitHub Actions checks and retained artifacts: Windows/macOS/Linux installed-app
smoke (clean install, upgrade, uninstall, keyboard UI Automation), separation
smoke, Playwright accessibility matrix, runtime/model fault recovery
(`fault-injection`), report schema and workflow contract tests, and the release
publish job graph that cannot start when any of those gates fail.

## Automated (Playwright UI smoke)

Run the browser-based UI smoke suite against the Vite dev server:

```bash
pnpm test:ui-smoke
```

This tests the React frontend against a mocked Tauri IPC layer (no Rust
backend required). Coverage includes:

- App launch and library state detection
- Library setup wizard (first-run flow)
- Song list rendering and search filtering
- Playback controls (play/pause/skip/seek)
- Lyrics display and scroll viewport
- Queue panel toggle and empty state
- Rotation / singer management
- Playlist creation
- Keyboard modal behavior (initial focus, Tab/Shift+Tab containment, Escape,
  and focus restoration)
- Automated WCAG 2.2 A/AA checks in both light and dark themes via axe

## Automated (Release-only native and installed-app smoke)

The GitHub Actions **Release** workflow runs the **installed-app smoke** job
before any publish matrix job starts. The job is fully automated and does not
need a maintainer to trigger it.

The smoke gate builds the `openkara_automation_driver` binary with the
`automation-smoke` Cargo feature. The driver runs a scenario that:

1. Prepares a fresh app-data directory and downloads and verifies the Runtime
   and model through the production app-data bootstrap path.
2. Restarts from the prepared app-data directory, imports the WAV fixture,
   probes playback, separates it, and checks both stem artifacts.

The driver writes a canonical `automation-report.json` for the scenario, plus
legacy `installed-app-smoke-report.json` files for compatibility with the
existing validator. It exits with a non-zero status and fails the release if
any assertion is not met.

The machine-readable reports assert that the restart process did not download
the Runtime or model again. This is the release acceptance for the Windows
install, upgrade, download, restart, and separation lifecycle; it is not part
of ordinary PR CI.

To run the driver locally after building it:

```bash
pnpm automation:driver:run -- --scenario clean-install \
  --app-data-dir /tmp/oka-smoke/app-data \
  --input-dir src-tauri/tests/fixtures/audio \
  --output-dir /tmp/oka-smoke/output
```

## Exploratory Desktop Checks

These steps are useful for exploratory work with physical devices and external
accounts. They are not a release acceptance gate: deterministic product paths
are covered by the CI and release smoke suites above.

### Prerequisites

- `pnpm tauri dev` running (macOS, Windows, or Linux)
- At least one MP3+CDG or audio file available for import

### 1. First Run — Library Setup

1. Delete or rename the app's data directory to trigger first-run.
2. Launch `pnpm tauri dev`.
3. Verify the language selection screen appears with English and Chinese.
4. Select a language — the library setup step should appear.
5. Choose "Create new local library" — a native folder picker should open.
6. Select a folder — the app should create the library and advance to stem mode.
7. Choose a stem mode and click "Get Started".
8. The main app layout (sidebar + playback area) should appear.

### 2. Song Import

1. With the app running, use the menu or drag-and-drop to import songs.
2. Imported songs should appear in the sidebar song list.
3. Cover art thumbnails should render (if present in the file).

### 3. Playback

1. Double-click a song in the sidebar — playback should start.
2. The CDG canvas (if applicable) should display synced lyrics/graphics.
3. Pause/resume via the center play button.
4. Seek by clicking on the seek bar.
5. Skip forward/back with the skip buttons.
6. Verify volume sliders respond to interaction.

### 4. Lyrics Panel

1. With a song playing, verify synced lyrics appear in the lyrics panel.
2. Active line should be highlighted as playback progresses.
3. Click the "Romanize" button — romanized text should appear.
4. Adjust lyrics offset with the +/- controls.
5. Adjust font size with the font size controls.

### 5. Queue and Rotation

1. Right-click a song — "Add to Queue" context menu should appear.
2. Add multiple songs to the queue.
3. Open the queue panel — songs should appear in order.
4. Reorder songs via drag-and-drop.
5. Remove a song from the queue.
6. Add singers to the rotation — verify singer tags appear.
7. Assign a singer to a queue entry.

### 6. Playlists

1. Click "+ Create" in the sidebar playlists section.
2. Enter a playlist name — the playlist should appear in the sidebar.
3. Add songs to the playlist via context menu.
4. Click the playlist in the sidebar — song list should filter to that playlist.
5. Navigate back to "All Tracks".

### 7. Settings

1. Open settings (gear icon or menu).
2. Verify settings sections load (general, library, model, etc.).
3. Close settings — main layout should restore.

### 8. AirPlay / Audience Output (macOS only)

1. If available, test AirPlay output connection.
2. Verify audience view renders lyrics in fullscreen.

### 9. Window Chrome

1. Verify the window title bar renders correctly.
2. On macOS, verify traffic light buttons (close/minimize/zoom) work.
3. Resize the window — layout should adapt responsively.
4. Toggle the sidebar — it should collapse and expand smoothly.

## Full Tauri E2E (WebDriver)

For automated full-desktop E2E, Tauri supports WebDriver via
`tauri-driver`. This requires platform-specific WebDriver setup:

```bash
# Install tauri-driver
cargo install tauri-driver

# Start the WebDriver server
# macOS: safaridriver --port 4444
# Linux: geckodriver --port 4444 (or chromedriver)
# Windows: msedgedriver --port 4444

# Run the Tauri app in test mode
WEBDRIVER=1 pnpm tauri dev

# Write WebDriver tests using selenium-webdriver or similar
```

This approach is more complex and platform-dependent. The Playwright
UI smoke suite provides focused coverage for the React UI layer. It is not
the full desktop E2E path; release-only separation smoke covers runtime/model
bootstrap and real stem separation before publishing.

## Release acceptance

A release is accepted only when the required GitHub Actions jobs pass and the
retained artifacts are available. The required jobs include:

- cross-platform installed-app smoke
- Windows desktop E2E and UI Automation tree validation
- accessibility gates across the application surface
- runtime and model fault recovery
- workflow contract tests

The retained artifacts include the automation reports, UI Automation snapshots,
Playwright reports, and separation output metadata. axe alone does not prove
complete accessibility; the release gate combines DOM, native UI Automation,
keyboard, display, and installed-app validation.
