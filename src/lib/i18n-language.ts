export const LANGUAGE_PRIORITY = [
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

export interface SupportedLanguage {
  code: SupportedLanguageCode;
  nameKey: SupportedLanguageNameKey;
}

function isSupportedLanguageCode(code: string): code is SupportedLanguageCode {
  return (LANGUAGE_PRIORITY as readonly string[]).includes(code);
}

function orderIndex(code: SupportedLanguageCode): number {
  const index = LANGUAGE_PRIORITY.indexOf(code);
  return index === -1 ? Number.POSITIVE_INFINITY : index;
}

export function createLanguageTable(loadedCodes: readonly string[]): {
  SUPPORTED_LANGUAGES: SupportedLanguage[];
  detectSystemLanguage: () => string;
  resolveAppLanguage: (
    persistedLanguage: string | null | undefined,
    detectSystem?: () => string,
  ) => string;
} {
  const SUPPORTED_LANGUAGES = loadedCodes
    .filter(isSupportedLanguageCode)
    .sort((a, b) => orderIndex(a) - orderIndex(b) || a.localeCompare(b))
    .map((code) => ({
      code,
      nameKey: `languageNames.${code}` as SupportedLanguageNameKey,
    }));

  function detectSystemLanguage(): string {
    const nav = navigator.language;
    const supported = new Set<string>(SUPPORTED_LANGUAGES.map((l) => l.code));

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

    if (base === "pt" && supported.has("pt-BR")) return "pt-BR";

    if (supported.has(base)) return base;
    const shared = SUPPORTED_LANGUAGES.find(
      (l) => l.code.toLowerCase().split("-")[0] === base,
    );
    return shared?.code ?? "en";
  }

  function resolveAppLanguage(
    persistedLanguage: string | null | undefined,
    detectSystem: () => string = detectSystemLanguage,
  ): string {
    if (typeof persistedLanguage === "string") {
      const trimmed = persistedLanguage.trim();
      if (
        trimmed.length > 0 &&
        SUPPORTED_LANGUAGES.some((language) => language.code === trimmed)
      ) {
        return trimmed;
      }
    }
    return detectSystem();
  }

  return {
    SUPPORTED_LANGUAGES,
    detectSystemLanguage,
    resolveAppLanguage,
  };
}
