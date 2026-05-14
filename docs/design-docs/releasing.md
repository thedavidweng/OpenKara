# Releasing OpenKara

## Release Flow

1. **Trigger**: Go to GitHub Actions → **Release** workflow → **Run workflow**
2. **Input**: Enter the version number (e.g. `0.8.1`) — do not include the `v` prefix
3. **Build**: CI builds for all 4 platforms (macOS ARM64, macOS x64, Windows, Linux)
4. **Publish**: GitHub Release is created automatically with DMG, NSIS installer, and AppImage
5. **Homebrew**: The tap repo (`thedavidweng/homebrew-tap`) polls for new releases once per day and updates the cask when it detects a new version. Expect up to about 24 hours before the scheduled sync picks it up.

## Manual Homebrew Update

If you don't want to wait for the scheduled check:

1. Go to `thedavidweng/homebrew-tap` → Actions → **Sync Releases**
2. Click **Run workflow**

## Verification

```bash
brew update
brew install --cask thedavidweng/tap/openkara
```

## Architecture

OpenKara's release workflow is **decoupled from distribution**:

- **OpenKara repo** — only builds and publishes GitHub Releases
- **Homebrew tap repo** — independently polls daily for new releases and updates the cask
- No cross-repo secrets needed; each repo manages its own automation

## Code signing and paid credentials

Default **GitHub Actions release** builds **do not** apply Apple codesigning + notarization or Windows Authenticode unless repository secrets and certificates are configured.

### macOS (blocked on Apple Developer Program)

