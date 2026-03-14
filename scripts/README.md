# Scripts

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

## `render-homebrew-cask.sh`

Renders a Homebrew Cask file from release metadata so the separate tap repo can
be updated without rewriting the Ruby file by hand.

- **Input:** release version, Apple Silicon DMG URL + SHA-256, Intel DMG URL +
  SHA-256
- **Prerequisites:** macOS release assets already published
- **Output:** `packaging/homebrew/openkara.generated.rb` by default
- **Success:** emits a versioned cask file ready to copy into the Homebrew tap
- **Failure:** exits non-zero if the template is missing or the argument set is
  incomplete

Run it from the repository root:

```bash
./scripts/render-homebrew-cask.sh \
  0.1.0 \
  https://example.com/OpenKara-aarch64.dmg \
  ARM_SHA256 \
  https://example.com/OpenKara-x64.dmg \
  INTEL_SHA256
```
