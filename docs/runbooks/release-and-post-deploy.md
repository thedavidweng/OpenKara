# Runbook: Release Verification and Rollback

Use this playbook to verify deployment impact after a release cut, website deploy, or model catalog update, and to roll back a faulty release.

## Post-Deploy Verification

OpenKara is a local-first desktop application without third-party runtime telemetry. Verify deployment health directly across its distribution endpoints.

### 1. Desktop Release and Auto-Updater Feed

Check the published release assets, checksums, and `tauri-plugin-updater` manifest:

```bash
gh release view --json tagName,isDraft,isPrerelease,publishedAt,assets
curl -fsSL https://github.com/thedavidweng/OpenKara/releases/latest/download/latest.json | jq '{version, pub_date, platforms: (.platforms | keys)}'
curl -fsSL https://github.com/thedavidweng/OpenKara/releases/latest/download/SHA256SUMS
```

Confirm these invariants:

- `isDraft` is `false` and `isPrerelease` is `false` for stable tags.
- `latest.json` reports the expected version and carries non-empty signatures for macOS, Windows, and Linux AppImage targets.
- `release-evidence.json` is attached to the release and matches the published tag commit.
- No open issue exists with the title `Release cut failed for vX.Y.Z`.

### 2. Website Deployment (`github-pages`)

Check the `Pages` workflow and active GitHub Pages deployment after changes to `website/`:

```bash
gh run list --workflow pages.yml --limit 5
gh api repos/thedavidweng/OpenKara/deployments?environment=github-pages --jq '.[0] | {id, sha, created_at}'
```

### 3. Downstream Package Channels

After `release.yml` finishes, verify downstream distribution pull requests:

- Homebrew tap (`thedavidweng/tap`): confirm the `openkara` cask version and SHA-256 values updated.
- WinGet (`microsoft/winget-pkgs`) and Flathub: confirm automated pull requests opened with stable `/releases/download/<tag>/<asset>` URLs and passed bot validation.

## Rollback Procedures

### Stop Auto-Updates to a Faulty Desktop Release

`tauri-plugin-updater` polls `/releases/latest/download/latest.json`. GitHub resolves `/releases/latest` to the newest published release that is neither a draft nor a prerelease.

1. Mark the faulty GitHub Release as a prerelease immediately so `/releases/latest` reverts to the previous healthy release:
   ```bash
   gh release edit vX.Y.Z --prerelease
   ```
2. Confirm `/releases/latest` now serves the previous version:
   ```bash
   curl -fsSL https://github.com/thedavidweng/OpenKara/releases/latest/download/latest.json | jq -r .version
   ```
3. Fix the defect on `main` and cut a new patch release (`vX.Y.(Z+1)`) so users who already installed `vX.Y.Z` receive a signed forward update. Never rotate `TAURI_SIGNING_PRIVATE_KEY` during a rollback, because key rotation orphans existing installations.

### Roll Back a Separation Model or ONNX Runtime Catalog Pin

If a catalog bump regresses stem separation or execution-provider loading:

1. Revert `src-tauri/catalog/stable-pointer.json` and `src-tauri/catalog/release-manifest.json` to the previous verified release ID.
2. Validate both resolver consumers locally:
   ```bash
   node scripts/resolve-model.mjs --field file_sha256
   node scripts/prepare-onnx-runtime.mjs --dry-run
   pnpm render:flatpak
   ```
3. Run `cd src-tauri && cargo test -q` and merge the revert pull request.

See [docs/RELEASING.md](../RELEASING.md) for the full release pipeline and native manifest sync rules.
