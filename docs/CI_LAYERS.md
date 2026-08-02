# CI layers

OpenKara uses four gate layers. Each layer has a different goal. Do not run
the full Nightly matrix on every PR or every main push.

## Layer goals

| Layer     | Goal                               | Wall-clock target |
| --------- | ---------------------------------- | ----------------- |
| PR        | Fast feedback on the change set    | 10–20 minutes     |
| Main push | Path-aware integration after merge | 10–15 minutes     |
| Nightly   | Full release quality matrix        | 1–2 hours         |
| Release   | Signed product + Windows #284 core | 30–45 minutes     |

## PR

Path-aware via `scripts/ci/classify-changes.mjs`.

Always:

- format, lint, typecheck (when frontend/tooling changes)
- unit tests for affected areas
- workflow contract tests when workflows change

Conditional:

- model / runtime / catalog change → Windows #284 fast path (branch or reusable smoke)
- UI / accessibility change → keyboard UIA + axe core
- installer / release workflow change → package build smoke
- separation pipeline change → Linux x64 real separation

Web accessibility on PR: Chromium, dark/light, core axe. Full page matrix,
WebKit, forced-colors, 400% zoom, and locale expansion stay on Nightly.

## Main push

Same path-aware classifier as PR. A docs-only merge does not rebuild every
platform. Manual `workflow_dispatch` still forces full CI.

## Nightly

Full quality line. It does not block PR merge. It blocks Release when the
candidate commit has no green Nightly evidence from the last 24 hours.

- four-arch separation (Linux x64/arm64, macOS AS/Intel)
- three-platform installed-app
- Windows keyboard UIA + display scaling 100/125/150/200%
- full fault injection on disk
- full Playwright e2e / accessibility matrix (Chromium + WebKit)

## Release

Product packaging and the Windows #284 contract. Release requires same-SHA
green Nightly evidence for:

- full accessibility matrix
- full DPI matrix
- four-arch separation (when catalog/runtime did not change)

Always on Release:

- signed multi-arch packages
- Windows clean install, upgrade, cold restart
- Windows #284 core: catalog identity, install load, cold rediscover, stale
  Downloading / stale manifest / corrupt file recovery, separation after heal
- macOS / Linux primary-arch install start + short lifecycle
- updater manifest, signatures, checksums

## Windows #284 core (must stay green)

- catalog generation, artifact ID, archive SHA, extracted-file SHA agree
- first install loads runtime/model for real
- cold start rediscovers installed assets
- stale Downloading, stale manifest, and on-disk corruption recover
- real separation succeeds after recovery

These concentrate on the Windows installed-app and fault-recovery path.

## Related files

- `scripts/ci/classify-changes.mjs` — PR / main path gates
- `.github/workflows/ci.yml` — PR and main
- `.github/workflows/nightly-hardening.yml` — full matrix
- `.github/workflows/release.yml` — product + Windows #284 core
- `.github/workflows/reusable-windows-installed-app.yml` — Windows lifecycle
