# Architecture

## Overview

OpenKara is a cross-platform desktop karaoke application built with Tauri 2. It combines a Rust backend for audio processing and AI inference with a React frontend for the karaoke UI.

```
User's local music files / Remote repositories
        │                        │
        ▼                        ▼
┌──────────────────────────────────────────────┐
│              Tauri Frontend (React)           │
│                                               │
│  ┌────────────┐  ┌─────────────────────────┐ │
│  │ File Import │  │     Karaoke Player UI   │ │
│  │ & Library   │  │  (lyrics sync/highlight)│ │
│  ├────────────┤  ├─────────────────────────┤ │
│  │  Playlists  │  │   Playback & Volume     │ │
│  │  & Rotation │  │   Controls              │ │
│  ├────────────┤  ├─────────────────────────┤ │
│  │  Remote     │  │   AirPlay / Fullscreen  │ │
│  │  Repository │  │   Presentation          │ │
│  │  Wizard     │  │                         │ │
│  └────────────┘  └─────────────────────────┘ │
├──────────────────────────────────────────────┤
│              Tauri Rust Backend               │
│                                               │
│  ┌────────────┐  ┌─────────────────────────┐ │
│  │   Audio     │  │    AI Stem Separation   │ │
│  │   Decode &  │  │    (Demucs v4 via       │ │
│  │   Streaming │  │     ONNX Runtime)       │ │
│  ├────────────┤  ├─────────────────────────┤ │
│  │  Metadata   │  │    Lyrics + Romanizer   │ │
│  │  Reader     │  │    (LRCLIB + embedded)  │ │
│  ├────────────┤  ├─────────────────────────┤ │
│  │  Remote     │  │    AirPlay Streaming    │ │
│  │  Providers  │  │    (HLS + route ctrl)   │ │
│  │  (GDrive,   │  │                         │ │
│  │  Dropbox,   │  │                         │ │
│  │  WebDAV)    │  │                         │ │
│  ├────────────┴──┴─────────────────────────┤ │
│  │         Cache Layer (SQLite + fs)        │ │
│  │  stems / lyrics / metadata / playlists   │ │
│  │  ChunkedCache (streaming media cache)    │ │
│  └──────────────────────────────────────────┘ │
└──────────────────────────────────────────────┘
```

## Tech Stack

