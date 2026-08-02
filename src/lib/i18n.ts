import i18next from "i18next";
import { initReactI18next } from "react-i18next";

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
] as const;

export type SupportedLanguageCode = (typeof LANGUAGE_PRIORITY)[number];
export type SupportedLanguageNameKey = `languageNames.${SupportedLanguageCode}`;

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

const LOADED_LOCALE_CODES = Object.keys(translations);

function isSupportedLanguageCode(code: string): code is SupportedLanguageCode {
  return (LANGUAGE_PRIORITY as readonly string[]).includes(code);
}

function orderIndex(code: SupportedLanguageCode): number {
  const index = LANGUAGE_PRIORITY.indexOf(code);
  return index === -1 ? Number.POSITIVE_INFINITY : index;
}

export const SUPPORTED_LANGUAGES = LOADED_LOCALE_CODES.slice()
  .filter(isSupportedLanguageCode)
  .sort((a, b) => orderIndex(a) - orderIndex(b) || a.localeCompare(b))
  .map((code) => ({
    code,
    nameKey: `languageNames.${code}` as SupportedLanguageNameKey,
  }));

export function detectSystemLanguage(): string {
  const nav = navigator.language;
  const supported = new Set<string>(SUPPORTED_LANGUAGES.map((l) => l.code));

  // 1. Exact match on the full BCP-47 tag (e.g. "pt-BR", "zh-CN").
  if (supported.has(nav)) return nav;

  const parts = nav.toLowerCase().split("-");
  const base = parts[0];

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

function setDocumentLanguage(language: string): void {
  if (typeof document === "undefined") return;
  document.documentElement.lang = language;
}

i18next.on("languageChanged", setDocumentLanguage);

void i18next.use(initReactI18next).init({
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
