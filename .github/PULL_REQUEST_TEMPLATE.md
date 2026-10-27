## Summary

Brief description of the changes.

## Changes

-

## Acceptance criteria

Link to the issue's acceptance criteria, or list them here if this PR has no linked issue.

## Test plan

Report the commands you ran, and say which you skipped and why.

- [ ] `node --run lint` and `node --run build`
- [ ] `pnpm exec vp test run` _(frontend changes)_
- [ ] `cd src-tauri && cargo clippy --all-targets -- -D warnings` _(Rust changes)_
- [ ] `cd src-tauri && cargo nextest run` _(Rust changes)_
- [ ] `cargo build --manifest-path src-tauri/Cargo.toml --profile optimized` _(release-sensitive or cross-stack changes)_
- [ ] `node --run check:i18n` _(new or changed UI copy)_
- [ ] `docs/references/contracts/*.md` updated _(public IPC command, payload, or event changed)_
- [ ] Applicable product-standard profile reviewed; its evidence or exception is stated in this PR

## Related issues

Closes #