| Layer             | Technology                                                                                                                                  |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| Desktop framework | [Tauri 2](https://github.com/tauri-apps/tauri) (Rust + WebView)                                                                             |
| Frontend          | [React](https://github.com/facebook/react) + [TypeScript](https://github.com/microsoft/TypeScript) + [Vite](https://github.com/vitejs/vite) |
| Audio decode      | [symphonia](https://github.com/pdeljanov/Symphonia) (Rust)                                                                                  |
| Audio playback    | [cpal](https://github.com/RustAudio/cpal) (Rust)                                                                                            |
| AI inference      | [ONNX Runtime](https://github.com/microsoft/onnxruntime) (ort crate)                                                                        |
| AI model          | Demucs v4 (HTDemucs), ONNX export                                                                                                           |
| Lyrics API        | LRCLIB (primary), LrcApi (`api.lrc.cx`) fallback                                                                                            |
| Metadata          | [lofty](https://github.com/Serial-ATA/lofty-rs) (Rust, ID3/Vorbis/FLAC tags)                                                                |
| Cache / DB        | [SQLite](https://github.com/sqlite/sqlite) via rusqlite                                                                                     |
| Build / Bundle    | Tauri CLI + [Vite](https://github.com/vitejs/vite)                                                                                          |
| Distribution      | GitHub Releases, Homebrew, WinGet, Flatpak packaging                                                                                        |

## Data Flow

### Import & Separation

```
1. User drags audio file into the app
2. Rust backend reads file metadata (title, artist, album art)
3. Rust backend checks cache for existing separated stems
4. If cache miss:
   a. Decode audio to PCM (symphonia)
   b. Run Demucs v4 ONNX model → vocals + accompaniment
   c. Write separated stems to cache directory
5. Return metadata + stem paths to frontend
```

### Lyrics Fetch

```
1. Use song title + artist from metadata
2. Query LRCLIB API for synced LRC lyrics
3. If not found, try the LrcApi provider (`api.lrc.cx`)
4. If not found, check for embedded lyrics in audio file tags
5. If not found, check for .lrc file alongside audio file
6. User can also: import .lrc files (auto-matched) or type lyrics manually
7. Cache result in SQLite (source: "lrclib" | "lrc_api" | "embedded" | "sidecar" | "manual")
```

### Playback

```
1. Frontend sends play command with song ID
2. Rust backend streams the accompaniment stem to audio output
3. Frontend receives current playback position via event stream
4. Frontend highlights the current lyric line based on LRC timestamps
5. User clicks a lyric line → seek to that timestamp
```

## Lyrics System

> Reference implementation: [monochrome](https://github.com/monochrome-music/monochrome) (`js/lyrics.js`)
> monochrome is a production music player with a well-tested synced lyrics system. The design below draws from its proven approach.

### LRC Format

LRC is the standard synced lyrics format. Each line carries a timestamp:

```
[00:12.34] First line of lyrics
[00:17.89] Second line of lyrics
```

- Timestamp precision: centiseconds (0.01s), format `[MM:SS.CC]`
- Parse regex: `/\[(\d+):(\d+)\.(\d+)\]\s*(.+)/`
- Parsed into array of `{ time: number, text: string, words?: WordToken[] }` objects
- Line-level sync (entire line highlights at once)
- Enhanced LRC: word-level timing via `WordToken { time_ms, text }` for per-word highlighting

### LRCLIB API

Primary lyrics source. Free, open, no API key required.

```
GET https://lrclib.net/api/get?track_name={title}&artist_name={artist}&album_name={album}&duration={seconds}
```

- Returns JSON with `syncedLyrics` field (LRC string) and `plainLyrics` fallback
- `album_name` and `duration` are optional but improve match accuracy
- Matching is done server-side by metadata, not audio fingerprint

### Lyrics Fetch Priority

| Priority | Source                  | Notes                                                             |
| -------- | ----------------------- | ----------------------------------------------------------------- |
| 1        | LRCLIB API              | Best coverage for synced lyrics                                   |
| 2        | LrcApi (`api.lrc.cx`)   | Secondary timed lyrics provider using title/artist/album metadata |
| 3        | Embedded lyrics in tags | ID3v2 SYLT/USLT, Vorbis LYRICS tag — extracted during import      |
| 4        | Sidecar .lrc file       | Same directory, same filename as audio                            |
| 5        | LRC file import         | User imports .lrc files, auto-matched by filename or artist/title |
| 6        | Manual input            | User-typed plain text or LRC with auto-detection                  |

### Playback Sync Mechanism

> Learned from monochrome's `setupSync()` — a high-precision approach using `requestAnimationFrame` + `performance.now()`.

The naive approach (relying solely on `timeupdate` events) has two problems:

- `timeupdate` fires only ~4 times per second (every ~250ms)
- Timing jitter makes lyric transitions feel choppy

The proven solution:

```
┌─ Audio Events ──────────────────────────────────┐
│ play / seeked / timeupdate                      │
│   → Record baseTimeMs = currentTime * 1000      │
│   → Record lastTimestamp = performance.now()     │
└─────────────────────────────────────────────────┘
        │
        ▼
┌─ requestAnimationFrame Loop (60 FPS) ──────────┐
│ Each frame:                                     │
│   elapsed = performance.now() - lastTimestamp   │
│   currentMs = baseTimeMs + elapsed              │
│   lyricsComponent.currentTime = currentMs       │
│                                                 │
│ On pause → cancelAnimationFrame                 │
│ On play  → restart loop                         │
└─────────────────────────────────────────────────┘
```

Key points:

- `performance.now()` provides sub-millisecond precision for interpolation between audio events
- The loop runs only while audio is playing (paused → cancel)
- `timeupdate` and `seeked` events re-anchor `baseTimeMs` to prevent drift

### Per-Song Timing Offset

Users can adjust lyrics timing per song (e.g., lyrics arrive 0.5s early):

- Stored in cache: `lyrics_offset_{song_hash}` → offset in milliseconds
- Applied during sync: `currentMs = baseTimeMs + elapsed - timingOffset`
- Positive offset = lyrics delayed, negative = lyrics advanced
- UI: +/- buttons with 0.5s increments and a reset button

### Lyrics romanization & CJK display

OpenKara ships optional **romanization** for non-Latin lyrics (`lyric-romanizer`), with per-song **language** metadata and overrides so transliteration can track the singer’s intent. The **kuromoji** dictionary used for Japanese segmentation is bundled/served locally (no public CDN dependency for that payload).

Remaining polish items tend to be **font fallback** quality and **detection heuristics** when metadata is wrong — treat those as UX hardening rather than a single “future feature” gate.

monochrome’s historical approach (kuroshiro/kuromoji over the web) informed the design; OpenKara’s implementation is frontend-local with explicit language control.

## AI Model Details

### Demucs v4 (HTDemucs)

- **Purpose**: Separate audio into vocals and accompaniment (drums + bass + other)
- **License**: MIT
- **Input**: Raw PCM audio (44100 Hz, stereo)
- **Output**: 4 stems (vocals, drums, bass, other). We mix drums + bass + other into a single accompaniment track.
- **Model size**: `htdemucs` v2.0.1 is ~339 MiB on disk; `htdemucs_ft` v2.0.1 is ~1.32 GiB (see GitHub release assets for exact byte counts)
- **Inference time**: ~30-60s per 4-min song on Apple Silicon, ~2-3 min on older CPUs
- **Runtime**: ONNX Runtime with platform defaults chosen internally by the app. XNNPACK provides SIMD-accelerated FP32 inference on ARM64 (NEON) and x86-64 (AVX2/AVX-512) without AOT compilation overhead. DirectML remains available on Windows for GPU-accelerated inference and falls back through XNNPACK to CPU if the GPU path fails.

### Why Demucs

- Best open-source separation quality (SDR benchmarks)
- MIT licensed
- Well-documented ONNX export path
- Active maintenance by Meta Research

### Alternatives Considered

| Model      | Pros                     | Cons                          |
| ---------- | ------------------------ | ----------------------------- |
| Open-Unmix | Lighter weight           | Lower separation quality      |
| Spleeter   | Fast, well-known         | Outdated, lower quality       |
| BSRNN      | State-of-the-art quality | Larger model, slower, complex |

## Caching Strategy

All expensive computations are cached to avoid redundant processing:

- **Separated stems**: Stored as compact OGG/Vorbis files under the active portable library's `stems/{hash}/` directory
- **Lyrics**: Stored in SQLite with song hash as key
- **Metadata**: Stored in SQLite for library browsing
- **Timing offsets**: Stored in SQLite per song hash

The cache key is a SHA-256 hash of the audio file content, ensuring deduplication even if files are renamed or moved.

## Platform Considerations

| Platform | Audio Backend   | AI Acceleration                                      |
| -------- | --------------- | ---------------------------------------------------- |
| macOS    | CoreAudio       | XNNPACK (NEON SIMD) by default on all Apple hardware |
| Windows  | WASAPI          | DirectML by default; falls back to XNNPACK then CPU  |
| Linux    | PulseAudio/ALSA | XNNPACK by default                                   |

ONNX Runtime CPU execution provider works on all platforms out of the box. Hardware acceleration is configured via the **Hardware Acceleration** setting in Preferences, which only exposes explicit providers such as `CPU`, `XNNPACK`, and `DirectML`. When the setting is unset, the app chooses a platform default internally. Session setup logs the requested provider path, still falls back to CPU if the selected accelerated provider fails during session creation, keys the in-process model session cache with `openkara.model_cache_key` when present, and disables runtime graph optimization for models tagged with `openkara.optimized_by=onnxruntime`.

## Backend Architecture

### Decomposed AppState

The Rust backend `AppState` is composed of five domain modules:

```
AppState
├── PlaybackState    — playback controller, audio output, streaming state
├── AirPlayState     — AirPlay HTTP server, route discovery
├── SeparationState  — ONNX model cache, separation jobs
├── RemoteState      — remote repository connections, provider dispatch
└── AppShell         — window chrome, menu state
```

Each module owns its `Arc<Mutex<...>>` state and exposes domain-specific methods. IPC commands in `commands/` compose across modules.

### Typed Error Handling

IPC commands use a typed `ErrorCode` enum with `FallbackAction` hints for the frontend. Domain errors (`PlaybackError`, `CacheError`, `FetchError`) convert into `ErrorCode` variants, providing structured recovery signals instead of string matching.

### Remote Repository System

```
┌─────────────────────────────────────────────────┐
│  Frontend: RemoteLibraryWizard / Settings UI    │
├─────────────────────────────────────────────────┤
│  IPC: register_remote_library, refresh, publish │
├─────────────────────────────────────────────────┤
│  RemoteProvider trait                           │
│  ├── GoogleDriveProvider (OAuth 2.0)            │
│  ├── DropboxProvider (OAuth 2.0)                │
│  └── WebDAVProvider (Basic Auth)                │
├─────────────────────────────────────────────────┤
│  Local Working Copy (SQLite + media files)      │
│  Remote Revision tracking for conflict safety   │
└─────────────────────────────────────────────────┘
```

- **Credentials** stored in OS keychain (macOS Keychain, Windows Credential Manager, Linux secret-tool)
- **Pre-Mutation Refresh**: automatic refresh before local edits when remote revision is newer
- **Pre-Publish Conflict**: safety stop when remote changes after local edit but before publish

### Streaming Playback

For remote audio, playback uses a streaming architecture:

```
Remote URL → Fetch Thread → ChunkedCache (disk) → Symphonia Decode → Audio Output
                ↑                                        │
          BandwidthMonitor ←── prefetch tracking ────────┘
```

- **ChunkedCache**: disk-backed byte-range cache with LRU eviction and condvar-based blocking reads
- **RemoteMediaSource**: implements `Read + Seek + MediaSource` for symphonia, backed by the chunked cache
- **ProviderFetcher**: HTTP Range fetcher with automatic token refresh on 403
- **BandwidthMonitor**: EWMA bandwidth estimation with automatic low-bitrate proxy mode for slow connections
- **Retry**: exponential backoff with configurable max retries and consecutive failure threshold

### Playlists & Singer Rotation

SQLite schema migrations (`008_playlists.sql`, `009_singer_rotation.sql`) add:

- `playlists` and `playlist_songs` tables for saved playlists
- `rotation_state` table with round-robin singer queue assignment
- IPC commands: `create_playlist`, `add_songs_to_playlist`, `advance_rotation`, etc.

## Tech Stack Additions

| Layer                | Technology                                                                            |
| -------------------- | ------------------------------------------------------------------------------------- |
| Streaming cache      | Custom `ChunkedCache` with `RangeSet` tracking, condvar blocking reads                |
| HTTP Range fetch     | `reqwest` (blocking) with `rustls` TLS                                                |
| Bandwidth monitoring | Custom EWMA `BandwidthMonitor`                                                        |
| Remote providers     | `GoogleDriveProvider`, `DropboxProvider`, `WebDAVProvider` via `RemoteProvider` trait |
| Credential storage   | OS keychain (macOS), Credential Manager (Windows), secret-tool (Linux)                |
| Romanization         | `lyric-romanizer` crate + bundled `kuromoji` dictionary                               |
| Virtualization       | `@tanstack/react-virtual` for efficient long lists                                    |
