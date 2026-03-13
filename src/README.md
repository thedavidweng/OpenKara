# src — React Frontend

Karaoke UI built with React + TypeScript + Vite.

| Directory     | Responsibility                                     |
| ------------- | -------------------------------------------------- |
| `components/` | React components organized by feature              |
| `hooks/`      | Custom React hooks (audio state, lyrics sync, etc) |
| `stores/`     | State management (player state, library, queue)    |
| `types/`      | Shared TypeScript type definitions                 |
| `styles/`     | Global CSS / style tokens                          |

## Component Structure

- **Player/** — Karaoke player: playback controls, progress bar, volume
- **Library/** — Song library: import, browse, search
- **Lyrics/** — Synced lyrics display with line-by-line highlighting
