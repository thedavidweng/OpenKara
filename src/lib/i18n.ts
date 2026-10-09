import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import en from "../locales/en.json";
import { createLanguageTable } from "./i18n-language";

export type {
  SupportedLanguage,
  SupportedLanguageCode,
  SupportedLanguageNameKey,
} from "./i18n-language";

const localeLoaders = import.meta.glob<{ default: Record<string, unknown> }>([
  "../locales/*.json",
  "!../locales/en.json",
]);

function codeFromPath(path: string): string {
  const file = path.slice(path.lastIndexOf("/") + 1);
  return file.slice(0, -".json".length);
}

export const { SUPPORTED_LANGUAGES, detectSystemLanguage, resolveAppLanguage } =
  createLanguageTable(["en", ...Object.keys(localeLoaders).map(codeFromPath)]);

function setDocumentLanguage(language: string): void {
  if (typeof document === "undefined") return;
  document.documentElement.lang = language;
}

async function ensureLanguageResources(code: string): Promise<void> {
  if (i18next.hasResourceBundle(code, "translation")) {
    return;
  }
  const loader = localeLoaders[`../locales/${code}.json`];
  if (!loader) {
    return;
  }
  const loaded = await loader();
  i18next.addResourceBundle(code, "translation", loaded.default, true, true);
}

const changeLanguage = i18next.changeLanguage.bind(i18next);
i18next.changeLanguage = (lng, callback) => {
  if (typeof lng !== "string" || lng.length === 0) {
    return changeLanguage(lng, callback);
  }
  return ensureLanguageResources(lng)
    .catch(() => undefined)
    .then(() => changeLanguage(lng, callback));
};

i18next.on("languageChanged", setDocumentLanguage);

void i18next.use(initReactI18next).init({
  resources: {
    en: { translation: en },
  },
  lng: "en",
  fallbackLng: "en",
  interpolation: {
    escapeValue: false,
  },
});

export default i18next;
