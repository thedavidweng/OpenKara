---
name: verify
description: Run the right verification commands based on what changed. Use when completing a change, before committing, or when asked to verify.
---

# Verification

## How to decide what to run

Identify the **highest-risk area** touched, then run the matching row. If multiple areas changed, use the stricter set.

| Change type                      | Required commands                                                                               |
| -------------------------------- | ----------------------------------------------------------------------------------------------- |
| Docs/config only                 | (formatting handled by hook — skip)                                                             |
| Frontend/UI (`src/`)             | `pnpm lint` → `pnpm build` → `pnpm test`                                                        |
| Rust/backend (`src-tauri/`)      | `cd src-tauri && cargo test -q`                                                                 |
| Release-sensitive or cross-stack | `pnpm lint` → `pnpm build` → `pnpm test` → `cd src-tauri && cargo test -q` → `pnpm tauri build` |

## Always escalate to full verification for:

- Version changes
- `src-tauri/tauri.conf.json` edits
- Packaging or release workflow changes
- Cross-frontend/backend refactors
- Model bootstrap changes
- Playback, separation, lyrics-sync, or other core media-path changes

## After verification

Report:

1. What changed
2. What commands were run and their results
3. What commands were **not** run (and why)
4. Any known residual risk

## Contract check

If you changed public IPC commands, payloads, or events, verify the corresponding `docs/references/contracts/*.md` was updated in the same change.

## Standards check

For a product-surface change, read `docs/references/product-standards.md` and
then the matched profile. Run its automated evidence and state any required
manual evidence in the delivery report.
