# Releasing

OpenKara releases are managed by [release-please]. The
[`Release Please` workflow](../.github/workflows/release-please.yml) runs on
every push to `main` and maintains a running release PR that bumps
`package.json`, updates `CHANGELOG.md`, and bumps
`.release-please-manifest.json`.

When the release PR is merged, release-please tags `vX.Y.Z` and opens a
**draft** GitHub Release with changelog notes. The
[`Release` workflow](../.github/workflows/release.yml) then builds every
platform bundle, runs the release-only separation and installed-app smoke
tests, attaches `SHA256SUMS`, and uploads assets to the draft release.

The release stays a draft until a human verifies the asset names and clicks
**Publish** in the GitHub UI. This last manual step is intentional: the
in-app updater polls `/releases/latest`, so a published release immediately
starts auto-updating existing installs.

[release-please]: https://github.com/googleapis/release-please

## Cut a release

1. **Merge the release PR.** release-please opens and updates the PR
   automatically as conventional commits land on `main`. Merge it when you
   are ready to ship. release-please bumps `package.json` and
   `CHANGELOG.md`, tags `vX.Y.Z`, and creates a draft GitHub Release with
   changelog notes.
2. **Watch the Release workflow.** It runs the separation smoke on every
   target, builds the bundles, and uploads assets to the draft release.
3. **Verify the asset names** on the draft release match the tag, e.g.
   `OpenKara_0.9.2_x64-setup.exe`. Confirm `SHA256SUMS` is attached.
4. **Publish** the draft release from the GitHub UI. Publishing (or the
   workflow, when configured) drives the Homebrew tap, WinGet, and Flathub
   submissions.

## Native manifest sync

release-please bumps `package.json` and `CHANGELOG.md` in its release PR.
The native manifests (`src-tauri/Cargo.toml`, `Cargo.lock`,
`tauri.conf.json`) are propagated by
[`scripts/sync-version.mjs`](../scripts/sync-version.mjs), which the
`Release Please` workflow runs in a follow-up job (`sync-native-versions`)
right after the release is created. `Cargo.lock` cannot use release-please
`extra-files` because a literal version-string replacement would corrupt
unrelated packages that happen to share the same version (e.g.
`memoffset 0.9.1`).

The `Release` workflow's build step also runs `pnpm version:sync` (via
`pnpm tauri` → `pnpm version:sync && tauri`) before building, so the
artifacts always carry the correct version even if the tag commit predates
the sync-native-versions push.

## If the version gate fails

The `prepare-release` job errors when the tag and `package.json` disagree.
This should not happen with release-please (it bumps `package.json` and
tags in the same merge), but if a manual `workflow_dispatch` is used with a
mismatched version:

```bash
# Bump package.json to the tag version, sync, commit, merge to main.
# Then delete and re-create the tag on the corrected commit:
git tag -d v1.0.0
git push origin :refs/tags/v1.0.0
git tag v1.0.0
git push origin v1.0.0
```

## In-app updater and signing

DMG/NSIS/AppImage installs update themselves through the first-party
`tauri-plugin-updater` (issue #255). The app polls
`https://github.com/thedavidweng/OpenKara/releases/latest/download/latest.json`
on launch and installs **only** payloads signed by the minisign key pair whose
public half lives in `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.
The `.deb` and Flatpak paths are not updatable through the plugin and stay on
the manual/package-manager channel. The plugin's own `check()` has no
install-format guard — on a `.deb` it would fall back to offering the AppImage
payload and only fail at install time — so the banner is gated on a
`self_update_supported` command that reports `true` only for the AppImage,
`.app`/DMG, and NSIS bundles. On a `.deb`, Flatpak, or dev build it returns
`false` and the in-app banner simply never appears.

The release build signs the updater artifacts (`*.sig` files and `latest.json`)
using two repository secrets, referenced by name in
[`release.yml`](../.github/workflows/release.yml):

- `TAURI_SIGNING_PRIVATE_KEY` — the minisign private key matching the pubkey.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — its password (empty string if none).

Generate a pair with `pnpm tauri signer generate` if one is ever lost. **Key
custody is load-bearing:** rotating the key orphans every existing install
(they reject updates signed by the new key), so back the private key up and
prefer recovering the original over generating a new one.

### Why a separate release config overlay

`bundle.createUpdaterArtifacts` is deliberately **not** set in
`src-tauri/tauri.conf.json`. OpenKara's verification contract runs a full
`pnpm tauri build` on every platform, and PR CI builds must stay keyless — a
base config that demanded a signing key would break both. Instead,
`src-tauri/tauri.release.conf.json` carries the single
`createUpdaterArtifacts: true` flag and the release workflow layers it on with
`--config src-tauri/tauri.release.conf.json`. Only the signed release build
emits updater artifacts; every keyless build (local `pnpm tauri build`, PR CI)
is unaffected.

`tauri-action` uploads `latest.json` itself (`uploadUpdaterJson` defaults to
true) and, across the build matrix, merges each platform's signatures into the
one manifest on the release — so no manual upload step is needed or wanted (a
manual clobbering upload would strip the other platforms' entries).

## Prerelease semantics and the updater

GitHub's `/releases/latest` — the URL the in-app updater polls — resolves only
the newest release that is neither a draft nor a prerelease. The workflow
therefore derives the prerelease flag from the tag: suffixed tags (`v1.0.0-rc.1`,
`v1.0.0-beta.1`) publish as prereleases the updater ignores; plain tags
(`v1.0.0`) publish as full releases the updater picks up **once the draft is
published**. Existing installs only start auto-updating after the first plain
tag ships as a published, non-prerelease release.
