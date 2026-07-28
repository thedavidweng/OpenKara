# OpenKara Agent Notes

Coding agent instructions. This file contains the global rules. Load linked
guidance only when the current change needs it.

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

- Prefer self-explanatory code. Names, types, and structure carry meaning — not essay comments.
- Do not put product tradeoffs, constraints, or design rationale in code comments. Put them in the git commit message, or in an ADR / human doc under `docs/` when the decision needs a durable record.
- Keep comments rare: only when the code cannot be made clear (e.g. a required non-obvious algorithm step, or a third-party bug workaround). Delete noise comments when you touch the file.
- Keep code, contracts, and docs aligned. If behavior changes, update the relevant `docs/references/contracts/*.md` in the same change.
- Treat repo-tracked docs and configs as first-class code.

## Deliverables

- Write deliverables as self-contained final-state artifacts. Incorporate feedback directly without mentioning drafts, versions, review rounds, prior wording, superseded decisions, or the editing process unless the user explicitly requests a changelog, history, or decision record.

## Source of Truth

**Read code, not docs.** The codebase is the authoritative reference for
behavior, types, and contracts. `docs/` exists for humans who need historical
context or design rationale — do not read `docs/` to understand what the code
does.

## Product Standards (load on demand)

Before you change a product surface, read
[`docs/references/product-standards.md`](docs/references/product-standards.md).
It routes the change to the applicable standard profile. Read only the matched
profiles. These profiles are repository constraints. Put their required
automated or manual evidence in the PR. Add an ADR before you introduce a new
product surface or change a standards target.

## Documentation Rules

- `CHANGELOG.md` is managed by release-please from Conventional Commits. Do not hand-write changelog entries. Write a clear Conventional Commit message (`feat:`, `fix:`, `refactor:`, etc.) and release-please will regenerate the file when it opens and merges the next release PR.
- Future backlog and prioritization live in the GitHub Project: https://github.com/users/thedavidweng/projects/2/views/1. Do not mirror backlog tables in `docs/`.
- IPC contracts live under `docs/references/contracts/`. Update the corresponding contract file in the same change that modifies a public IPC command, payload, event, or source-enum.
- Generated docs must name their generator and output path in `scripts/README.md`.

## Technical English (ASD-STE100)

Write technical documentation in ASD-STE100 Simplified English. Apply the standard by intent, not by dictionary lookup. Use short sentences. Use active voice. Use one topic per sentence. Use one word for one meaning. Avoid synonyms, passive voice, `-ing` verbs, noun clusters, and vague quantifiers.

Scope: all repo-tracked English prose except `README.md`, `README_CN.md`, `website/`, `CHANGELOG.md`, `docs/references/generated/`, and `.github/` templates. Do not use code comments as documentation. Make code self-explanatory; write rationale in commits or ADRs.

## Never Do

- Never use `as any`, `@ts-ignore`, or `@ts-expect-error` to silence type errors. Fix the types instead.
- Never change public IPC commands, payloads, or events without updating the corresponding contract docs.
- Never leave completed plans in the repo; delete them. The outcome will appear in `CHANGELOG.md` via the commit message when release-please next creates a release PR.
- Never add long rationale or product-decision comments in source. Prefer a clear commit message.
- Never remove Linux CI package `libasound2-dev` unless the audio stack itself changes.
- Never repurpose `src-tauri/models/` as the production runtime model location.
- Never write docs as agent instructions. Docs are for humans; make code self-explanatory instead.

## Skills (load on demand)

Project-specific skills in `.agents/skills/`. Read the relevant file when you need detailed guidance:

- `.agents/skills/verify/SKILL.md` — which verification commands to run for each change type
- `.agents/skills/delivery/SKILL.md` — PR description format and delivery report requirements
- `.agents/skills/ci-notes/SKILL.md` — CI environment constraints, CodeQL patterns, Cursor Cloud notes
