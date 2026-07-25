#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODELS_DIR="$ROOT_DIR/src-tauri/models"

require_tool() {
  local tool="$1"

  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "error: required tool '$tool' is not installed" >&2
    exit 1
  fi
}

verify_checksum() {
  local file_path="$1"
  local actual_checksum

  actual_checksum="$(shasum -a 256 "$file_path" | awk '{print $1}')"
  [[ "$actual_checksum" == "$MODEL_SHA256" ]]
}

require_tool curl
require_tool node
require_tool shasum

# Model identity comes from the pinned catalog snapshot — the same contract
# fixture the application embeds. Never hardcode model URLs or digests here.
# The download may be a compressed archive; the installed file is the
# extracted .onnx, verified against its own digest.
MODEL_FILENAME="$(node "$ROOT_DIR/scripts/resolve-model.mjs" --field filename)"
MODEL_PATH="$MODELS_DIR/$MODEL_FILENAME"
MODEL_URL="$(node "$ROOT_DIR/scripts/resolve-model.mjs" --field url)"
MODEL_SHA256="$(node "$ROOT_DIR/scripts/resolve-model.mjs" --field file_sha256)"
DOWNLOAD_SHA256="$(node "$ROOT_DIR/scripts/resolve-model.mjs" --field sha256)"
DOWNLOAD_ARCHIVED="$(node "$ROOT_DIR/scripts/resolve-model.mjs" --field archived)"

mkdir -p "$MODELS_DIR"

if [[ -f "$MODEL_PATH" ]]; then
  if verify_checksum "$MODEL_PATH"; then
    echo "Model already present and verified at $MODEL_PATH"
    node "$ROOT_DIR/scripts/prepare-onnx-runtime.mjs"
    exit 0
  fi

  echo "error: existing model at $MODEL_PATH failed SHA-256 verification" >&2
  echo "error: remove the file and rerun scripts/setup.sh to fetch a clean copy" >&2
  exit 1
fi

tmp_file="$(mktemp "$MODELS_DIR/$MODEL_FILENAME.download.XXXXXX")"

cleanup() {
  rm -f "$tmp_file"
}

trap cleanup EXIT

echo "Downloading $MODEL_FILENAME to $tmp_file"
curl -L --fail --progress-bar "$MODEL_URL" -o "$tmp_file"

download_checksum="$(shasum -a 256 "$tmp_file" | awk '{print $1}')"
if [[ "$download_checksum" != "$DOWNLOAD_SHA256" ]]; then
  echo "error: downloaded payload checksum mismatch" >&2
  echo "error: expected $DOWNLOAD_SHA256" >&2
  exit 1
fi

if [[ "$DOWNLOAD_ARCHIVED" == "true" ]]; then
  extract_dir="$(mktemp -d "$MODELS_DIR/extract.XXXXXX")"
  tar -xzf "$tmp_file" -C "$extract_dir"
  extracted="$extract_dir/$MODEL_FILENAME"
  if [[ ! -f "$extracted" ]]; then
    echo "error: archive did not contain $MODEL_FILENAME" >&2
    rm -rf "$extract_dir"
    exit 1
  fi
  if ! verify_checksum "$extracted"; then
    echo "error: extracted model checksum mismatch" >&2
    rm -rf "$extract_dir"
    exit 1
  fi
  mv "$extracted" "$MODEL_PATH"
  rm -rf "$extract_dir"
  rm -f "$tmp_file"
else
  if ! verify_checksum "$tmp_file"; then
    echo "error: downloaded model checksum mismatch" >&2
    echo "error: expected $MODEL_SHA256" >&2
    exit 1
  fi
  mv "$tmp_file" "$MODEL_PATH"
fi
trap - EXIT

echo "Model verified and saved to $MODEL_PATH"
node "$ROOT_DIR/scripts/prepare-onnx-runtime.mjs"
