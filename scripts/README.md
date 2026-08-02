# Scripts

## `ci/nightly-evidence.mjs`

The script creates and verifies the Nightly release-gate manifest.

- **Create the manifest:** `node scripts/ci/nightly-evidence.mjs create --commit <sha> --run-id <id> --needs-json <json> --output nightly-evidence.json`
- **Verify the manifest:** `node scripts/ci/nightly-evidence.mjs verify --input nightly-evidence.json --commit <sha> --max-age-hours 24`
- **Write the artifact:** `nightly-evidence.json` in the `nightly-evidence` workflow artifact
- **Run the contract tests:** `tests/nightly-evidence.test.ts`

## `ci/run-accessibility-matrix.mjs`

The script runs the extended Playwright accessibility matrix on Windows, macOS,
and Linux.

- **Run the matrix:** `pnpm test:a11y:matrix`
- **Write the reports:** Playwright console, HTML, JSON, trace, and screenshot reports
- **Test path:** `tests/e2e/accessibility/`

## `openkara_release_evidence`

Builds and validates the canonical release evidence files from the Rust
release evidence module.

- **Automation validation:** `openkara_release_evidence validate-automation-report --input automation-report.json --output installed-app-smoke-validation.json`
- **Desktop E2E validation:** `openkara_release_evidence validate-desktop-e2e --input desktop-e2e-report.json --scenario keyboard-workflow`
- **Separation smoke validation:** `openkara_release_evidence validate-local-audio-smoke --input local-audio-smoke-report.json`
- **Schema:** `cargo run --features automation-driver --bin openkara_release_evidence -- schema schemas/release-evidence.schema.json`
- **Fragment:** `openkara_release_evidence fragment-from-automation-report --input automation-report.json --repository thedavidweng/OpenKara --commit-sha <sha> --tag <tag> --version <version> --platform <platform> --scenario <scenario> --output release-evidence-fragment.json`
- **Aggregate:** `openkara_release_evidence aggregate --commit-sha <sha> --tag <tag> --version <version> --fragment <fragment> --output release-evidence.json`
- **Candidate verification:** `openkara_release_evidence verify-assets --evidence release-evidence.json --target <target> --assets-dir <candidate-root>`
- **Latest manifest:** `openkara_release_evidence latest --evidence release-evidence.json --output latest.json`
- **Checksums:** `openkara_release_evidence checksums --evidence release-evidence.json --assets-dir <release-assets> --output SHA256SUMS`
- **Output:** JSON Schema, platform evidence fragments, canonical aggregate evidence, updater manifest, and checksums
- **Owner:** `src-tauri/src/release_evidence.rs`

## `ci/classify-changes.mjs`

CI change classifier — the single source of truth for path-based CI gating.
Maps changed files to categories, collects unmatched files as `unknown`, and
derives the expected job set from the category union. Consumed by the triage
job in `.github/workflows/ci.yml` and `.github/workflows/packaging.yml`.

- **Input:** newline-delimited filenames (`--files`) or JSON array (`--json`),
  plus event type (`--event pull_request|push|workflow_dispatch`)
- **Events:** `pull_request` and `push` are path-aware; `workflow_dispatch`
  forces full CI. Full multi-platform matrices live on Nightly — see
  `docs/CI_LAYERS.md`.
- **Output:** JSON to stdout; `GITHUB_OUTPUT` entries (`expected-jobs`,
  `run_<job>` booleans, `unknown-files`, `categories`) and
  `GITHUB_STEP_SUMMARY` markdown table when those env vars are set
- **Pure function:** no network or filesystem access beyond stdin — testable
  locally without GitHub API access
- **Contract tests:** `tests/ci/classify-changes.test.ts`
- **Drift tests:** `tests/ci/ci-workflow-contract.test.ts`
- **Run:** `node scripts/ci/classify-changes.mjs --files <paths> --event pull_request`

## `generate-mock-songs.mjs`

Regenerates the shared mock/preview song catalog used by both the website
embedded preview and the Playwright E2E Tauri mock from a local playlist of
m4a files.

- **Input:** m4a files in `~/Music/OpenKara/media` (override with
  `--media-dir <path>`)
- **Output:** `src/mock/preview-songs.ts` (self-contained: base64 cover art +
  synced lyrics + MBIDs) and `src/mock/covers/*.jpg` (300×300 downscaled
  JPEGs for human/git inspection)
- **Lyrics source:** fetched from lrclib.net (`/api/get`) using the embedded
  title/artist/album/duration tags. Synced lyrics (LRC with real
  `[mm:ss.xx]` timestamps) are used when available; otherwise the embedded
  m4a `lyrics` tag is used with pseudo-LRC timestamps as a fallback
- **Run:** `node scripts/generate-mock-songs.mjs [--media-dir <path>] [--cover-size 300]`
- **When to run:** after changing the preview playlist
- **Idempotent:** two consecutive runs produce zero diff in the output files
  for the same input media and cover size (assuming lrclib returns the same
  synced lyrics)
- **Why a shared module:** the website preview (`website/src/mock-app.ts`)
  and the E2E Tauri mock (`tests/e2e/fixtures/tauri-mock.ts`) both serialize
  from `src/mock/preview-songs.ts` so the two surfaces cannot drift apart

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

## `flatpak/rewrite_lockfile_local_tarballs.mjs`

Rewrites `pnpm-lock.yaml` package resolutions in the Flatpak build directory
so each entry has `tarball: file:flatpak-node/pnpm-tarballs/<name>.tgz`.

- **Why:** even with a pre-seeded store, offline install must never hit
  `registry.npmjs.org` inside the Flatpak sandbox (DNS fails with EAI_AGAIN).
  The `file:` resolution uses pnpm's localTarball fetcher, which works offline.
- **Run (inside Flatpak build, before install):**
  `node flatpak-node/rewrite_lockfile_local_tarballs.mjs`

## `generate-flatpak-cargo-sources.mjs`

Regenerates Flatpak offline Cargo dependency sources from `src-tauri/Cargo.lock`.

- **Input:** `src-tauri/Cargo.lock`
- **Output:** `packaging/flatpak/generated/cargo-sources.json`
- **Run:** `node scripts/generate-flatpak-cargo-sources.mjs [lockfile] [output]` or
  `pnpm generate:flatpak-cargo-sources`
- **Optional args:** `lockfile` and `output` override the default input/output
  paths (used by tests to render into a temp directory without touching the
  checkout)
- **Exports:** `parseCargoLockfile`, `generateCargoSources`, and
  `renderCargoSources` (pure rendering for non-destructive tests)
- **When to run:** after changing Rust dependencies or `Cargo.lock` entries
  used by Flatpak packaging
- **Idempotent:** two consecutive runs produce zero diff in the output file
