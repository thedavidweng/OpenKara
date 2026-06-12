# Product Specs

This directory holds user-facing behavior specs rather than implementation notes.
Use these docs when the question is about what the product should do for a user, not how the code currently does it.

Executable engineering plans live in [`../../plans/`](../../plans/), not here.
Future ideas belong in the GitHub Project until they are ready to execute.

## Current Specs

| Document                                                                     | Description                                     |
| ---------------------------------------------------------------------------- | ----------------------------------------------- |
| [new-user-onboarding.md](./new-user-onboarding.md)                           | First-run setup and initial import expectations |
| [queue-management.md](./queue-management.md)                                 | Queue panel, up-next, reorder, auto-advance     |
| [F1-playlists-and-singer-rotation.md](./F1-playlists-and-singer-rotation.md) | Playlist CRUD, singer assignment, round-robin   |

## What Belongs Here

- First-run onboarding
- Library setup flows
- Playback and karaoke-mode behavior from the singer's perspective
- Settings behavior that changes user-visible outcomes
- Import and export behavior with user-facing error expectations
