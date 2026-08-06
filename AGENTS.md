# OpenKara Agent Notes

Global rules only. Load linked files when the task needs them. Prefer short
hard rules here over long prose.

## Project

Cross-platform desktop karaoke: Tauri 2 + React 19 + TypeScript + Zustand.
`src-tauri/models/` is a local dev/test cache only. End-user models and
runtimes live in the app data directory.

## Hard rules

- **Code is the source of truth** for behavior, types, and contracts. Do not
  read `docs/` to learn what the code does.
- **Self-explanatory code.** Rare comments only for non-obvious algorithm steps
  or third-party workarounds. Product rationale goes in the commit message or
  an ADR under `docs/adr/`. Delete noise comments when you touch a file.
- **No type escapes:** never `as any`, `@ts-ignore`, or `@ts-expect-error`.
- **IPC:** public command, payload, or event changes update
  `docs/references/contracts/` in the same change.
- **Product surfaces:** before changing one, read
  [`docs/references/product-standards.md`](docs/references/product-standards.md)
  and only the matched profiles. Put required evidence in the PR. Add an ADR
  for a new product surface or a changed standards target.
- **Formatting** is automatic (`pnpm format:write` + `cargo fmt`). Fix hook
  failures. Watch `pnpm-lock.yaml`, `website/**`, `tauri.conf.json`, and
  workflow YAML.
- **Changelog:** Conventional Commits only; do not hand-edit `CHANGELOG.md`.
- **No completed plans** left in the repo.
- **Never** remove Linux CI `libasound2-dev` unless the audio stack changes.
- **Never** treat `src-tauri/models/` as the production model location.
- **Deliverables** are final-state only unless the user asks for history.

## Load on demand

| Need                                              | Read                                                                           |
| ------------------------------------------------- | ------------------------------------------------------------------------------ |
| Domain terms                                      | [`CONTEXT.md`](CONTEXT.md)                                                     |
| Domain doc rules                                  | [`docs/agents/domain.md`](docs/agents/domain.md)                               |
| Issue tracker                                     | [`docs/agents/issue-tracker.md`](docs/agents/issue-tracker.md)                 |
| Engineering detail (STE100, docs, CI notes index) | [`docs/agents/engineering.md`](docs/agents/engineering.md)                     |
| Decisions                                         | [`docs/adr/`](docs/adr/)                                                       |
| Product standards                                 | [`docs/references/product-standards.md`](docs/references/product-standards.md) |
| Verify                                            | [`.agents/skills/verify/SKILL.md`](.agents/skills/verify/SKILL.md)             |
| Delivery / PR                                     | [`.agents/skills/delivery/SKILL.md`](.agents/skills/delivery/SKILL.md)         |
| CI / CodeQL                                       | [`.agents/skills/ci-notes/SKILL.md`](.agents/skills/ci-notes/SKILL.md)         |

## Agent skills

### Issue tracker

GitHub Issues for this repository. See `docs/agents/issue-tracker.md`.

### Domain docs

Single-context: root `CONTEXT.md` + `docs/adr/`. See `docs/agents/domain.md`.
