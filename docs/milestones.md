# Milestones

> Each milestone is a shippable increment. The **Demo** column describes what a stakeholder should see at the end of the milestone. The **Exit Criteria** column lists the hard requirements to move on.

---

## Overview

```
M0        M1          M2          M3          M4          M5         M6
 │         │           │           │           │           │          │
 ▼         ▼           ▼           ▼           ▼           ▼          ▼
Shell  ─→ Library ─→ Playback ─→ Separation ─→ Lyrics ─→ Polish ─→ Release
                                   ║                       ║
                                   ╚══════ parallel ══════╝
```

---

## M0 — Empty Shell

| Item              | Detail                                            |
| ----------------- | ------------------------------------------------- |
| **Goal**          | Project compiles and runs on all target platforms |
| **Phases**        | Phase 0                                           |
| **Demo**          | `pnpm tauri dev` → empty window with app title    |
| **Exit Criteria** | CI green (lint + build) on macOS, Windows, Linux  |

### Task Breakdown

| Task                             | Owner | Status | Notes                    |
| -------------------------------- | ----- | ------ | ------------------------ |
| Init Tauri 2 + React + TS + Vite | —     | ☐      | `pnpm create tauri-app`  |
| Tailwind CSS setup               | —     | ☐      |                          |
| ESLint + Prettier config         | —     | ☐      |                          |
| SQLite migration infra           | —     | ☐      | Empty `songs` table      |
| ONNX model download script       | —     | ☐      | `scripts/setup.sh`       |
| GitHub Actions CI                | —     | ☐      | lint, build, test matrix |

---

## M1 — Music Library

| Item              | Detail                                                                                |
| ----------------- | ------------------------------------------------------------------------------------- |
| **Goal**          | User imports local music and browses a library                                        |
| **Phases**        | Phase 1                                                                               |
| **Demo**          | Drag 10 MP3s into app → grid with cover art, title, artist. Search filters live.      |
| **Exit Criteria** | Metadata reads correctly for MP3, FLAC, M4A. Songs persist in SQLite across restarts. |

### Task Breakdown

| Task                          | Owner | Status | Notes                    |
| ----------------------------- | ----- | ------ | ------------------------ |
| Metadata reader (lofty)       | —     | ☐      | ID3v2, Vorbis, FLAC tags |
| SQLite songs CRUD             | —     | ☐      | insert, query, delete    |
| `import_songs` Tauri command  | —     | ☐      | Accept file paths        |
| Library UI — grid + list view | —     | ☐      | Responsive layout        |
| Drag-and-drop import          | —     | ☐      | + file picker fallback   |
| Library search + filter       | —     | ☐      | By title, artist         |

---

## M2 — Audio Playback

| Item              | Detail                                                                                  |
| ----------------- | --------------------------------------------------------------------------------------- |
| **Goal**          | Full playback of original audio with transport controls                                 |
| **Phases**        | Phase 2                                                                                 |
| **Demo**          | Click a song → plays through speakers. Seek bar, volume, play/pause all work.           |
| **Exit Criteria** | Gapless decode for MP3/FLAC/WAV. Seek latency < 200ms. No audio glitches on rapid seek. |

### Task Breakdown

| Task                            | Owner | Status | Notes                        |
| ------------------------------- | ----- | ------ | ---------------------------- |
| Audio decode (symphonia)        | —     | ☐      | MP3, FLAC, WAV, OGG, AAC     |
| Audio output (cpal)             | —     | ☐      | Platform-specific backends   |
| Playback state machine          | —     | ☐      | play / pause / stop / seek   |
| Position event emitter (~60 Hz) | —     | ☐      | Tauri events                 |
| Player UI — controls            | —     | ☐      | Play/pause, seek bar, volume |
| Zustand playerStore             | —     | ☐      | Global playback state        |

---

## M3 — AI Stem Separation

| Item              | Detail                                                                                                     |
| ----------------- | ---------------------------------------------------------------------------------------------------------- |
| **Goal**          | Any song can be separated into vocals + instrumental. Cached for instant replay.                           |
| **Phases**        | Phase 3                                                                                                    |
| **Demo**          | Click "Karaoke Mode" → progress bar → instrumental plays without vocals. Second play is instant.           |
| **Exit Criteria** | Separation completes for a 4-min song in < 90s on M1 Mac. Cache hit → < 500ms to play. Peak memory < 3 GB. |

### Task Breakdown

| Task                                | Owner | Status | Notes                      |
| ----------------------------------- | ----- | ------ | -------------------------- |
| Load Demucs ONNX model              | —     | ☐      | `ort::Session`             |
| PCM → model input preprocessing     | —     | ☐      | Chunking, tensor shape     |
| Inference + overlap-add postprocess | —     | ☐      | 4 stems → 2 outputs        |
| Stems cache (fs, hash-based)        | —     | ☐      | `~/.openkara/cache/stems/` |
| Background processing (tokio)       | —     | ☐      | Non-blocking UI            |
| Progress events                     | —     | ☐      | Percent complete           |
| Mode toggle UI (original / karaoke) | —     | ☐      | Player component           |

