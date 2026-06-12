# Current Implementation Status

> **Archived:** 2026-06-11. This snapshot is superseded by
> [`../../CHANGELOG.md`](../../CHANGELOG.md) for shipped history and the
> [GitHub Project](https://github.com/users/thedavidweng/projects/2/views/1)
> for future work.

> **Last updated:** 2026-05-14 · This file tracks the implementation status and is updated alongside releases.
> **Current source version:** 0.9.0
> **Released source of truth:** `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` must match the version described here.
> **For a concise version history, see [CHANGELOG.md](../../CHANGELOG.md).**

## v0.9.0 — Source Complete (Pending Tag)

The v0.9.0 cycle is complete in source (merged via PR #20, commit `0eb8808`). The next tagged release will ship these items.

**Deferred from v0.9** (tracked in [GitHub Projects](https://github.com/users/thedavidweng/projects/2)):

- `cargo deny` setup (scheduled weekly check)
- CycloneDX SBOM generation
- H2 WebDAV smoke on Windows/Linux (needs maintainer access)
- Windows DirectML validation in CI (needs GPU runner)
- macOS codesign + notarization (requires Apple Developer Program)
- Windows Authenticode (requires paid code-signing cert)

## Roadmap

The roadmap is tracked in [GitHub Projects](https://github.com/users/thedavidweng/projects/2). Repo-local execution plans live in [`../plans/`](../plans/) only when they are ready to implement.
