#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test -f "$root/dist/index.html" || {
  echo "Build frontend assets with pnpm build before this backend-only scenario." >&2
  exit 1
}
test -f "$root/packaging/source-runtime.json"

isolated_path="$(mktemp -d)"
trap 'rm -rf "$isolated_path"' EXIT
IFS=: read -r -a search_paths <<< "$PATH"
for directory in "${search_paths[@]}"; do
  [[ "$directory" = /* && -d "$directory" ]] || continue
  for executable in "$directory"/*; do
    [[ -f "$executable" && -x "$executable" ]] || continue
    name="${executable##*/}"
    case "$name" in
      node|nodejs|npm|npx|pnpm|pnpx|corepack|yarn|yarnpkg|bun|bunx|deno) continue ;;
    esac
    [[ -e "$isolated_path/$name" ]] || ln -s "$executable" "$isolated_path/$name"
  done
done
export PATH="$isolated_path"
for command in node nodejs npm pnpm; do
  if command -v "$command"; then
    echo "$command remains available in the backend-only environment" >&2
    exit 1
  fi
done

cd "$root/src-tauri"
# Force the application build script to execute even with a restored Rust cache.
cargo clean --package openkara --release
cargo build --locked --release --bin openkara
