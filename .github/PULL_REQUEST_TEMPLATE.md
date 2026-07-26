## Summary

Brief description of the changes.

## Changes

-

## Test plan

Report the commands you ran, and say which you skipped and why.

- [ ] `node --run lint` and `node --run build`
- [ ] `pnpm vitest run` _(frontend changes)_
- [ ] `cd src-tauri && cargo clippy --all-targets -- -D warnings` _(Rust changes)_
- [ ] `cd src-tauri && cargo nextest run` _(Rust changes)_
- [ ] `node --run check:i18n` _(new or changed UI copy)_
- [ ] `docs/references/contracts/*.md` updated _(public IPC command, payload, or event changed)_

## Related issues

Closes #
