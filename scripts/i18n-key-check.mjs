/**
 * Pure, dependency-free i18n key-structure checks.
 *
 * This module is imported by BOTH the CLI checker (`scripts/check-i18n.mjs`,
 * which adds filesystem reading + a process exit code) and the vitest suite
 * (`src/locales/locales.test.ts`, which runs under jsdom). Keeping the logic
 * pure — no `fs`, no `process`, no globals beyond `Intl` — is what lets a Node
 * script and a browser-like test share exactly the same comparison.
 */

/**
 * i18next plural suffixes, matched WITH a leading underscore. This matters:
 * a real key whose last segment merely ends in "Other"/"One" (e.g.
 * `songProperties.channelsOther`) must NOT be mistaken for a plural variant.
 */
export const PLURAL_SUFFIXES = [
  "_zero",
  "_one",
  "_two",
  "_few",
  "_many",
  "_other",
];

/** Walk an object and return all leaf entries as `[dotPath, value]` pairs. */
export function flattenEntries(obj, prefix = "") {
  const entries = [];
  for (const [k, v] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${k}` : k;
    if (v !== null && typeof v === "object" && !Array.isArray(v)) {
      entries.push(...flattenEntries(v, path));
    } else {
      entries.push([path, v]);
    }
  }
  return entries;
}

/** Walk an object and return all leaf paths (e.g. "setup.welcome"). */
export function flattenKeys(obj, prefix = "") {
  return flattenEntries(obj, prefix).map(([path]) => path);
}

/**
 * Split a flat key into `{ base, category }` when it ends in an i18next plural
 * suffix, else `null`. `category` is the bare Intl category ("one", "other", …).
 */
export function splitPlural(key) {
  for (const suffix of PLURAL_SUFFIXES) {
    if (key.endsWith(suffix)) {
      return { base: key.slice(0, -suffix.length), category: suffix.slice(1) };
    }
  }
  return null;
}

/**
 * Analyze the reference (en) flat key list into its non-plural keys and the set
 * of plural base keys. A base is "plural" if ANY suffixed variant exists.
 */
export function analyzeReference(referenceKeys) {
  const pluralBases = new Set();
  const nonPluralKeys = new Set();
  for (const key of referenceKeys) {
    const split = splitPlural(key);
    if (split) pluralBases.add(split.base);
    else nonPluralKeys.add(key);
  }
  // If a base also appeared as a bare key, keep it a plural base only so it is
  // checked once, via the plural rules.
  for (const base of pluralBases) nonPluralKeys.delete(base);
  return { pluralBases, nonPluralKeys };
}

/**
 * The plural categories i18next expects for a locale, per the ICU/Intl spec.
 * Falls back to `["other"]` for an unrecognized code.
 */
export function pluralCategoriesFor(localeCode) {
  try {
    return new Intl.PluralRules(localeCode).resolvedOptions().pluralCategories;
  } catch {
    return ["other"];
  }
}

/**
 * Compare one locale's flat key list against the analyzed reference.
 *
 * Returns `{ missing, extra, extraPluralWarnings }`, each a sorted string array:
 *   - `missing`  required keys absent from the locale (FAILURE)
 *   - `extra`    keys present in the locale but not the reference (FAILURE)
 *   - `extraPluralWarnings` plural categories beyond the locale's Intl set, on a
 *                key that IS a reference plural base (WARNING — tolerated)
 */
export function compareLocale(referenceAnalysis, localeKeys, localeCode) {
  const { pluralBases, nonPluralKeys } = referenceAnalysis;
  const intlCategories = new Set(pluralCategoriesFor(localeCode));

  const localePlain = new Set();
  const localePlurals = new Map(); // base -> Set(category)
  for (const key of localeKeys) {
    const split = splitPlural(key);
    if (split) {
      let categories = localePlurals.get(split.base);
      if (!categories) {
        categories = new Set();
        localePlurals.set(split.base, categories);
      }
      categories.add(split.category);
    } else {
      localePlain.add(key);
    }
  }

  const missing = [];
  const extra = [];
  const extraPluralWarnings = [];

  // Non-plural keys require an exact match.
  for (const key of nonPluralKeys) {
    if (!localePlain.has(key)) missing.push(key);
  }

  // Plural base keys require exactly the categories the locale's Intl rules
  // declare. Extra categories beyond that set are tolerated (warning).
  for (const base of pluralBases) {
    const have = localePlurals.get(base) ?? new Set();
    for (const category of intlCategories) {
      if (!have.has(category)) missing.push(`${base}_${category}`);
    }
    for (const category of have) {
      if (!intlCategories.has(category)) {
        extraPluralWarnings.push(`${base}_${category}`);
      }
    }
  }

  // Plain keys not in the reference are genuinely extra. A plain key that
  // collides with a reference plural base means the locale forgot to pluralize,
  // which is also wrong — so it lands here too.
  for (const key of localePlain) {
    if (!nonPluralKeys.has(key)) extra.push(key);
  }

  // Suffixed keys whose base is not a reference plural base are genuinely extra.
  for (const [base, categories] of localePlurals) {
    if (!pluralBases.has(base)) {
      for (const category of categories) extra.push(`${base}_${category}`);
    }
  }

  missing.sort();
  extra.sort();
  extraPluralWarnings.sort();
  return { missing, extra, extraPluralWarnings };
}
