# Product Specs

This directory holds user-facing behavior specs rather than implementation notes.
Use these docs when the question is about what the product should do for a user, not how the code currently does it.

## Current Specs

| Document                                           | Description                                     |
| -------------------------------------------------- | ----------------------------------------------- |
| [new-user-onboarding.md](./new-user-onboarding.md) | First-run setup and initial import expectations |

## Engineering plan (single active slice)

The repo keeps **one** execution plan under [`../plan/`](../plan/) (see [`../plan/next-execution-plan.md`](../plan/next-execution-plan.md)). Agreed priority-only history lives in [`../archive/plans/future-work-and-hardening-priorities-2026-05.md`](../archive/plans/future-work-and-hardening-priorities-2026-05.md).

## What Belongs Here

- First-run onboarding
- Library setup flows
- Playback and karaoke-mode behavior from the singer's perspective
- Settings behavior that changes user-visible outcomes
- Import and export behavior with user-facing error expectations