- **Codesigning + notarization** require [Apple Developer Program](https://developer.apple.com/programs/) membership (paid). Without it, maintainers ship **unsigned** (or ad-hoc) artifacts suitable for contributors and testers who accept **Gatekeeper** friction (e.g. Control-click the app → **Open**, or follow current Apple documentation for reducing quarantine prompts).
- **Do not** claim “notarized” or “App Store–safe” distribution in release notes until those credentials exist.
- **Work that does not require the paid program:** document the exact artifact layout, honest user-facing install steps, and checksum verification (see below).

### Windows (Authenticode optional, cert usually paid)

- **OV/EV code-signing certificates** are typically a recurring purchase. NSIS installers from CI may trigger **Microsoft SmartScreen** “Unknown publisher” until reputation improves or signing secrets are added to the workflow.
- **Work without a cert:** document SmartScreen expectations; when the org obtains a certificate, extend `.github/workflows/release.yml` and list required secrets here.

### Download integrity without OS trust stores (H8.1)

- Prefer attaching **SHA256** digests for each release asset (e.g. workflow-generated `SHA256SUMS` or a table in the GitHub Release body) so users can verify downloads even when binaries are unsigned.

### In-app updates

- **Today:** updates are **manual** (GitHub Releases, Homebrew, WinGet, Flatpak, etc.). There is **no** in-app Tauri updater wired in this repository.
- **If added later:** document pubkey pinning, staging vs production endpoints, and update [`../references/contracts/`](../references/contracts/) together with IPC changes (`AGENTS.md`).

## Upgrade, migration, and data safety (H8.2)

### Migration policy

- **Forward-only:** SQLite migrations are additive only. Once a migration version has been applied in the wild, it must not be modified. New migrations always append a new numbered file.
- **Downgrade:** Currently **unsupported**. If a user needs to revert to an older app version, they must restore from a library backup (see below). The app checks the schema version on startup and will refuse to open a database with a higher version than it understands.
- **Schema version:** Tracked via `library_meta` key `schema_version` (integer). The app applies pending migrations in numeric order on library open.
- **Failure:** If a migration fails (e.g. disk full, constraint violation), the transaction is rolled back. The library remains at the previous schema version. A user-visible error toast describes the failure and recommends backup/restore.

### Library backup

Before any major version upgrade, users are advised to:

1. Locate the library folder (shown in Settings → Karaoke Library).
2. Copy the entire folder to a safe location.
3. If using a remote repository, ensure the local working copy is in sync before upgrading.

A future version may add a one-click export; for now the manual copy is the supported path.

## Supply chain checks (H8.4)

| Check         | Status                                                                                                                                          | Waivers / issues                                                                                                    |
| ------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `pnpm audit`  | ✅ Green (CI runs per commit, `--audit-level=high`). Run log: [CI workflow](.github/workflows/ci.yml)                                           | None                                                                                                                |
| `cargo audit` | ✅ Last scan (2026-05-13): 0 vulnerabilities, 18 allowed warnings (all Tauri/Linux-native deps with no runtime impact). Tool v0.22.1 installed. | Allowed warnings documented in [`../../AGENTS.md`](../../AGENTS.md) (Linux audio, WebKit, GTK — platform‑expected). |
| `cargo deny`  | ❌ Not installed. Could be added as a scheduled weekly check (`cargo install cargo-deny` + config) — deferred as resource-constrained.          | —                                                                                                                   |
| SBOM          | ❌ Not generated. GitHub Dependency Graph provides lockfile-level information. CycloneDX generation deferred.                                   | —                                                                                                                   |

## Diagnostics & supportability (H8.5)

### Log file location

- **macOS:** `~/Library/Logs/OpenKara/`
- **Windows:** `%APPDATA%\OpenKara\logs\`
- **Linux:** `~/.local/share/OpenKara/logs/`

### Size cap & rotation

Log files are rotated at 10 MB per file; up to 3 rotated files are retained. The app writes to `openkara.log` and renames on rotation.

### Redaction

Before writing, the app redacts:

- OAuth tokens and `authorization` header values → `<REDACTED_TOKEN>`
- File paths containing the current user's home directory → `<REDACTED_HOME>`
- Remote repository URLs that contain embedded credentials (`user:pass@`) → `<REDACTED_CREDENTIALS>`

### Version / About

The About dialog (`windowChrome.about`) displays:

- App version from `package.json` / `Cargo.toml`
- Build identifier: git short SHA (when available from CI or `git rev-parse --short HEAD` at build time)
- Platform and architecture

### Diagnostics export

Use the "Copy debug info" option in the Help menu (or equivalent surface) to copy a minimal diagnostic string: app version, platform, build SHA. **No secrets** are included — the export runs the same redaction pass as logs.

## Pre-release checklist (H8)

Before running the **Release** workflow for a version tag, use this list in addition to **[`docs/plan/plan.md`](../plan/plan.md)** stream **H8**:

- [ ] `pnpm format` and the `AGENTS.md` verification matrix for the **highest-risk area** touched since the last release.
- [ ] `pnpm lint` → `pnpm build` → `pnpm test` → `cd src-tauri && cargo test -q` when frontend or Rust changed.
- [ ] Manual smoke on **at least two OS tiers** when media, packaging, separation, or IPC changed.
- [ ] Changelog / GitHub Release notes: version, highlights, and **known limitations** (unsigned macOS/Windows until signing credentials exist).
- [ ] Confirm release assets match the workflow matrix (e.g. DMG variants, NSIS, Linux bundles).
- [ ] If the version bumps packaging metadata: WinGet/Flatpak generator sanity (`pnpm test` coverage for packaging or `.github/workflows/packaging.yml`).
- [ ] `pnpm audit` and `cargo audit` scans are recorded (CI logs or this file §H8.4).
- [ ] i18n: at least **en** and **zh-CN** checked for missing keys (run `node scripts/check-i18n.mjs` if available, or confirm by manual diff).
- [ ] Release assets include a `SHA256SUMS` file (generated by CI after the build step, see H8.1).

This is the **shippable bar without paid Apple signing**; completing H8 in the active plan adds diagnostics, migration safety, supply-chain checks, and i18n smoke beyond this short list.

## Automated Distribution Manifests

GitHub Releases remain the only place where OpenKara binaries are built.

Additional distribution channels are derived from those releases:

- **WinGet** — release automation renders versioned manifests and, when
  `WINGET_FORK_REPO` and `WINGET_PR_TOKEN` are configured, opens or updates a PR
  against `microsoft/winget-pkgs`. If the token can push the fork branch but
  cannot create the upstream PR, the workflow now prints a compare URL and
  keeps the release run green. WinGet PR titles use the
  `New version: <PackageIdentifier> version <Version>` convention.
- **Flathub** — release automation renders the source-build Flatpak bundle and,
  when `FLATHUB_FORK_REPO` and `FLATHUB_PR_TOKEN` are configured, prepares the
  correct Flathub branch. Because OpenKara is not published on Flathub yet, the
  workflow targets `flathub/flathub:new-pr`, pushes an initial-submission
  branch to the fork, and prints a GitHub web compare URL with the official
  Flathub submission notes prefilled. The submission PR itself must still be
  reviewed and opened manually. After Flathub creates
  `flathub/io.github.thedavidweng.OpenKara`, set `FLATHUB_TARGET_REPO` to that
  app repo and `FLATHUB_BASE_BRANCH` to `master` so release automation can open
  update PRs.

Repo-local source of truth:

- `packaging/winget/`
- `packaging/flatpak/`

Repo-local validation:

- `.github/workflows/packaging.yml` validates that the manifest generators still
  produce syntactically correct WinGet and Flatpak metadata from the latest
  public release.

## Manual smoke log — Remote Libraries (H2)

Before tagging a release that changes remote library code (OAuth flows, WebDAV, sync),
record a manual smoke pass here.

| Provider     | Platform | Date       | Result | Notes                                                 |
| ------------ | -------- | ---------- | ------ | ----------------------------------------------------- |
| Google Drive | macOS    | 2026-05-13 | ✅     | Connect, browse, play, disconnect                     |
| Dropbox      | macOS    | 2026-05-13 | ✅     | Connect, browse, play, disconnect                     |
| WebDAV       | macOS    | 2026-05-13 | ✅     | Connect, browse, play, disconnect (local test server) |

**Known gaps:** Linux WebDAV, Windows Drive/Dropbox — pending maintainer access.
Results will be appended when available.

> ⚠️ This table is **manual / credentialed** per H2 acceptance criteria. If any
> provider cannot be tested before a release, mark the row **Deferred** and note
> the earliest next date for validation.

## Future Distribution Channels

### Windows

- **winget**: Automated via release workflow once the external fork/token
  bootstrap is configured.
- **Scoop**: Create `thedavidweng/scoop-bucket` with the same self-polling pattern as the Homebrew tap. Simpler than winget.

### Linux

- **Flatpak**: Source-build Flathub-ready manifest is maintained in-repo and can
  be prepared automatically after bootstrap. The first Flathub submission PR is
  still opened manually against `flathub/flathub:new-pr`; final Flatpak
  binaries are built by Flathub, not by GitHub Actions.
- **AUR**: Write a PKGBUILD. Community can help maintain. Deferred.
- **Snap**: Write a snapcraft.yaml. Deferred.
