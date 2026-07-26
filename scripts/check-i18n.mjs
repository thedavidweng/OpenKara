#!/usr/bin/env node

/**
 * Check that every locale file in src/locales matches en.json's key structure.
 *
 * Exits 0 when every locale matches (extra plural categories are tolerated as
 * warnings), non-zero with a per-file report on any missing or genuinely-extra
 * key.
 *
 * Usage:
 *   node scripts/check-i18n.mjs
 *
 * Run before release to catch missing translations or stale keys. The pure
 * comparison logic lives in ./i18n-key-check.mjs so the vitest suite
 * (src/locales/locales.test.ts) can apply the exact same rules.
 */

import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import {
  flattenKeys,
  analyzeReference,
  compareLocale,
} from "./i18n-key-check.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const LOCALES_DIR = join(__dirname, "..", "src", "locales");
const REFERENCE = "en.json";

/** Parse a locale JSON file and return its flat key list. */
function loadKeys(file) {
  const raw = readFileSync(join(LOCALES_DIR, file), "utf-8");
  return flattenKeys(JSON.parse(raw));
}

// Every locale JSON in the directory — no hardcoded list, so a new
// src/locales/<code>.json is checked the moment it lands.
const files = readdirSync(LOCALES_DIR)
  .filter((f) => f.endsWith(".json"))
  .sort();

if (!files.includes(REFERENCE)) {
  console.error(`Reference locale ${REFERENCE} not found in ${LOCALES_DIR}.`);
  process.exit(1);
}

const referenceAnalysis = analyzeReference(loadKeys(REFERENCE));

let exitCode = 0;

for (const file of files) {
  if (file === REFERENCE) continue;

  const localeCode = file.slice(0, -".json".length);
  const { missing, extra, extraPluralWarnings } = compareLocale(
    referenceAnalysis,
    loadKeys(file),
    localeCode,
  );

  const failed = missing.length > 0 || extra.length > 0;

  if (missing.length > 0) {
    console.error(`[ MISSING ] ${file} is missing ${missing.length} key(s):`);
    for (const k of missing) console.error(`  - ${k}`);
    exitCode = 1;
  }

  if (extra.length > 0) {
    console.error(
      `[ EXTRA   ] ${file} has ${extra.length} key(s) not in ${REFERENCE}:`,
    );
    for (const k of extra) console.error(`  - ${k}`);
    exitCode = 1;
  }

  if (extraPluralWarnings.length > 0) {
    console.warn(
      `[ WARN    ] ${file} declares ${extraPluralWarnings.length} plural ` +
        `categor${extraPluralWarnings.length === 1 ? "y" : "ies"} beyond ` +
        `Intl.PluralRules("${localeCode}") (tolerated):`,
    );
    for (const k of extraPluralWarnings) console.warn(`  - ${k}`);
  }

  if (!failed && extraPluralWarnings.length === 0) {
    console.log(
      `[ OK      ] ${file} matches ${REFERENCE} (locale ${localeCode}).`,
    );
  }
}

if (exitCode === 0) {
  console.log("\ni18n keys OK — all locales match en.json reference.");
}

process.exit(exitCode);
