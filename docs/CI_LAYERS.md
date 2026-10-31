# CI layers

OpenKara uses four gate layers. Each layer has a different goal. Do not run
the full Nightly matrix on every PR or every main push.

## Layer goals

| Layer     | Goal                               | Wall-clock target |
| --------- | ---------------------------------- | ----------------- |
| PR        | Fast feedback on the change set    | 10–20 minutes     |
| Main push | CI Gate only                       | Under 1 minute    |
| Nightly   | Full release quality matrix        | 1–2 hours         |
| Release   | Signed product + Windows #284 core | 30–45 minutes     |

## PR

Path-aware via `scripts/ci/classify-changes.mjs`.

Default PRs run only the fast job set:

- commit title and workflow lint (`conventional-commits`, `workflow-lint`)
- format, lint, i18n, knip, and schema drift (`js-quality`)
- frontend build, typecheck, and unit tests (`app-frontend`, `website`)
- cached Linux Rust format, Clippy, and integration tests (`rust-test`, `cargo-deny`, `dependency-checks`)
- standards and release contract checks (`standards-reference`, `release-validation`)

Heavy jobs do not run on default PRs. Add the `ci:full` label to a PR to run
the path-matched heavy jobs before merge:

- Tauri build matrix (`tauri-build`) and Linux smoke build (`tauri-build-smoke`)
- Playwright UI smoke (`playwright-ui-smoke`)
- macOS Rust tests (`rust-test-macos`) and Windows Rust compile (`rust-test-windows-compile`)
- Flatpak and WinGet packaging (`packaging.yml`)
- Windows installed-app gate (`windows-installed-app-smoke-test.yml`)

`rust-test-windows-compile` also carries the live Source Registry consumer
gate. The owner arms the gate with the `OPENKARA_LIVE_REGISTRY_URL` repo
variable after the real registry package is published. While the variable is
unset the gate step does not run. The test itself never skips: when the step
runs, a missing or empty URL fails the job.

## Main push

Main push runs CI Gate only: the gate accepts skipped verification jobs.
PR checks run on the merge ref against the latest `main`, so a conflict
between two merged PRs fails the next PR's CI. Manual `workflow_dispatch`
still forces full CI.

A red `main` push or `schedule` run opens or comments on the open
`CI failure on main/nightly` tracking issue (`bug` label) with links to the
failed workflow run and failed jobs. PR failures and green runs never open or
update tracking issues.

## Nightly

Full quality line. It does not block PR merge. It blocks Release when the
candidate commit has no green Nightly evidence from the last 24 hours.

- four-arch separation (Linux x64/arm64, macOS AS/Intel)
- three-platform installed-app
- Windows keyboard UIA + display scaling 100/125/150/200%
- full fault injection on disk
- full Playwright e2e / accessibility matrix (Chromium + WebKit)

A separate nightly-installers workflow builds unsigned installers from main
(skipping when main has not moved) and publishes them as a rolling `nightly`
prerelease for pre-release testing. It never ships updater assets
(latest.json / *.sig), so production auto-update ignores it.

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

- `scripts/ci/classify-changes.mjs` — PR path gates (main push runs CI Gate only)
- `.github/workflows/ci.yml` — PR, main, schedule, and dispatch
- `.github/workflows/packaging.yml` — WinGet and Flatpak packaging gate
- `.github/workflows/windows-installed-app-smoke-test.yml` — branch Windows installed-app gate
- `.github/workflows/nightly-hardening.yml` — full matrix
- `.github/workflows/nightly-installers.yml` — rolling nightly prerelease
- `.github/workflows/release.yml` — product + Windows #284 core
- `.github/workflows/reusable-windows-installed-app.yml` — Windows lifecycle
