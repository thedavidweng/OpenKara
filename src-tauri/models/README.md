# models

This directory is a local cache for large model binaries used during
development.

Rules:

- Commit `.gitkeep` only.
- Do **not** commit downloaded `.onnx` files.
- `scripts/setup.sh` may populate this directory for local development and
  tests.
- Runtime installs should prefer the app data directory instead of this repo
  path.

The current default model filename is `htdemucs.onnx`.
