[简体中文](./README_CN.md)

# OpenKara

OpenKara is an open-source desktop application that turns a local music library into an instant karaoke experience.

It uses on-device AI to separate vocals and instrumentals, then fetches synced lyrics from the Internet so users can sing with the music they already own.

## Vision

Make karaoke affordable and open by letting users reuse the music they already own.

## Problem

Existing karaoke software usually has one or more of these limits:

- Limited song catalogs
- Expensive subscriptions
- Repurchase requirements for songs users already own

OpenKara solves this by combining local music playback, AI stem separation, and synced lyrics.

## Target Users

- Budget-conscious university students
- Dorm party organizers
- People with private local music collections (MP3, FLAC, and similar formats)

## Core Feature List

- Local Audio Import
- AI Stem Separation
- Synced Lyrics Fetch
- Karaoke UI
- Playback Controls
- Mic Input and Vocal Effects
- Playlist and Multi-user Queue
- Pitch and Key Shift
- Session Recording
- Multi-screen Support

## MVP Scope

- Local Audio Import
- AI Stem Separation
- Synced Lyrics Fetch
- Karaoke UI
- Playback Controls

## Why This MVP

OpenKara's core value is turning an existing music library into a karaoke library. Stem separation and lyric sync are the unique differentiators. Local import and playback controls are the minimum product foundation required to make the experience usable.

## Nice-to-Have Features

- Mic Input and Vocal Effects
- Playlist and Multi-user Queue
- Pitch and Key Shift
- Session Recording
- Multi-screen Support

These features are intentionally deferred to reduce product risk and validate the core experience first.

## Launch Strategy

### Soft Launch

Start with a small group of early users, such as university clubs, to validate:

- Hardware performance on real user devices
- Stem separation quality
- Lyric sync quality
- Baseline usability and stability

### Incremental Releases

After the soft launch, ship regular updates based on user feedback and usage signals.

### Distribution

- GitHub Releases
- Homebrew

### Marketing Channels

- Reddit communities
- Discord communities
- Facebook groups

### Early Feedback Loop

- User surveys
- Selected user interviews for qualitative feedback

## Post-Launch Strategy

### Customer Support

Because OpenKara is open source, support should be scalable:

- FAQ and troubleshooting knowledge base
- GitHub Issues for bug reports and feature requests

### Community Management

- Run an official Discord server for peer support and sharing
- Participate in relevant Reddit communities such as karaoke-related subreddits

### Continuous Improvement

- Monitor GitHub issues and community discussions
- Turn feedback from Discord and Reddit into roadmap priorities
- Prioritize improvements that increase retention and repeat usage

## Viability Reflection

Primary health signal:

- Churn and retention trend

If the product proves unviable, options include:

- Pivoting the separation pipeline toward musician practice tools
- Leaving the codebase healthy for community forks

## Documentation

- [Architecture](./docs/architecture.md) — System design, tech stack, data flow, and AI model details
- [Project Structure](./docs/project-structure.md) — Directory layout and module responsibilities
- [Development Phases](./docs/development-phases.md) — Executable phase checklist with verify steps
- [Technical Roadmap](./docs/roadmap.md) — Tech choices, API contracts, and risk mitigations
- [Milestones](./docs/milestones.md) — Milestone task table with exit criteria

## Phase 0 Setup

### Prerequisites

- Node.js 20+
- pnpm 10+
- Rust stable via `rustup`
- Tauri CLI v2 via `cargo install tauri-cli --version "^2"`

### Local Bootstrap

```bash
pnpm install
./scripts/setup.sh
pnpm tauri dev
```

`./scripts/setup.sh` is still the recommended local-dev prewarm step because it
places the Demucs model in `src-tauri/models/` for deterministic testing.
Starting from Phase 6, the desktop app also auto-downloads the model into the
app data directory on first launch when no verified local copy exists.

### Local Verification

```bash
pnpm lint
pnpm format
cd src-tauri && cargo test
cd ..
pnpm tauri build --debug --no-bundle --ci
```

## Current Status

Concept defined, MVP scoped, architecture documented, and the current branch now
includes import/playback/separation/lyrics backend foundations, structured
errors, backend performance baselines, and runtime model bootstrap for
first-launch installs.

## Contributing

Contributions are welcome. Open an issue before making major changes.

## Acknowledgments

- [monochrome](https://github.com/monochrome-music/monochrome) — Lyrics sync and LRCLIB integration approach
- [Demucs](https://github.com/facebookresearch/demucs) — AI stem separation model by Meta Research
- [LRCLIB](https://lrclib.net) — Open synced lyrics API

## License

TBD. MIT is a reasonable default if the goal is broad open-source adoption.
