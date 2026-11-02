#!/usr/bin/env bash
set -euo pipefail

flatpak run --env=WEBKIT_DISABLE_COMPOSITING_MODE=1 io.github.thedavidweng.OpenKara > "$RUNNER_TEMP/flatpak-launch.log" 2>&1 &
pid=$!
trap 'kill "$pid" 2>/dev/null || true' EXIT

for ((attempt = 1; attempt <= 30; attempt++)); do
  kill -0 "$pid" || { cat "$RUNNER_TEMP/flatpak-launch.log"; exit 1; }
  if window=$(xdotool search --onlyvisible --name '^OpenKara$' | head -n 1); then
    sleep 5
    scrot "$RUNNER_TEMP/flatpak-startup.png"
    xdotool mousemove --window "$window" 120 120 click 1
    sleep 2
    kill -0 "$pid"
    exit 0
  fi
  sleep 2
done

cat "$RUNNER_TEMP/flatpak-launch.log"
exit 1
