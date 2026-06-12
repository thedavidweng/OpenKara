# OpenKara Agent Notes

Coding agent instructions. Self-contained — do not assume cross-referencing.

## Project

OpenKara: cross-platform desktop karaoke app. Tauri 2 (Rust + WebView) + React 19 + TypeScript + Zustand.

**Model path boundary:**

- `src-tauri/models/` is a local dev/test cache only.
- End-user installs use the app data directory for runtime model downloads.
- Do not treat `src-tauri/models/` as a required runtime dependency for shipped builds.

## Formatting

Formatting is automated via PostToolUse hook (`pnpm format:write` + `cargo fmt`). You do not need to run it manually. If the hook fails, fix the formatting issue before proceeding.

**Easy-to-miss hotspots:** `pnpm-lock.yaml`, `website/**/*.html`, `website/**/*.css`, `src-tauri/tauri.conf.json`, `.github/workflows/*.yml`, any Markdown/JSON/HTML/CSS/YAML touched in the change.

## Engineering Rules

- Preserve comments that explain **why** a piece of code exists, not just what it does.
- When touching product tradeoffs, portability rules, or storage/performance constraints, add or update a short rationale comment near the code.
- Keep code, contracts, and docs aligned. If behavior changes, update the relevant `docs/references/contracts/*.md` in the same change.
- Treat repo-tracked docs and configs as first-class code.

## Documentation Rules

- Completed user-visible or maintainer-visible changes go in `CHANGELOG.md`.
- Future backlog and prioritization live in the GitHub Project: https://github.com/users/thedavidweng/projects/2/views/1. Do not mirror backlog tables in `docs/`.
- Executable implementation plans live in `docs/plans/` only while they are actively driving work. Archive completed, superseded, or candidate-only plans under `docs/archive/`.
- Current architecture, release rules, product behavior, generated facts, testing guidance, and IPC contracts live under `docs/references/`.
- Public IPC command, payload, event, or source-enum changes must update the corresponding `docs/references/contracts/*.md` file in the same change.
- Generated docs must name their generator and output path in `scripts/README.md`.

## Never Do

- Never use `as any`, `@ts-ignore`, or `@ts-expect-error` to silence type errors. Fix the types instead.
- Never change public IPC commands, payloads, or events without updating the corresponding contract docs.
- Never leave completed work in `docs/plans/`; archive it and summarize the outcome in `CHANGELOG.md`.
- Never remove rationale comments just because the surrounding code was refactored.
- Never remove Linux CI package `libasound2-dev` unless the audio stack itself changes.
- Never repurpose `src-tauri/models/` as the production runtime model location.

## Skills (load on demand)

Project-specific skills in `.agents/skills/`. Read the relevant file when you need detailed guidance:

- `.agents/skills/verify/SKILL.md` — which verification commands to run for each change type
- `.agents/skills/delivery/SKILL.md` — PR description format and delivery report requirements
- `.agents/skills/ci-notes/SKILL.md` — CI environment constraints, CodeQL patterns, Cursor Cloud notes
