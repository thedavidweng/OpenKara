#!/bin/bash
set -euo pipefail

bundle_dir="${1:?provide the Tauri bundle directory}"
: "${APPLE_TEAM_ID:?}" "${APPLE_API_KEY:?}" "${APPLE_API_ISSUER:?}" "${APPLE_API_KEY_PATH:?}"
apps=("$bundle_dir"/macos/*.app)
dmgs=("$bundle_dir"/dmg/*.dmg)
archives=("$bundle_dir"/macos/*.app.tar.gz)
[[ ${#apps[@]} == 1 && -d "${apps[0]}" ]]
[[ ${#dmgs[@]} == 1 && -f "${dmgs[0]}" ]]
[[ ${#archives[@]} == 1 && -f "${archives[0]}.sig" ]]

verify_signature() {
  codesign --verify --deep --strict --verbose=2 "$1"
  details="$(codesign --display --verbose=4 "$1" 2>&1)"
  grep -Fq 'Authority=Developer ID Application: ' <<< "$details"
  grep -Fxq "TeamIdentifier=$APPLE_TEAM_ID" <<< "$details"
  grep -q '^Timestamp=' <<< "$details"
}

verify_app() {
  verify_signature "$1"
  grep -q 'flags=.*(runtime)' <<< "$details"
  xcrun stapler validate "$1"
  spctl --assess --type execute --verbose=2 "$1"
}

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
verify_app "${apps[0]}"
tar -xzf "${archives[0]}" -C "$work_dir"
verify_app "$work_dir/OpenKara.app"
diff -rq "${apps[0]}" "$work_dir/OpenKara.app"
verify_signature "${dmgs[0]}"
xcrun notarytool submit "${dmgs[0]}" \
  --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER" \
  --wait --timeout 30m --output-format json > "$work_dir/result.json"
cat "$work_dir/result.json"
jq -e '.status == "Accepted"' "$work_dir/result.json" > /dev/null
xcrun stapler staple "${dmgs[0]}"
xcrun stapler validate "${dmgs[0]}"
spctl --assess --type open --context context:primary-signature --verbose=2 "${dmgs[0]}"
