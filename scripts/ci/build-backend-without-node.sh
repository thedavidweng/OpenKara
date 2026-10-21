#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test -f "$root/dist/index.html" || {
  echo "Build frontend assets with pnpm build before this backend-only scenario." >&2
  exit 1
}
test -f "$root/packaging/source-runtime.json"

cd "$root/src-tauri"
cargo_executable="$(rustup which cargo)"
export RUSTC="$(rustup which rustc)"
export RUSTDOC="$(rustup which rustdoc)"
unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER

isolated_path="$(mktemp -d)"
trap 'rm -rf "$isolated_path"' EXIT
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$isolated_path/target}"
[[ "$CARGO_TARGET_DIR" = /* ]] || {
  echo "CARGO_TARGET_DIR must be an absolute path outside the checkout." >&2
  exit 1
}
case "$CARGO_TARGET_DIR/" in
  "$root/"*)
    echo "CARGO_TARGET_DIR must be outside the checkout." >&2
    exit 1
    ;;
esac
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

# Force the application build script to execute even with a restored Rust cache.
"$cargo_executable" clean --package openkara --release
"$cargo_executable" build --locked --release --bin openkara
