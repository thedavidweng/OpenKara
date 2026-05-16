# Current Implementation Status

> **Last updated:** 2026-05-14 · This file tracks the implementation status and is updated alongside releases.
> **Current source version:** 0.9.0  
> **Released source of truth:** `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` must match the version described here.

## Completed Milestones

### ✅ v0.1 — MVP (Released)

- [x] Project scaffolding (Tauri 2 + React + TypeScript + Vite)
- [x] SQLite database with migration system
- [x] Audio import with metadata extraction (ID3v2, Vorbis, FLAC)
- [x] Library search and browsing
- [x] Audio decode and playback (symphonia + cpal)
- [x] Playback state machine (play / pause / seek / volume)
- [x] Demucs v4 ONNX stem separation with progress tracking
- [x] Stems caching (hash-based, no re-inference on replay)
- [x] Karaoke mode toggle (original / instrumental)
- [x] Synced lyrics fetch (LRCLIB → embedded → sidecar .lrc)
- [x] Lyrics display with rAF-based sync and click-to-seek
- [x] Per-song lyrics timing offset
- [x] First-launch AI model bootstrap with background download
- [x] Portable library system with relative paths
- [x] Full frontend UI (sidebar, player, lyrics panel, settings)
- [x] Queue panel with play next, drag reorder, and auto-advance
- [x] Keyboard shortcuts (space, arrows)
- [x] Drag-and-drop file import
- [x] CI/CD pipeline (macOS, Windows, Linux)
- [x] Release automation (tag → GitHub Release with binaries)

### ✅ v0.2.0 — Released

OpenKara v0.2.0 is the release that established the current core app flow.

- [x] CD+G sidecar playback for same-name audio + `.cdg` pairs
- [x] MP3+G ZIP import and playback support
- [x] Managed CD+G library storage and pairing disambiguation
- [x] Second-display fullscreen audience window
- [x] 4-stem volume mixer with collapsible UI
- [x] Dual separation modes (2-stem / 4-stem) with settings persistence
- [x] Efficient compressed stem storage
- [x] Resumable separation with per-chunk checkpointing
- [x] Multi-threaded ONNX inference optimization
- [x] Settings system (stem mode configuration)
- [x] UI polish and transitions
- [x] Error toasts and user-facing error messages
- [x] App icon and branding

### ✅ v0.3.0 — Released

OpenKara v0.3.0 adds:

- [x] AirPlay support for casting playback to compatible devices
- [x] Improved player behavior and layout at narrow window widths
- [x] Visual refinements to the Windows app appearance
- [x] Better preservation of original track metadata on import
- [x] WinGet installation support on Windows

### ✅ v0.4.0 — Released

OpenKara v0.4.0 adds:

- [x] Refined macOS host chrome behavior, including tighter titlebar metrics and better traffic-light alignment
- [x] Fixed a crash that could occur after long idle/suspend periods

### ✅ v0.5.1 — Released

OpenKara v0.5.1 adds:

- [x] Upgraded separator runtime acceleration path to XNNPACK for more stable performance
- [x] Improved hardware acceleration provider selection and fallback behavior across settings and separation flow
- [x] Fixed song dialogs layering so dialogs reliably appear above list rows
- [x] Refined desktop titlebar controls placement for better usability
- [x] Includes lyrics auto-scroll behavior improvements

### ✅ v0.6.0 — Released

OpenKara v0.6.0 adds:

- [x] Remote Repository Support: Fully implemented connection, refresh, publish, playback, reauthorization, and deletion semantics for Google Drive, Dropbox, and WebDAV providers
- [x] Secure Credential Storage: Authentication tokens are now securely stored in the system Keychain (macOS) or Credential Manager (Windows)
- [x] Legal & Privacy: Added dedicated Privacy Policy and Terms of Service disclosures

### ✅ v0.7.0 — Released

OpenKara v0.7.0 adds:

- [x] Version metadata sync across the frontend package, Cargo, Tauri config, and release packaging validation
- [x] Online lyrics provider User-Agent metadata tied to the compiled app version
- [x] Pronunciation display for non-Latin lyrics using `lyric-romanizer`

### ✅ v0.8.0 — Released

OpenKara v0.8.0 adds:

- [x] **Playback & UI:** Optional full-window cover-art backdrop; improved click-through behavior for overlay UI; **play history** (capped at 500 entries to bound database growth)
- [x] **Lyrics:** Per-song **language** metadata with user override; **romanization** language override; automatic re-romanization when the language changes; plain-text lyric formatting and scroll spacing fixes; fix for lyrics sync stalling during normal playback
- [x] **Romanization assets:** Kuromoji dictionary is **bundled/served locally** (no runtime fetch from a public CDN for the dictionary payload)
- [x] **Remote libraries:** Unified **reauthorization** flow across remote repository providers
- [x] **Dependencies:** Routine Tauri and ecosystem dependency refreshes

### ✅ v0.8.1 — Released

OpenKara v0.8.1 is a **packaging, release automation, and CI** follow-up on v0.8.0, including:

