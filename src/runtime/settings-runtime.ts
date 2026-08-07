import { resolveAppLanguage } from "@/lib/i18n";
import type { AppSettings } from "@/types/ipc";

interface StartupSettingsDependencies {
  getSettings: () => Promise<AppSettings>;
  hydrateAppSettings: (settings: AppSettings) => void;
  changeLanguage: (language: string) => Promise<unknown>;
  detectFallbackLanguage: () => string;
}

export async function loadStartupSettings({
  getSettings,
  hydrateAppSettings,
  changeLanguage,
  detectFallbackLanguage,
}: StartupSettingsDependencies) {
  const settings = await getSettings();

  hydrateAppSettings(settings);

  const language = resolveAppLanguage(
    settings.language,
    detectFallbackLanguage,
  );
  await changeLanguage(language);
}
