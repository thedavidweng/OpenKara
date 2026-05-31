---
name: delivery
description: Format delivery reports and PR descriptions. Use when preparing a commit, push, PR, or handoff.
---

# Delivery Requirements

## Before committing or pushing

1. Scope: State what behavior or subsystem changed.
2. Verification: Report exact commands run and their results.
3. Gaps: If you ran only a subset, say exactly what you did **not** run.
4. Contracts: If behavior changed, confirm `docs/references/contracts/*.md` was updated.
5. Never skip local verification and leave first discovery of breakage to CI.

## Delivery report template

```
## What changed
[concise description]

## Verification
- [x] pnpm lint — PASS
- [x] pnpm test — PASS (X tests)
- [ ] pnpm tauri build — SKIPPED (docs-only change)

## Residual risk
[none / describe known limitations]
```

## Rules

- Do not revert, reformat, or reorganize unrelated dirty changes you did not make.
- Keep documentation/config/contract updates in the same change when implementation meaning moved.
