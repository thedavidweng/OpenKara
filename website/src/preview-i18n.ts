import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { createLanguageTable } from "../../src/lib/i18n-language";
import en from "../../src/locales/en.json";
import zhCN from "../../src/locales/zh-CN.json";

export type {
  SupportedLanguage,
  SupportedLanguageCode,
  SupportedLanguageNameKey,
} from "../../src/lib/i18n-language";

const translations = {
  en,
  "zh-CN": zhCN,
} as const;

export const { SUPPORTED_LANGUAGES, detectSystemLanguage, resolveAppLanguage } =
  createLanguageTable(Object.keys(translations));

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
