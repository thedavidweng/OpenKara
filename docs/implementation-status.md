# Current Implementation Status

> **Last updated:** 2026-05-13 · This file tracks the implementation status and is updated alongside releases.  
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

### ✅ v0.8.1 — Current App Version

OpenKara v0.8.1 is the current source and package version. It is primarily a **packaging, release automation, and CI** follow-up on v0.8.0, including:

- [x] Flatpak source-build path fixes (offline `pnpm`/Cargo layout, manifest paths, Flathub validation and compliance tweaks)
- [x] WinGet automation: fallback compare URL when upstream PR creation is unavailable
- [x] Linux release metadata and draft-release handling fixes

Work **after** the `v0.8.1` tag on `main` (CI hardening, CodeQL-driven logging fixes, Windows test staging, flaky-test quarantine) lands in source control first; the next tagged release will document those items when shipped.

## Planned Future Features

### 🎯 v0.9 and Beyond

The **current execution plan** (hardening H1–H8 including release readiness without paid Apple signing, plus new capability F1) lives in **[`docs/plan/plan.md`](./plan/plan.md)**. Historical priority-only snapshot: [`archive/plans/future-work-and-hardening-priorities-2026-05.md`](./archive/plans/future-work-and-hardening-priorities-2026-05.md).

High-level backlog (unchanged intent, version bucket renamed from “v0.8+” now that v0.8 has shipped):

- **Mic Input & Vocal Effects** — Microphone capture, reverb, echo, volume mix
- **Saved Playlists & Singer Rotation** — Named playlists, singer assignment, and stronger turn-based queue workflows
- **Pitch & Key Shift** — Real-time pitch shifting of the accompaniment track
- **Session Recording** — Record vocal performances, export as audio
- **Mobile Companion App** — Remote control and lyrics display on phone/tablet

---

_For the current technical roadmap, see [Technical Roadmap](./design-docs/roadmap.md)._
