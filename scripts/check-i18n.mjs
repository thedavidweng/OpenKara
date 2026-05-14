#!/usr/bin/env node

/**
 * Check that all locale files have the same key structure.
 *
 * Exits 0 on match, non-zero with a diff report on mismatch.
 *
 * Usage:
 *   node scripts/check-i18n.mjs
 *
 * Run before release to catch missing translations or stale keys.
 */

import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const LOCALES_DIR = join(__dirname, "..", "src", "locales");

/** Walk an object and return all leaf paths (e.g. "setup.welcome"). */
function flattenKeys(obj, prefix = "") {
  const keys = [];
  for (const [k, v] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${k}` : k;
    if (v !== null && typeof v === "object" && !Array.isArray(v)) {
      keys.push(...flattenKeys(v, path));
    } else {
      keys.push(path);
    }
  }
  return keys;
}

/** Parse a locale JSON file and return its flat key set. */
function loadKeys(file) {
  const raw = readFileSync(join(LOCALES_DIR, file), "utf-8");
  const data = JSON.parse(raw);
  return new Set(flattenKeys(data));
}

const files = ["en.json", "zh-CN.json"];
const keySets = {};
for (const f of files) {
  keySets[f] = loadKeys(f);
}

// Use en.json as the reference (canonical).
const reference = "en.json";
const referenceKeys = keySets[reference];

let exitCode = 0;

for (const [file, keys] of Object.entries(keySets)) {
  if (file === reference) continue;

  // Keys in reference but missing from this file
  const missing = [...referenceKeys].filter((k) => !keys.has(k));
  if (missing.length > 0) {
    console.error(`[ MISSING ] ${file} is missing ${missing.length} key(s):`);
    for (const k of missing) {
      console.error(`  - ${k}`);
    }
    exitCode = 1;
  }

  // Keys in this file but absent from the reference (stale / unused)
  const extra = [...keys].filter((k) => !referenceKeys.has(k));
  if (extra.length > 0) {
    console.error(
      `[ EXTRA  ] ${file} has ${extra.length} key(s) not in reference:`,
    );
    for (const k of extra) {
      console.error(`  - ${k}`);
    }
    // Extra keys are a warning, not a failure — they might be references for
    // i18next plurals (e.g. `_one` / `_other`) or deprecated keys.
  }
}

if (exitCode === 0) {
  console.log("i18n keys OK — all locales match en.json reference.");
}
process.exit(exitCode);
