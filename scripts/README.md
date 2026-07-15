# Scripts

## `generate-db-schema.mjs`

Regenerates `docs/references/generated/db-schema.md` from
`src-tauri/migrations/*.sql`.

- **Input:** none (reads migration files directly)
- **Output:** `docs/references/generated/db-schema.md`
- **Run:** `node scripts/generate-db-schema.mjs` or `pnpm generate:db-schema`
- **When to run:** after any migration PR or manual schema change
- **Idempotent:** two consecutive runs produce zero diff in the output file

## `setup.sh`

Bootstraps the local Demucs ONNX model required by later separation work.

- **Input:** none
- **Prerequisites:** `curl`, `shasum`
- **Output:** `src-tauri/models/htdemucs.onnx`
- **Success:** downloads the model, verifies SHA-256, and stores it in the models directory
- **Repeat runs:** exit immediately if the existing model already matches the pinned checksum
- **Failure:** exits non-zero with a readable error if a required tool is missing, the download fails, or checksum verification fails

Run it from the repository root:

```bash
./scripts/setup.sh
```

## `run-local-smoke.sh`

Runs a local backend smoke pass against real audio files in a directory and
writes JSON + Markdown reports into an output directory.

- **Input:** optional input directory, defaults to `./test`
- **Prerequisites:** Rust toolchain, local dependencies installed, optional
  model downloaded via `./scripts/setup.sh` if separation should run
- **Output:** `output/local-audio-smoke-report.json`,
  `output/local-audio-smoke-report.md`, and separation cache under
  `output/cache/`
- **Success:** imports supported audio files, profiles playback load/seek, and
  runs separation when a verified model is available
- **Repeat runs:** overwrite the smoke DB/report files while reusing any cached
  stems under the selected output directory
- **Failure:** exits non-zero with readable stderr when the input directory is
  missing, no readable audio files are found, or a backend step fatally fails

Run it from the repository root:

```bash
./scripts/run-local-smoke.sh
```

Optional custom paths:

```bash
./scripts/run-local-smoke.sh ./test ./output
```

## `generate-macos-liquid-glass-icon.mjs`

Compiles the Icon Composer project into macOS 26 Liquid Glass assets.

- **Input:** `src-tauri/icons/OpenKara.icon/` plus `src-tauri/icons/app-icon.png`
  (extracts the microphone foreground into `OpenKara Mic.png` before compiling;
  the `.icon` fill owns the macOS 26 background shape)
- **Prerequisites:** macOS host with Xcode `actool` (`xcrun actool`)
- **Output:** `src-tauri/icons/Assets.car`, `src-tauri/icons/OpenKara.icns`
- **Run:** `node scripts/generate-macos-liquid-glass-icon.mjs` or `pnpm icons:generate` (chained after `tauri icon`)
- **Non-macOS hosts:** exits successfully without writing files
- **When to run:** after changing `app-icon.png` or `OpenKara.icon/icon.json`
- **Bundle:** `Assets.car` is copied into the app via `tauri.conf.json` `bundle.resources`; `Info.plist` sets `CFBundleIconName` to `OpenKara`

## `generate-flatpak-node-sources.mjs`

Regenerates Flatpak offline pnpm dependency sources from `pnpm-lock.yaml`.

- **Input:** `pnpm-lock.yaml` plus existing
  `packaging/flatpak/generated/node-sources.0.json` scaffold entries
- **Output:** `packaging/flatpak/generated/node-sources.0.json`
- **Run:** `node scripts/generate-flatpak-node-sources.mjs` or
  `pnpm generate:flatpak-node-sources`
- **When to run:** after changing JavaScript dependencies or lockfile entries
  used by Flatpak packaging

## `flatpak/populate_pnpm_store.mjs`

Seeds the Flatpak offline pnpm 11 store from downloaded tarballs. Canonical
copy also lives inline in `node-sources.0.json` as
`flatpak-node/populate_pnpm_store.mjs` and is invoked from the Flatpak
manifest **after** the pnpm tarball is installed.

- **Why:** pnpm 11 indexes packages in `store-dir/v11/index.db` (SQLite +
  msgpackr). Legacy JSON `index/` entries are ignored, which previously
  produced `ERR_PNPM_NO_OFFLINE_TARBALL` despite intact CAFS blobs.
- **How:** replays each lockfile tarball through pnpm's own
  `dist/worker.js` extract path so the store matches a normal install.
- **Run (inside Flatpak build):**
  `node flatpak-node/populate_pnpm_store.mjs <manifest.json> <tarball-dir> <store-dir>`
