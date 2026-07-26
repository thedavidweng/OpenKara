# Releasing

OpenKara releases are driven by a `v*` git tag. The
[`Release` workflow](../.github/workflows/release.yml) builds every platform
bundle, opens a **draft** GitHub Release, attaches `SHA256SUMS`, and renders the
WinGet and Flatpak manifests.

The published bundle takes its version from `package.json` (see
[`scripts/sync-version.mjs`](../scripts/sync-version.mjs)), while the tag drives
asset naming and the distribution manifests. **These two must agree.** If they
drift, the build produces assets named `OpenKara_<package.json version>_*` while
the WinGet/Flatpak manifests look for `OpenKara_<tag version>_*`, so the release
ships misnamed assets and the manifest jobs fail. The `prepare-release` job
fails fast with this exact guidance when they disagree.

## Cut a release

1. **Bump `package.json`** to the new version (e.g. `1.0.0`).
2. **Sync the native manifests:** run `pnpm version:sync`. This propagates the
   version into `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and
   `src-tauri/tauri.conf.json`.
3. **Commit** the version bump (`git commit -am "chore(release): 1.0.0"`) and
   merge it to `main`.
4. **Tag that commit** with a leading `v` and push the tag:
   ```bash
   git tag v1.0.0
   git push origin v1.0.0
   ```
   The tag version (minus the `v`) must match `package.json`.
5. **Watch the draft release.** The workflow runs the separation smoke on every
   target, builds the bundles, and opens a draft GitHub Release.
6. **Verify the asset names** on the draft release match the tag, e.g.
   `OpenKara_1.0.0_x64-setup.exe`. Confirm `SHA256SUMS` is attached.
7. **Publish** the draft release from the GitHub UI. Publishing (or the
   workflow, when configured) drives the Homebrew tap, WinGet, and Flathub
   submissions.

## If the version gate fails

The `prepare-release` job errors when the tag and `package.json` disagree. To
recover: bump `package.json` to the tag version, run `pnpm version:sync`,
commit, then delete and re-create the tag on the corrected commit:

```bash
git tag -d v1.0.0
git push origin :refs/tags/v1.0.0
# ...bump, sync, commit, merge to main...
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
