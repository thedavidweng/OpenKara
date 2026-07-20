#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODELS_DIR="$ROOT_DIR/src-tauri/models"
MODEL_FILENAME="htdemucs.onnx"
MODEL_PATH="$MODELS_DIR/$MODEL_FILENAME"
# Stable manifest URL that maps each model variant to its newest release.
# Resolved at runtime so this script always fetches the latest model without
# a manual version bump.
MODEL_MANIFEST_URL="https://raw.githubusercontent.com/thedavidweng/openkara-models/main/latest.json"

require_tool() {
  local tool="$1"

  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "error: required tool '$tool' is not installed" >&2
    exit 1
  fi
}

resolve_latest_model() {
  local manifest
  manifest="$(curl -fsSL "$MODEL_MANIFEST_URL")"
  MODEL_URL="$(echo "$manifest" | jq -r '.htdemucs.url')"
  MODEL_SHA256="$(echo "$manifest" | jq -r '.htdemucs.sha256')"
  MODEL_TAG="$(echo "$manifest" | jq -r '.htdemucs.tag')"

  if [[ -z "$MODEL_URL" || -z "$MODEL_SHA256" ]]; then
    echo "error: failed to resolve model URL or SHA-256 from manifest" >&2
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
require_tool jq
require_tool node
require_tool shasum

resolve_latest_model
echo "Latest model: $MODEL_TAG"

mkdir -p "$MODELS_DIR"

if [[ -f "$MODEL_PATH" ]]; then
  if verify_checksum "$MODEL_PATH"; then
    echo "Model already present and verified at $MODEL_PATH"
    node "$ROOT_DIR/scripts/prepare-onnx-runtime.mjs"
    exit 0
  fi

  echo "Existing model at $MODEL_PATH does not match the latest release."
  echo "Removing the stale file and downloading the latest version."
  rm -f "$MODEL_PATH"
fi

tmp_file="$(mktemp "$MODELS_DIR/$MODEL_FILENAME.download.XXXXXX")"

cleanup() {
  rm -f "$tmp_file"
}

trap cleanup EXIT

echo "Downloading $MODEL_FILENAME to $tmp_file"
curl -L --fail --progress-bar "$MODEL_URL" -o "$tmp_file"

if ! verify_checksum "$tmp_file"; then
  echo "error: downloaded model checksum mismatch" >&2
  echo "error: expected $MODEL_SHA256" >&2
  exit 1
fi

mv "$tmp_file" "$MODEL_PATH"
trap - EXIT

echo "Model verified and saved to $MODEL_PATH"
node "$ROOT_DIR/scripts/prepare-onnx-runtime.mjs"
