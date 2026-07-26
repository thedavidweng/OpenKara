// Type declarations for the pure JS i18n key-check helpers so the vitest suite
// (a .ts file under the DOM tsconfig) can import them without tripping the
// `tsc --noEmit` gate. The runtime source lives in ./i18n-key-check.mjs.

export const PLURAL_SUFFIXES: readonly string[];

export function flattenEntries(
  obj: Record<string, unknown>,
  prefix?: string,
): [string, unknown][];

export function flattenKeys(
  obj: Record<string, unknown>,
  prefix?: string,
): string[];

export function splitPlural(
  key: string,
): { base: string; category: string } | null;

export interface ReferenceAnalysis {
  pluralBases: Set<string>;
  nonPluralKeys: Set<string>;
}

export function analyzeReference(referenceKeys: string[]): ReferenceAnalysis;

export function pluralCategoriesFor(localeCode: string): readonly string[];

export function compareLocale(
  referenceAnalysis: ReferenceAnalysis,
  localeKeys: string[],
  localeCode: string,
): { missing: string[]; extra: string[]; extraPluralWarnings: string[] };
