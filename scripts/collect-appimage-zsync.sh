#!/usr/bin/env bash
set -euo pipefail
bundle="${1:?bundle directory required}"
root="$(cd "$(dirname "$0")/.." && pwd)"
shopt -s nullglob
images=("$bundle"/*.AppImage)
[ "${#images[@]}" -eq 1 ]
image="$(realpath "${images[0]}")"
control="$image.zsync"
if [ ! -f "$control" ]; then
  for candidate in "$root/$(basename "$control")" "$root/src-tauri/$(basename "$control")"; do
    if [ -f "$candidate" ]; then mv "$candidate" "$control"; break; fi
  done
fi
test -s "$control"
python3 - "$image" <<'PY'
import hashlib, pathlib, subprocess, sys
image = pathlib.Path(sys.argv[1])
result = subprocess.run([str(image), '--appimage-updateinformation'], capture_output=True, text=True, check=True)
expected = 'gh-releases-zsync|thedavidweng|OpenKara|latest|OpenKara_*_amd64.AppImage.zsync'
if (result.stdout + result.stderr).strip() != expected:
    raise SystemExit('AppImageUpdate transport mismatch')
header = image.with_name(image.name + '.zsync').read_bytes().split(b'\n\n', 1)[0].decode()
fields = dict(line.split(': ', 1) for line in header.splitlines() if ': ' in line)
if fields.get('SHA-1') != hashlib.sha1(image.read_bytes()).hexdigest() or fields.get('Filename') != image.name:
    raise SystemExit('zsync does not describe the final signed-updater AppImage')
PY