- [x] Flatpak source-build path fixes (offline `pnpm`/Cargo layout, manifest paths, Flathub validation and compliance tweaks)
- [x] WinGet automation: fallback compare URL when upstream PR creation is unavailable
- [x] Linux release metadata and draft-release handling fixes

### ✅ v0.9.0 — Source Complete (Pending Tag)

The v0.9.0 cycle is **complete in source** (merged via PR #20, commit `0eb8808`). The next tagged release will ship these items. The v0.9 plan itself has been archived — see `docs/archive/plans/2026-05-13-v0.9-hardening-and-playlists-plan.md`.

**Hardening (H1–H8):**

- [x] **H1** — Lyrics pipeline hardening: sync stall fix, plain-text layout, romanization language overrides, automated test coverage expansion
- [x] **H2** — Remote libraries: OAuth/WebDAV smoke tests, structured error handling per `phase-5-error-contract.md`, token redaction in logs
- [x] **H3** — Separation runtime: ONNX provider selection stability, XNNPACK fallback, session lifecycle cleanup, limits documentation
- [x] **H4** — AirPlay & presentation: second-window fullscreen audience output contract alignment, gated test policy for CI
- [x] **H5** — Packaging & supply chain: WinGet/Flatpak manifest generators validated, release workflow permissions explicit
- [x] **H6** — Documentation ownership: `F1-playlists-and-singer-rotation.md`, `queue-management.md`, and `releasing.md` updates
- [x] **H7** — Generated DB schema doc: `scripts/generate-db-schema.mjs` idempotent regenerator + `pnpm generate:db-schema`
- [x] **H8** — Release readiness without paid Apple signing:
  - **H8.1** — `SHA256SUMS` generated by CI for every release
  - **H8.2** — Forward-only migration policy documented; F1 migration tests exist
  - **H8.3** — Model bootstrap recovery: retry, clear cache, open folder paths
  - **H8.4** — `pnpm audit` green in CI; `cargo audit` last scan 2026-05-13 (0 vulns)
  - **H8.5** — Diagnostics: log rotation, redaction rules, About dialog with version + SHA
  - **H8.6** — i18n smoke: `check-i18n.mjs` script validates `en` + `zh-CN` key coverage
  - **H8.7** — Pre-release checklist maintained in `releasing.md`

**New capability F1 — Saved playlists & singer rotation:**

- [x] **Playlist CRUD:** create, rename, delete with inline editing — ✅ **Verified working**
- [x] **Add/remove songs to playlists:** Library context menu exposes "Add to Playlist…" with checked/mixed membership indicators; playlist detail rows expose "Remove from Playlist" — ✅ **Verified**
- [x] **Singer rotation:** Queue panel exposes rotation controls, singer tags, queue-entry singer assignment, and automatic round-robin assignment when songs are queued — ✅ **Verified**
- [x] **Data layer:** SQLite schema migrations (`008_playlists.sql`, `009_singer_rotation.sql`) — ✅ **Verified**
- [x] **IPC commands:** `list_playlists`, `create_playlist`, `rename_playlist`, `delete_playlist`, `add_songs_to_playlist`, `remove_songs_from_playlist`, `get_playlist_songs`, `set_rotation_state`, `get_rotation_state`, `advance_rotation`, `set_queue_entry_singer` — ✅ **Backend tested**
- [x] **i18n strings:** `en` and `zh-CN` keys present — ✅ `check-i18n.mjs` passes
- [x] **Rust tests:** persistence and rotation rules covered — ✅ **Green**

> ✅ **F1 status:** Source-complete and locally verified. Remaining coverage is manual app smoke on the packaged macOS build for right-click/menu interactions and queue-panel rotation controls.

**Deferred from v0.9:**

- `cargo deny` setup (scheduled weekly check)
- CycloneDX SBOM generation
- H2 WebDAV smoke on Windows/Linux (needs maintainer access)
- Windows DirectML validation in CI (needs GPU runner)
- macOS codesign + notarization (requires Apple Developer Program)
- Windows Authenticode (requires paid code-signing cert)

## Planned Future Features

### 🎯 v0.9 and Beyond

The **current execution plan skeleton** lives in **[`docs/plans/plan.md`](./plans/plan.md)**. Historical priority-only snapshot: [`archive/plans/future-work-and-hardening-priorities-2026-05.md`](./archive/plans/future-work-and-hardening-priorities-2026-05.md).

High-level backlog (unchanged intent, version bucket renamed from “v0.8+” now that v0.8 has shipped):

- **Mic Input & Vocal Effects** — Microphone capture, reverb, echo, volume mix
- **Saved Playlists & Singer Rotation** — Named playlists, singer assignment, and stronger turn-based queue workflows
- **Pitch & Key Shift** — Real-time pitch shifting of the accompaniment track
- **Session Recording** — Record vocal performances, export as audio
- **Mobile Companion App** — Remote control and lyrics display on phone/tablet

---

_For the current technical roadmap, see [Technical Roadmap](./design-docs/roadmap.md)._
