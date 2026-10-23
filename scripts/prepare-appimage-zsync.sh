#!/usr/bin/env bash
set -euo pipefail

bundle="${1:?bundle directory required}"
shopt -s nullglob
images=("$bundle"/*.AppImage)
if [ "${#images[@]}" -ne 1 ]; then
  echo "Expected exactly one AppImage, found ${#images[@]}." >&2
  exit 1
fi
image="$(realpath "${images[0]}")"
expected='gh-releases-zsync|thedavidweng|OpenKara|latest|OpenKara_*_amd64.AppImage.zsync'
information="$("$image" --appimage-updateinformation 2>&1)"
if [ "$information" != "$expected" ]; then
  echo 'AppImageUpdate transport mismatch' >&2
  exit 1
fi

# Generate from the final bytes without changing the signed AppImage.
zsyncmake -u "$(basename "$image")" -o "$image.zsync" "$image"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
(cd "$scratch" && zsync -i "$image" -o verified.AppImage "$image.zsync")
cmp "$image" "$scratch/verified.AppImage"
