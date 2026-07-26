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
