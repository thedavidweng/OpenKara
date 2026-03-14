#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE_PATH="$ROOT_DIR/packaging/homebrew/openkara.rb.template"
OUTPUT_PATH="${6:-"$ROOT_DIR/packaging/homebrew/openkara.generated.rb"}"

usage() {
  echo "usage: ./scripts/render-homebrew-cask.sh <version> <arm-url> <arm-sha256> <intel-url> <intel-sha256> [output-path]" >&2
}

if [[ $# -lt 5 || $# -gt 6 ]]; then
  usage
  exit 1
fi

VERSION="$1"
ARM_URL="$2"
ARM_SHA256="$3"
INTEL_URL="$4"
INTEL_SHA256="$5"

if [[ ! -f "$TEMPLATE_PATH" ]]; then
  echo "missing cask template at $TEMPLATE_PATH" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT_PATH")"

sed \
  -e "s|__VERSION__|$VERSION|g" \
  -e "s|__ARM_URL__|$ARM_URL|g" \
  -e "s|__ARM_SHA256__|$ARM_SHA256|g" \
  -e "s|__INTEL_URL__|$INTEL_URL|g" \
  -e "s|__INTEL_SHA256__|$INTEL_SHA256|g" \
  "$TEMPLATE_PATH" >"$OUTPUT_PATH"

echo "rendered Homebrew cask to $OUTPUT_PATH"
