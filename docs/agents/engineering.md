# Agent engineering detail

Load this file when you need the expanded engineering, documentation, or
language rules. Hard rules that always apply live in root `AGENTS.md`.

## Source of truth

The codebase is the authority for behavior, types, and contracts. `docs/` holds
historical context, design rationale, and human guides. Do not read docs to
discover current runtime behavior.

## Comments and rationale

Prefer names, types, and structure over comments. Put product tradeoffs and
design rationale in the commit message, or in an ADR under `docs/adr/` when the
decision needs a durable record. Keep comments rare: non-obvious algorithm
steps, or third-party bug workarounds. Delete noise when you touch a file. Do
not use code comments as documentation. Do not write docs as agent
instructions.

## Documentation

- `CHANGELOG.md` is managed by release-please from Conventional Commits. Do not
  hand-write entries.
- Backlog and prioritization live in the GitHub Project:
  https://github.com/users/thedavidweng/projects/2/views/1. Do not mirror
  backlog tables in `docs/`.
- IPC contracts live under `docs/references/contracts/`. Update the matching
  file in the same change as a public IPC command, payload, event, or
  source-enum change.
- Generated docs must name their generator and output path in
  `scripts/README.md`.
- Treat repo-tracked docs and configs as first-class code. Keep code,
  contracts, and docs aligned when behavior changes.

## Technical English (ASD-STE100)

Write technical documentation in ASD-STE100 Simplified English by intent, not
by dictionary lookup. Use short sentences. Use active voice. Use one topic per
sentence. Use one word for one meaning. Avoid synonyms, passive voice, `-ing`
verbs as main verbs, noun clusters, and vague quantifiers.

Scope: all repo-tracked English prose except `README.md`, `README_CN.md`,
`website/`, `CHANGELOG.md`, `docs/references/generated/`, and `.github/`
templates.

Also see `docs/references/standards/language-terminology-and-data.md` and
`docs/adr/README.md`.

## Deliverables

Write deliverables as self-contained final-state artifacts. Incorporate
feedback directly. Do not mention drafts, versions, review rounds, prior
wording, superseded decisions, or the editing process unless the user asks for
a changelog, history, or decision record.

## Formatting hotspots

Formatting runs via hook. If the hook fails, fix the issue before you continue.
Easy-to-miss paths: `pnpm-lock.yaml`, `website/**/*.html`, `website/**/*.css`,
`src-tauri/tauri.conf.json`, `.github/workflows/*.yml`, and any Markdown, JSON,
HTML, CSS, or YAML you touch.

## Project skills

- `.agents/skills/verify/SKILL.md` — verification commands by change type
- `.agents/skills/delivery/SKILL.md` — PR description and delivery report
- `.agents/skills/ci-notes/SKILL.md` — CI environment, CodeQL, Cursor Cloud
