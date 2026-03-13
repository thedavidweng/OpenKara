# Project Structure

```
OpenKara/
├── docs/                   # Project documentation
│   ├── architecture.md     # System architecture & tech stack
│   └── project-structure.md# This file
│
├── src-tauri/              # Rust backend (Tauri)
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs         # Tauri app entry point
│   │   ├── audio/          # Audio decode & playback
│   │   ├── separator/      # AI stem separation (ONNX)
│   │   ├── lyrics/         # Lyrics fetching & parsing
│   │   ├── metadata/       # Audio file tag reading
│   │   ├── cache/          # Cache management (SQLite + fs)
│   │   └── commands/       # Tauri IPC command handlers
│   ├── models/             # ONNX model files (git-ignored)
│   └── migrations/         # SQLite schema migrations
│
├── src/                    # React frontend
│   ├── main.tsx            # App entry point
│   ├── App.tsx
│   ├── components/         # React components
│   │   ├── Player/         # Karaoke player & controls
│   │   ├── Library/        # Song library & import
│   │   └── Lyrics/         # Synced lyrics display
│   ├── hooks/              # Custom React hooks
│   ├── stores/             # State management
│   ├── types/              # TypeScript type definitions
│   └── styles/             # Global styles
│
├── public/                 # Static assets
├── README.md
├── README_CN.md
├── package.json
├── tsconfig.json
├── vite.config.ts
└── .gitignore
```

## Directory Responsibilities

### `src-tauri/` — Rust Backend

All heavy lifting happens here: audio decoding, AI inference, lyrics fetching, and caching. The frontend communicates with this layer through Tauri's IPC command system.

### `src/` — React Frontend

The UI layer. Renders the karaoke experience: song library, lyrics display with synchronized highlighting, and playback controls. Stays thin — no audio processing or AI logic.

### `docs/` — Documentation

Architecture decisions, project structure, and development guides. Kept in the repo so documentation evolves with the code.

### `src-tauri/models/` — AI Models

ONNX model files for Demucs v4. These are large binary files (~80 MB) and are **not checked into git**. A setup script downloads them on first build.