---

## M4 — Synced Lyrics

| Item              | Detail                                                                                                              |
| ----------------- | ------------------------------------------------------------------------------------------------------------------- |
| **Goal**          | Lyrics scroll in sync with playback, highlighted line-by-line                                                       |
| **Phases**        | Phase 4                                                                                                             |
| **Demo**          | Song plays → lyrics panel shows lines scrolling and highlighting in real time. Click a line → playback jumps there. |
| **Exit Criteria** | Lyrics sync jitter < 50ms. LRCLIB fetch success rate > 70% for English pop songs. Offset adjustment persists.       |

### Task Breakdown

| Task                         | Owner | Status | Notes                         |
| ---------------------------- | ----- | ------ | ----------------------------- |
| LRCLIB API client            | —     | ☐      | HTTP GET with metadata params |
| LRC parser                   | —     | ☐      | Regex parse → structured data |
| Fetch priority chain         | —     | ☐      | LRCLIB → embedded → sidecar   |
| Lyrics SQLite cache          | —     | ☐      | song_hash → lrc               |
| Lyrics UI component          | —     | ☐      | Scrolling panel, highlight    |
| rAF + performance.now() sync | —     | ☐      | useLyricsSync hook            |
| Click-to-seek on lyric line  | —     | ☐      |                               |
| Timing offset controls       | —     | ☐      | ± 0.5s, persisted             |

---

## M5 — Integration & Polish

| Item              | Detail                                                                                                                |
| ----------------- | --------------------------------------------------------------------------------------------------------------------- |
| **Goal**          | Complete, polished karaoke experience end-to-end                                                                      |
| **Phases**        | Phase 5                                                                                                               |
| **Demo**          | Import song → auto-separate → lyrics appear → sing along with instrumental. Feels like a real product.                |
| **Exit Criteria** | 5 different songs tested end-to-end without errors. Keyboard shortcuts work. Loading states for all async operations. |

### Task Breakdown

| Task                           | Owner | Status | Notes                         |
| ------------------------------ | ----- | ------ | ----------------------------- |
| E2E flow testing (5+ songs)    | —     | ☐      | Different formats & languages |
| Error handling & user feedback | —     | ☐      | Toasts, fallback states       |
| Performance profiling          | —     | ☐      | Latency, jitter, memory       |
| UI polish & transitions        | —     | ☐      | Smooth, responsive            |
| Keyboard shortcuts             | —     | ☐      | Space, arrows, etc.           |
| App branding (icon, splash)    | —     | ☐      |                               |
| Documentation update           | —     | ☐      | Install + usage guide         |

---

## M6 — v0.1.0 Release

| Item              | Detail                                                                                                          |
| ----------------- | --------------------------------------------------------------------------------------------------------------- |
| **Goal**          | Downloadable binaries on GitHub for macOS, Windows, Linux                                                       |
| **Phases**        | Phase 6                                                                                                         |
| **Demo**          | User downloads `.dmg` / `.exe` / `.AppImage`, installs, imports music, and sings karaoke.                       |
| **Exit Criteria** | Smoke test passes on all 3 platforms. GitHub Release with checksums published. README has install instructions. |

### Task Breakdown

| Task                     | Owner | Status | Notes                               |
| ------------------------ | ----- | ------ | ----------------------------------- |
| Tauri build config       | —     | ☐      | App ID, signing, targets            |
| CI build matrix          | —     | ☐      | macOS (arm64 + x64), Windows, Linux |
| Release automation       | —     | ☐      | Tag → Release with artifacts        |
| First-run model download | —     | ☐      | Progress UI                         |
| Platform smoke tests     | —     | ☐      | Manual per-platform                 |
| Homebrew formula         | —     | ☐      | macOS distribution                  |

---

## Post-MVP Milestones (Future)

These milestones are scoped but not scheduled. They become relevant after v0.1.0 is released and validated.

| Milestone                      | Scope                                            |
| ------------------------------ | ------------------------------------------------ |
| M7 — Mic Input & Vocal Effects | Microphone capture, reverb, echo, volume mix     |
| M8 — Playlist & Queue          | Multi-song queue, multi-user turn-based queue    |
| M9 — Pitch & Key Shift         | Real-time pitch shifting of accompaniment track  |
| M10 — Session Recording        | Record user's vocal performance, export as audio |
| M11 — Multi-screen             | Second display for audience lyrics view          |
| M12 — CJK Transliteration      | Romaji/Pinyin display alongside original lyrics  |

---

## How to Use This Document

1. **Project manager**: Assign owners to tasks in the current milestone. Track status (☐ → ⏳ → ✅).
2. **New engineer**: Find the current milestone, read its Exit Criteria, then pick an unowned task.
3. **Reviewer**: Check Exit Criteria before signing off on a milestone.
4. **Stakeholder**: Read the Demo column to understand what each milestone delivers.
