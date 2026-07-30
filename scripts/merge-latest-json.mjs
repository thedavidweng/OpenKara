#!/usr/bin/env node
// Merge Tauri updater latest.json files.
// The first argument is the base manifest; each subsequent argument is a
// platform-specific manifest whose platforms entries are merged in.
// The result is written to stdout.

import { readFileSync } from "node:fs";

const paths = process.argv.slice(2);
if (paths.length === 0) {
  console.error(
    "Usage: merge-latest-json.mjs <base.json> [<overlay.json> ...]",
  );
  process.exit(1);
}

const base = JSON.parse(readFileSync(paths[0], "utf8"));

for (const path of paths.slice(1)) {
  const overlay = JSON.parse(readFileSync(path, "utf8"));
  if (overlay.version) {
    base.version = overlay.version;
  }
  if (overlay.notes) {
    base.notes = overlay.notes;
  }
  if (overlay.pub_date) {
    base.pub_date = overlay.pub_date;
  }
  if (overlay.platforms) {
    base.platforms = { ...(base.platforms || {}), ...overlay.platforms };
  }
}

process.stdout.write(JSON.stringify(base, null, 2));
