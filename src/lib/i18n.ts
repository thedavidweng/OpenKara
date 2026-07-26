import i18next from "i18next";
import { initReactI18next } from "react-i18next";

/**
 * Native display names for the locales we actually ship. This curated map is
 * the ONLY place a human-readable language name lives, and it must stay in
 * lock-step with the JSON files in `src/locales/*.json`: a test
 * (src/locales/locales.test.ts) fails if a file has no name here, or a name
 * here has no file. Translators add their entry when they add their JSON file
 * (see TRANSLATING.md, which lists the canonical native name for every planned
 * language so nobody has to invent one).
 */
export const NATIVE_LANGUAGE_NAMES: Record<string, string> = {
  en: "English",
  "zh-CN": "简体中文",
};

/**
 * Display order for the language pickers, in the issue #227 priority order.
 * Listing a code here is purely an ordering hint — a code with no file/name is
 * simply skipped — so the full roster can sit here up front and every language
 * lands in a stable, deterministic slot the moment its file is added.
 */
const LANGUAGE_PRIORITY = [
  "en",
  "zh-CN",
  "ja",
  "ko",
  "zh-TW",
  "es",
  "pt-BR",
  "fr",
  "de",
  "it",
  "ru",
  "id",
  "vi",
  "th",
  "tr",
  "pl",
  "nl",
];

// Auto-load every locale JSON at build time. `import.meta.glob` is resolved by
// Vite (app build) AND by vitest (which transforms modules through Vite), so
// the same glob populates i18next resources in both environments — there is no
// manual import list to keep in sync when a translator adds a file.
const localeModules = import.meta.glob<Record<string, unknown>>(
  "../locales/*.json",
  { eager: true, import: "default" },
);

/** "../locales/pt-BR.json" -> "pt-BR" */
function codeFromPath(path: string): string {
  const file = path.slice(path.lastIndexOf("/") + 1);
  return file.slice(0, -".json".length);
}

const translations: Record<string, Record<string, unknown>> = {};
for (const [path, data] of Object.entries(localeModules)) {
  translations[codeFromPath(path)] = data;
}

/** BCP-47 codes of every locale file actually present, unordered. */
const LOADED_LOCALE_CODES = Object.keys(translations);

function orderIndex(code: string): number {
  const index = LANGUAGE_PRIORITY.indexOf(code);
  return index === -1 ? Number.POSITIVE_INFINITY : index;
}

/**
 * The languages offered in every picker (onboarding + settings). Derived from
 * the loaded files so adding `src/locales/<code>.json` (plus a name above) is
 * the only step. Ordered by LANGUAGE_PRIORITY, any unlisted file last
 * (alphabetical) as a safe deterministic fallback.
 */
export const SUPPORTED_LANGUAGES = LOADED_LOCALE_CODES.slice()
  .sort((a, b) => orderIndex(a) - orderIndex(b) || a.localeCompare(b))
  .map((code) => ({ code, name: NATIVE_LANGUAGE_NAMES[code] ?? code }));

export function detectSystemLanguage(): string {
  const nav = navigator.language;
  const supported = new Set(SUPPORTED_LANGUAGES.map((l) => l.code));

  // 1. Exact match on the full BCP-47 tag (e.g. "pt-BR", "zh-CN").
  if (supported.has(nav)) return nav;

  const parts = nav.toLowerCase().split("-");
  const base = parts[0];

  // 2. Chinese needs script/region disambiguation: several Chinese locales
  //    share the "zh" base. Traditional-script regions (Taiwan, Hong Kong,
  //    Macau) and an explicit Hant script resolve to zh-TW; everything else
  //    (Mainland, Singapore, Hans) resolves to zh-CN. Whichever bundle exists
  //    wins so the fallback is still correct before zh-TW ships.
  if (base === "zh") {
    const traditional =
      parts.includes("hant") ||
      parts.some((p) => p === "tw" || p === "hk" || p === "mo");
    const preferred = traditional ? "zh-TW" : "zh-CN";
    if (supported.has(preferred)) return preferred;
    if (supported.has("zh-CN")) return "zh-CN";
    if (supported.has("zh-TW")) return "zh-TW";
  }

  // 3. Portuguese collapses to the single pt-BR bundle we ship.
  if (base === "pt" && supported.has("pt-BR")) return "pt-BR";

  // 4. General base-tag fallback: the bare base code (e.g. "ja"), then any
  //    locale whose code shares the base (e.g. "de-AT" -> "de").
  if (supported.has(base)) return base;
  const shared = SUPPORTED_LANGUAGES.find(
    (l) => l.code.toLowerCase().split("-")[0] === base,
  );
  return shared?.code ?? "en";
}

i18next.use(initReactI18next).init({
  resources: Object.fromEntries(
    Object.entries(translations).map(([code, translation]) => [
      code,
      { translation },
    ]),
  ),
  lng: "en",
  fallbackLng: "en",
  interpolation: {
    escapeValue: false,
  },
});

export default i18next;
